//! Hex digests and file hashing.

use std::fmt;
use std::fs::File;
use std::io::Read;
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha1::Sha1;
use sha2::{Digest as _, Sha256};

use crate::error::{Error, IoContext, Result};
use crate::event::Cancel;

const BUFFER: usize = 1 << 20;

macro_rules! hex_type {
    ($name:ident, $len:expr, $label:expr) => {
        #[doc = concat!("A lowercase hexadecimal ", $label, " digest.")]
        #[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);

        impl $name {
            /// Validates and lowercases a digest.
            pub fn parse(value: &str) -> Result<Self> {
                let ok = value.len() == $len && value.bytes().all(|b| b.is_ascii_hexdigit());
                if ok {
                    Ok(Self(value.to_ascii_lowercase()))
                } else {
                    Err(Error::InvalidName(value.to_string()))
                }
            }

            /// The digest as text.
            pub fn as_str(&self) -> &str {
                &self.0
            }

            /// The first eight characters, for short display.
            pub fn prefix(&self) -> &str {
                &self.0[..8]
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl TryFrom<String> for $name {
            type Error = Error;
            fn try_from(value: String) -> Result<Self> {
                Self::parse(&value)
            }
        }

        impl From<$name> for String {
            fn from(value: $name) -> String {
                value.0
            }
        }
    };
}

hex_type!(Sha1Hex, 40, "SHA-1");
hex_type!(Sha256Hex, 64, "SHA-256");

/// A pinned digest of a downloaded file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Digest {
    Sha1(Sha1Hex),
    Sha256(Sha256Hex),
}

impl Digest {
    /// The expected value as text.
    pub fn expected(&self) -> &str {
        match self {
            Digest::Sha1(h) => h.as_str(),
            Digest::Sha256(h) => h.as_str(),
        }
    }

    /// The algorithm's name, for messages.
    pub fn algorithm(&self) -> &'static str {
        match self {
            Digest::Sha1(_) => "sha1",
            Digest::Sha256(_) => "sha256",
        }
    }

    /// Hashes a file with this digest's algorithm and returns the hex result. A zip takes
    /// seconds to hash, so `cancel` is checked between chunks.
    pub fn compute_cancellable(&self, path: &Path, cancel: &Cancel) -> Result<String> {
        self.compute_reader_cancellable(File::open(path).at(path)?, path, cancel)
    }

    /// Hashes an open reader, so that the bytes checked are the bytes the same handle goes
    /// on to read. `path` only names the source in an error.
    pub fn compute_reader_cancellable(
        &self,
        reader: impl Read,
        path: &Path,
        cancel: &Cancel,
    ) -> Result<String> {
        match self {
            Digest::Sha1(_) => digest_reader::<Sha1>(reader, path, cancel),
            Digest::Sha256(_) => digest_reader::<Sha256>(reader, path, cancel),
        }
    }
}

fn digest_reader<D: sha2::Digest>(
    reader: impl Read,
    path: &Path,
    cancel: &Cancel,
) -> Result<String> {
    let mut hasher = D::new();
    for_each_chunk(reader, path, cancel, |chunk| hasher.update(chunk))?;
    Ok(hex::encode(hasher.finalize()))
}

fn for_each_chunk(
    mut reader: impl Read,
    path: &Path,
    cancel: &Cancel,
    mut on_chunk: impl FnMut(&[u8]),
) -> Result<()> {
    let mut buffer = vec![0u8; BUFFER];
    loop {
        cancel.check()?;
        let read = reader.read(&mut buffer).at(path)?;
        if read == 0 {
            return Ok(());
        }
        on_chunk(&buffer[..read]);
    }
}

/// The SHA-1 of a byte string.
pub fn sha1_bytes(bytes: &[u8]) -> Sha1Hex {
    Sha1Hex(hex::encode(Sha1::digest(bytes)))
}

/// The SHA-256 of a byte string.
pub fn sha256_bytes(bytes: &[u8]) -> Sha256Hex {
    Sha256Hex(hex::encode(Sha256::digest(bytes)))
}

/// The SHA-256 of a file.
pub fn sha256_file(path: &Path) -> Result<Sha256Hex> {
    sha256_file_cancellable(path, &Cancel::new())
}

/// The SHA-256 of a file, checking `cancel` between chunks.
pub fn sha256_file_cancellable(path: &Path, cancel: &Cancel) -> Result<Sha256Hex> {
    digest_reader::<Sha256>(File::open(path).at(path)?, path, cancel).map(Sha256Hex)
}

/// A file's length, CRC-32 and SHA-256, computed in one read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileHash {
    pub len: u64,
    pub crc32: u32,
    pub sha256: Sha256Hex,
}

/// Hashes a file for the compare against its zip entry.
pub fn hash_file(path: &Path) -> Result<FileHash> {
    let mut crc = crc32fast::Hasher::new();
    let mut sha = Sha256::new();
    let mut len = 0u64;
    for_each_chunk(File::open(path).at(path)?, path, &Cancel::new(), |chunk| {
        crc.update(chunk);
        sha.update(chunk);
        len += chunk.len() as u64;
    })?;
    Ok(FileHash {
        len,
        crc32: crc.finalize(),
        sha256: Sha256Hex(hex::encode(sha.finalize())),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_digests() {
        assert_eq!(
            sha1_bytes(b"abc").as_str(),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
        assert_eq!(
            sha256_bytes(b"abc").as_str(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn a_set_cancel_stops_hashing_before_the_first_chunk() {
        let digest = Digest::Sha1(sha1_bytes(b"abc"));
        let cancel = Cancel::new();
        let hash = digest
            .compute_reader_cancellable(&b"abc"[..], Path::new("abc"), &cancel)
            .unwrap();
        assert_eq!(hash, digest.expected());

        cancel.cancel();
        let err = digest
            .compute_reader_cancellable(&b"abc"[..], Path::new("abc"), &cancel)
            .unwrap_err();
        assert!(matches!(err, Error::Cancelled), "{err}");
    }

    #[test]
    fn parse_rejects_wrong_lengths_and_non_hex() {
        assert!(Sha1Hex::parse("abc").is_err());
        assert!(Sha1Hex::parse(&"g".repeat(40)).is_err());
        assert_eq!(
            Sha1Hex::parse(&"A".repeat(40)).unwrap().as_str(),
            "a".repeat(40)
        );
    }
}
