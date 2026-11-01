//! Where downloads may come from, and the HTTP client that enforces it.
//!
//! Redirects are followed by hand so that every hop passes the policy before a socket
//! opens. No configuration value can widen the policy.

use std::io::Read;
use std::time::Duration;

use ureq::http::Uri;
use ureq::tls::{RootCerts, TlsConfig};
use ureq::{Agent, Body};

use crate::error::{Error, Result};
use crate::event::Cancel;
use crate::fsutil;

const MAX_REDIRECTS: u32 = 5;
const GITHUB_ORIGIN: &str = "https://github.com";
const GOOGLE_ORIGIN: &str = "https://dl.google.com";

/// The allowed scheme and exact hosts for every request and redirect hop.
#[derive(Clone, Debug)]
pub struct SourcePolicy {
    scheme: &'static str,
    hosts: &'static [&'static str],
    port: Option<u16>,
}

impl SourcePolicy {
    /// What the product may talk to: Google's repository and Magisk's GitHub release,
    /// whose download link redirects to GitHub's asset hosts.
    pub fn production() -> Self {
        Self {
            scheme: "https",
            hosts: &[
                "dl.google.com",
                "github.com",
                "release-assets.githubusercontent.com",
                "objects.githubusercontent.com",
            ],
            port: None,
        }
    }

    /// The production hosts plus the two GitHub hosts the pin tool reads release data from.
    pub fn pin_update() -> Self {
        Self {
            scheme: "https",
            hosts: &[
                "dl.google.com",
                "github.com",
                "release-assets.githubusercontent.com",
                "objects.githubusercontent.com",
                "api.github.com",
                "raw.githubusercontent.com",
            ],
            port: None,
        }
    }

    /// A fixture server on this machine. Exists only for tests.
    #[cfg(feature = "test-support")]
    pub fn loopback(port: u16) -> Self {
        Self {
            scheme: "http",
            hosts: &["127.0.0.1"],
            port: Some(port),
        }
    }

    fn origin(&self, production: &str) -> String {
        match self.port {
            Some(port) => format!("{}://{}:{port}", self.scheme, self.hosts[0]),
            None => production.to_string(),
        }
    }

    /// Every pinned Google URL must start with this.
    pub fn google_prefix(&self) -> String {
        format!("{}/android/repository/", self.origin(GOOGLE_ORIGIN))
    }

    /// The pinned Magisk URL must start with this.
    pub fn github_release_prefix(&self, repo: &str, tag: &str) -> String {
        format!(
            "{}/{repo}/releases/download/{tag}/",
            self.origin(GITHUB_ORIGIN)
        )
    }

    /// Refuses a URL unless its scheme, host and port are allowed. Performs no I/O.
    pub fn check(&self, url: &str) -> Result<()> {
        let insecure = || Error::InsecureUrl {
            url: display_url(url),
        };
        let uri: Uri = url.parse().map_err(|_| insecure())?;
        if uri.scheme_str() != Some(self.scheme) {
            return Err(insecure());
        }
        let authority = uri.authority().ok_or_else(insecure)?;
        if authority.as_str().contains('@') {
            return Err(insecure());
        }
        let host = authority.host();
        if !self
            .hosts
            .iter()
            .any(|allowed| allowed.eq_ignore_ascii_case(host))
        {
            return Err(Error::RedirectNotAllowed {
                host: host.to_string(),
            });
        }
        let port_ok = match (self.port, authority.port_u16()) {
            (Some(allowed), Some(port)) => allowed == port,
            (Some(_), None) => false,
            (None, Some(port)) => port == 443,
            (None, None) => true,
        };
        if port_ok { Ok(()) } else { Err(insecure()) }
    }
}

/// A URL without its query or fragment, safe to print: GitHub's signed links carry
/// credentials in the query string.
pub fn display_url(url: &str) -> String {
    url.split(['?', '#']).next().unwrap_or(url).to_string()
}

fn host_of(url: &str) -> String {
    url.parse::<Uri>()
        .ok()
        .and_then(|u| u.host().map(str::to_string))
        .unwrap_or_default()
}

fn path_of(url: &str) -> String {
    url.parse::<Uri>()
        .map(|u| u.path().to_string())
        .unwrap_or_default()
}

