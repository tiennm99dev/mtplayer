//! Provisions Google's stock emulator and Android 16 image, and Magisk's binary, onto a
//! user's machine. MTPlayer ships none of them: each user accepts each license, then this
//! crate downloads the pinned versions, checks them, and stores them unmodified.
//!
//! The crate is blocking and holds no async runtime. The daemon calls it on a blocking
//! task and forwards `Event`s; cancellation goes through `Cancel`.
#![deny(unsafe_code)]

pub mod archive;
pub mod download;
pub mod error;
pub mod event;
mod fsutil;
pub mod hash;
pub mod home;
pub mod install;
pub mod license;
pub mod magisk;
pub mod manifest;
pub mod repository;
pub mod source;
pub mod store;
pub mod windows;

pub use error::{Error, Result};
pub use event::{Cancel, Event};
pub use home::{DamagedRoot, Home, HomeLock};
pub use install::{InstallOptions, InstallReport, Installer, Plan, RemoveTarget, Removed, Status};
pub use license::{
    ANDROID_SDK_LICENSE, Authority, Decision, GPL_3, LicensePrompt, LicenseRecord, LicenseState,
    LicenseStore, LicenseText, PreAccepted, Via, gpl3_text,
};
pub use manifest::{MagiskPin, Manifest, PackagePin, SetId};
pub use repository::{RemoteArchive, Repository, check_pin};
pub use source::{Client, SourcePolicy, display_url};
pub use store::{CheckReport, Notice, RootState, SdkRoot};
