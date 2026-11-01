//! `mtp sdk ...`. Exit codes: 0 fine, 1 a check failed or an I/O or network error, 2 usage,
//! 3 license not accepted or text changed, 4 lock held, 5 a pinned package is no longer a
//! channel-0 android-sdk-license entry in Google's XML.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::ExitCode;

use mtp_sdk::hash::Sha1Hex;
use mtp_sdk::install::RootStatus;
use mtp_sdk::{
    ANDROID_SDK_LICENSE, Authority, Cancel, Client, Error, Event, GPL_3, Home, InstallOptions,
    Installer, LicensePrompt, Manifest, PreAccepted, RemoveTarget, Result, SourcePolicy, Status,
};

use crate::prompt::CliPrompt;
use crate::{InstallArgs, LicenseArgs, RemoveArgs, SdkCommand, StatusArgs};

const PROGRESS_STEP: u64 = 64 << 20;
const GIB: f64 = (1u64 << 30) as f64;
const MIB: f64 = (1u64 << 20) as f64;

pub fn parse_accept(value: &str) -> std::result::Result<(String, Sha1Hex), String> {
    let (id, sha1) = value
        .split_once('=')
        .ok_or_else(|| "expected ID=SHA1".to_string())?;
    let sha1 = Sha1Hex::parse(sha1).map_err(|_| format!("{sha1} is not a SHA-1"))?;
    Ok((id.to_string(), sha1))
}

struct Context {
    installer: Installer,
    home_source: &'static str,
    manifest_source: String,
}

fn context(home: Option<PathBuf>, manifest: Option<PathBuf>) -> Result<Context> {
    let (home, home_source) = match (
        home,
        std::env::var_os("MTPLAYER_HOME"),
        std::env::var_os("LOCALAPPDATA"),
    ) {
        (Some(path), _, _) => (path, "--home"),
        (None, Some(path), _) => (PathBuf::from(path), "MTPLAYER_HOME"),
        (None, None, Some(local)) => (PathBuf::from(local).join("MTPlayer"), "default"),
        (None, None, None) => {
            return Err(Error::Io {
                path: "MTPlayer home".into(),
                source: std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "pass --home, or set MTPLAYER_HOME or LOCALAPPDATA",
                ),
            });
        }
    };
    let home = std::path::absolute(&home).map_err(|source| Error::Io {
        path: home.clone(),
        source,
    })?;
    let policy = SourcePolicy::production();
    let (manifest, manifest_source) = match manifest {
        Some(path) => (
            Manifest::from_path(&path, &policy)?,
            path.display().to_string(),
        ),
        None => (Manifest::embedded()?, "embedded".to_string()),
    };
    Ok(Context {
        installer: Installer::new(Home::new(home), manifest, Client::new(policy)),
        home_source,
        manifest_source,
    })
}

pub fn run(home: Option<PathBuf>, manifest: Option<PathBuf>, command: SdkCommand) -> ExitCode {
    let result = context(home, manifest).and_then(|context| match command {
        SdkCommand::Status(args) => status(&context, &args),
        SdkCommand::Install(args) => install(&context, args),
        SdkCommand::Remove(args) => remove(&context, args),
        SdkCommand::License(args) => license(&context, &args),
    });
    match result {
        Ok(code) => code,
        Err(error) => {
            eprintln!("error: {error}");
            if let Error::LicenseNotAccepted { id } = &error {
                eprintln!(
                    "An install needs a terminal to show the license. Without one, read it with \
                     `mtp sdk license {id} --text`, then run again with \
                     `--accept {id}=<sha1> --on-behalf-of personal|employer`."
                );
            }
            ExitCode::from(error.exit_code())
        }
    }
}

fn usage_error(message: &str) -> ExitCode {
    eprintln!("error: {message}");
    ExitCode::from(2)
}

fn install(context: &Context, args: InstallArgs) -> Result<ExitCode> {
    let mut accepted = BTreeMap::new();
    let known = &context.installer.manifest().licenses;
    for (id, sha1) in args.accept {
        if !known.contains_key(&id) {
            return Ok(usage_error(&format!("unknown license {id}")));
        }
        accepted.insert(id, sha1);
    }
    let mut prompt: Box<dyn LicensePrompt> = match args.on_behalf_of {
        Some(who) => Box::new(PreAccepted::new(accepted, Authority::from(who))),
        None => Box::new(CliPrompt::new()),
    };
    let mut next_progress = BTreeMap::<String, u64>::new();
    let mut print = |event: Event| {
        if let Event::Progress { name, received, .. } = &event {
            let next = next_progress.entry(name.clone()).or_insert(PROGRESS_STEP);
            if received < next {
                return;
            }
            *next = (received / PROGRESS_STEP + 1) * PROGRESS_STEP;
        }
        println!("{event}");
    };
    let options = InstallOptions {
        verify: args.verify,
        keep_downloads: args.keep_downloads,
    };
    context
        .installer
        .install(prompt.as_mut(), options, &mut print, &Cancel::new())?;
    Ok(ExitCode::SUCCESS)
}