/// Resolves a `Location` header against the URL that sent it.
fn resolve_location(current: &str, location: &str) -> Result<String> {
    if location.contains("://") {
        return Ok(location.to_string());
    }
    let uri: Uri = current.parse().map_err(|_| Error::InsecureUrl {
        url: display_url(current),
    })?;
    let scheme = uri.scheme_str().unwrap_or_default();
    let authority = uri.authority().map(|a| a.as_str()).unwrap_or_default();
    if let Some(rest) = location.strip_prefix("//") {
        return Ok(format!("{scheme}://{rest}"));
    }
    if location.starts_with('/') {
        return Ok(format!("{scheme}://{authority}{location}"));
    }
    let path = uri.path();
    let dir = path.rfind('/').map_or("/", |i| &path[..=i]);
    Ok(format!("{scheme}://{authority}{dir}{location}"))
}

/// Removes any URL query that an underlying error message might repeat.
fn redact(message: &str) -> String {
    message
        .split(' ')
        .map(|word| {
            if word.contains("://") {
                display_url(word)
            } else {
                word.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The blocking HTTP client. Holds no async runtime; the daemon calls it on a blocking task.
#[derive(Clone, Debug)]
pub struct Client {
    agent: Agent,
    policy: SourcePolicy,
    pub(crate) retries: u32,
    pub(crate) backoff: Duration,
}

impl Client {
    /// A client limited to `policy`. Certificates come from the Windows store, so a
    /// corporate TLS-inspecting proxy keeps working. A loopback policy never goes through a
    /// proxy, because a proxy can't reach this machine's own loopback address.
    pub fn new(policy: SourcePolicy) -> Self {
        let config = Agent::config_builder()
            .proxy(if policy.port.is_some() {
                None
            } else {
                ureq::Proxy::try_from_env()
            })
            .https_only(policy.scheme == "https")
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_connect(Some(Duration::from_secs(20)))
            .timeout_recv_response(Some(Duration::from_secs(30)))
            .timeout_per_read(Some(Duration::from_secs(60)))
            .user_agent(concat!(
                "mtp-sdk/",
                env!("CARGO_PKG_VERSION"),
                " (+https://github.com/tiennm99dev/mtplayer)"
            ))
            .tls_config(
                TlsConfig::builder()
                    .root_certs(RootCerts::PlatformVerifier)
                    .build(),
            )
            .build();
        Self {
            agent: config.into(),
            policy,
            retries: 3,
            backoff: Duration::from_secs(2),
        }
    }

    /// Changes how an interrupted transfer retries.
    pub fn with_retries(mut self, retries: u32, backoff: Duration) -> Self {
        self.retries = retries;
        self.backoff = backoff;
        self
    }

    /// The policy this client enforces.
    pub fn policy(&self) -> &SourcePolicy {
        &self.policy
    }

    /// Sends a GET and follows redirects by hand, re-checking every hop against the
    /// policy and re-sending `Range` on each. Returns the final response and its URL.
    pub(crate) fn send(
        &self,
        url: &str,
        range_from: Option<u64>,
    ) -> Result<(ureq::http::Response<Body>, String)> {
        let mut current = url.to_string();
        for _ in 0..=MAX_REDIRECTS {
            self.policy.check(&current)?;
            let mut request = self.agent.get(&current);
            if let Some(from) = range_from {
                request = request.header("Range", format!("bytes={from}-"));
            }
            let response = request.call().map_err(|e| Error::Network {
                host: host_of(&current),
                message: redact(&e.to_string()),
            })?;
            let status = response.status().as_u16();
            if !matches!(status, 301 | 302 | 303 | 307 | 308) {
                return Ok((response, current));
            }
            let location = response
                .headers()
                .get("location")
                .and_then(|v| v.to_str().ok())
                .ok_or_else(|| Error::BadResponse {
                    host: host_of(&current),
                    path: path_of(&current),
                    reason: "a redirect without a Location".into(),
                })?;
            current = resolve_location(&current, location)?;
        }
        Err(Error::TooManyRedirects { host: host_of(url) })
    }

    /// Fetches a small text document, refusing one larger than `max_bytes`. A failure that
    /// can pass is retried like a download's.
    pub fn get_text(&self, url: &str, max_bytes: u64) -> Result<String> {
        self.get_text_cancellable(url, max_bytes, &Cancel::new())
    }

    /// `get_text`, with `cancel` checked before each attempt and during the waits between them.
    pub fn get_text_cancellable(
        &self,
        url: &str,
        max_bytes: u64,
        cancel: &Cancel,
    ) -> Result<String> {
        let mut attempt = 0;
        loop {
            cancel.check()?;
            match self.get_text_once(url, max_bytes) {
                Err(e) if e.is_transient() && attempt < self.retries => {
                    attempt += 1;
                    fsutil::sleep_unless_cancelled(e.retry_delay(self.backoff), cancel)?;
                }
                other => return other,
            }
        }
    }

    fn get_text_once(&self, url: &str, max_bytes: u64) -> Result<String> {
        let (response, final_url) = self.send(url, None)?;
        let host = host_of(&final_url);
        let path = path_of(&final_url);
        let status = response.status().as_u16();
        if status != 200 {
            return Err(Error::Http {
                status,
                host,
                path,
                retry_after: retry_after(&response),
            });
        }
        let mut bytes = Vec::new();
        let mut body = response.into_body();
        body.as_reader()
            .take(max_bytes + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| Error::Network {
                host: host.clone(),
                message: redact(&e.to_string()),
            })?;
        if bytes.len() as u64 > max_bytes {
            return Err(Error::BadResponse {
                host,
                path,
                reason: format!("larger than {max_bytes} bytes"),
            });
        }
        String::from_utf8(bytes).map_err(|_| Error::BadResponse {
            host,
            path,
            reason: "not UTF-8 text".into(),
        })
    }
}

pub(crate) fn host_and_path(url: &str) -> (String, String) {
    (host_of(url), path_of(url))
}

/// A numeric `Retry-After`. The date form is ignored, and the caller's backoff applies.
pub(crate) fn retry_after(response: &ureq::http::Response<Body>) -> Option<u64> {
    response
        .headers()
        .get("retry-after")?
        .to_str()
        .ok()?
        .trim()
        .parse()
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_production_policy_refuses_plain_http_without_any_socket() {
        let err = SourcePolicy::production()
            .check("http://dl.google.com/android/repository/x.zip")
            .unwrap_err();
        assert!(matches!(err, Error::InsecureUrl { .. }), "{err}");
    }

    #[test]
    fn the_production_policy_allows_exact_hosts_only() {
        let policy = SourcePolicy::production();
        assert!(policy.check("https://dl.google.com/x.zip").is_ok());
        assert!(policy.check("https://DL.google.com:443/x.zip").is_ok());
        for bad in [
            "https://dl.google.com.evil.example/x.zip",
            "https://evil.example/dl.google.com/x.zip",
            "https://raw.githubusercontent.com/x",
            "https://api.github.com/x",
        ] {
            assert!(
                matches!(policy.check(bad), Err(Error::RedirectNotAllowed { .. })),
                "{bad}"
            );
        }
        for bad in [
            "https://dl.google.com:8443/x.zip",
            "https://user@dl.google.com/x.zip",
            "ftp://dl.google.com/x.zip",
            "not a url",
        ] {
            assert!(
                matches!(policy.check(bad), Err(Error::InsecureUrl { .. })),
                "{bad}"
            );
        }
    }

    #[test]
    fn the_pin_tool_policy_adds_only_its_two_hosts() {
        let policy = SourcePolicy::pin_update();
        assert!(policy.check("https://api.github.com/repos").is_ok());
        assert!(policy.check("https://raw.githubusercontent.com/x").is_ok());
        assert!(policy.check("https://evil.example/x").is_err());
    }

    #[test]
    fn locations_resolve_absolute_and_relative() {
        let base = "https://github.com/a/b/c.apk?x=1";
        for (location, expected) in [
            ("https://dl.google.com/z", "https://dl.google.com/z"),
            ("//dl.google.com/z", "https://dl.google.com/z"),
            ("/z", "https://github.com/z"),
            ("z", "https://github.com/a/b/z"),
        ] {
            assert_eq!(resolve_location(base, location).unwrap(), expected);
        }
    }

    #[test]
    fn printed_urls_and_messages_carry_no_query_string() {
        let signed = "https://release-assets.githubusercontent.com/a/b.apk?sig=SECRET&se=1";
        assert_eq!(
            display_url(signed),
            "https://release-assets.githubusercontent.com/a/b.apk"
        );
        assert!(!redact(&format!("failed for {signed} today")).contains("SECRET"));
    }
}