fn remove(context: &Context, args: RemoveArgs) -> Result<ExitCode> {
    let target = match (args.set, args.unused) {
        (Some(set), _) => RemoveTarget::Set(set),
        (None, true) => RemoveTarget::Unused,
        (None, false) => RemoveTarget::Downloads,
    };
    let removed = context.installer.remove(target, args.force)?;
    for root in &removed.roots {
        println!("remove   root {root}");
    }
    for tag in &removed.magisk_tags {
        println!("remove   magisk {tag}");
    }
    if removed.downloads {
        println!("remove   downloads");
    }
    for root in &removed.leased {
        println!("keep     root {root} (leased)");
    }
    for name in &removed.skipped {
        println!("keep     tools/magisk/{name} (no record, so not MTPlayer's)");
    }
    if removed.roots.is_empty() && removed.magisk_tags.is_empty() && !removed.downloads {
        println!("nothing to remove");
    }
    Ok(ExitCode::SUCCESS)
}

fn license(context: &Context, args: &LicenseArgs) -> Result<ExitCode> {
    if args.id != ANDROID_SDK_LICENSE && args.id != GPL_3 {
        return Ok(usage_error(&format!("unknown license {}", args.id)));
    }
    let text = context.installer.license_text(&args.id)?;
    println!("license  {}", text.id());
    println!("source   {}", text.source());
    // The hash is what `--accept` takes, so it comes only together with the text it stands for.
    if args.text {
        println!();
        println!("{}", text.text());
        println!();
        println!("sha1     {}", text.sha1());
    } else {
        println!("run with --text to read the license and see its sha1");
    }
    Ok(ExitCode::SUCCESS)
}

fn status(context: &Context, args: &StatusArgs) -> Result<ExitCode> {
    let status = context.installer.status(args.verify)?;
    if args.json {
        let text = serde_json::to_string_pretty(&status).expect("a status always serializes");
        println!("{text}");
    } else {
        print_status(context, &status, args.notices);
    }
    Ok(if status.ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

fn print_status(context: &Context, status: &Status, notices: bool) {
    println!(
        "home     {} ({})",
        status.home.display(),
        context.home_source
    );
    println!("manifest {}, set {}", context.manifest_source, status.set);
    for license in &status.licenses {
        match license.state.as_str() {
            "not-accepted" => println!("license  {} not accepted", license.id),
            state => println!(
                "license  {} {} {}, text {}, authority {}{}",
                license.id,
                state.replace('-', " "),
                license.accepted_at.as_deref().unwrap_or_default(),
                license.text_sha1.as_deref().map_or("", |h| &h[..8]),
                match license.authority.as_deref() {
                    Some("notstated") => "not stated",
                    other => other.unwrap_or_default(),
                },
                if license.differs_from_reviewed {
                    ", differs from reviewed text"
                } else {
                    ""
                },
            ),
        }
    }
    if status.roots.is_empty() {
        println!("root     none installed");
    }
    for root in &status.roots {
        print_root(root, notices);
    }
    let magisk = &status.magisk;
    match (magisk.present, magisk.hash_ok) {
        (false, _) => println!("magisk   {} not installed", magisk.tag),
        (true, true) => println!("magisk   {} sha256 ok", magisk.tag),
        (true, false) => println!("magisk   {} SHA256 MISMATCH", magisk.tag),
    }
    for download in &status.pending_downloads {
        println!(
            "download {} {:.1} MiB",
            download.name,
            download.bytes as f64 / MIB
        );
    }
}

fn print_root(root: &RootStatus, notices: bool) {
    let role = if root.current { "current" } else { "previous" };
    let check = &root.check;
    println!(
        "root     {} {} {} {}",
        root.set,
        root.state,
        role,
        root.path.display()
    );
    if let Some(error) = &root.error {
        println!("  error   {error}");
        println!("  remove it with `mtp sdk remove {}`", root.set);
        return;
    }
    for package in &root.packages {
        println!("  package {package}");
    }
    let result = if check.ok() && check.extra.is_empty() {
        "ok".to_string()
    } else if check.ok() {
        format!("{} extra files", check.extra.len())
    } else {
        format!(
            "{} missing, {} modified",
            check.missing.len(),
            check.modified.len()
        )
    };
    println!(
        "  check   {result}, {} files, {:.2} GiB",
        check.files,
        check.bytes as f64 / GIB
    );
    for path in &check.missing {
        println!("  missing {path}");
    }
    for path in &check.modified {
        println!("  modified {path}");
    }
    for path in &check.extra {
        println!("  extra   {path}");
    }
    if root.leases.is_empty() {
        println!("  leases  none");
    } else {
        println!("  leases  {}", root.leases.join(", "));
    }
    println!("  notices {} files", root.notices.len());
    if notices {
        for notice in &root.notices {
            println!("  notice  {} {}", notice.package, notice.path.display());
        }
    }
}
