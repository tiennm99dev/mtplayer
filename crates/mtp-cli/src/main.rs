//! The `mtp` command line.

mod prompt;
mod sdk_cmd;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand, ValueEnum};
use mtp_sdk::{Authority, SetId};

#[derive(Parser)]
#[command(
    name = "mtp",
    version,
    about = "MTPlayer: an Android emulator manager for app testing"
)]
struct Cli {
    /// The MTPlayer home folder. Defaults to MTPLAYER_HOME, then %LOCALAPPDATA%\MTPlayer.
    #[arg(long, global = true, value_name = "DIR")]
    home: Option<PathBuf>,
    /// A manifest to use instead of the one in this release, for development and tests.
    #[arg(long, global = true, value_name = "FILE")]
    manifest: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Manage Google's packages and the Magisk binary.
    Sdk {
        #[command(subcommand)]
        command: SdkCommand,
    },
}

#[derive(Subcommand)]
enum SdkCommand {
    /// Show licenses, SDK roots, Magisk and pending downloads. Makes no network request.
    Status(StatusArgs),
    /// Accept the licenses, then download, check and install the pinned packages.
    Install(InstallArgs),
    /// Remove an SDK root, unused data, or the downloads.
    Remove(RemoveArgs),
    /// Show where a license comes from and, with --text, its full text and its hash.
    License(LicenseArgs),
}

#[derive(Args)]
struct StatusArgs {
    /// Hash every file and list each difference.
    #[arg(long)]
    verify: bool,
    /// Print JSON.
    #[arg(long)]
    json: bool,
    /// List every LICENSE and NOTICE file the packages ship.
    #[arg(long)]
    notices: bool,
}

#[derive(Args)]
struct InstallArgs {
    /// Hash already installed files instead of checking only their sizes.
    #[arg(long)]
    verify: bool,
    /// Keep the zips after the install finishes.
    #[arg(long)]
    keep_downloads: bool,
    /// Accept exactly this text of a license, without a prompt. Repeat for each pending
    /// license. Get the hash from `mtp sdk license <id> --text`.
    #[arg(long, value_name = "ID=SHA1", value_parser = sdk_cmd::parse_accept, requires = "on_behalf_of")]
    accept: Vec<(String, mtp_sdk::hash::Sha1Hex)>,
    /// On whose behalf --accept is given. "employer" states you have full legal authority
    /// to bind your employer to the license. There is no default.
    #[arg(long, value_enum, value_name = "WHO", requires = "accept")]
    on_behalf_of: Option<OnBehalfOf>,
}

#[derive(Clone, Copy, ValueEnum)]
enum OnBehalfOf {
    Personal,
    Employer,
}

impl From<OnBehalfOf> for Authority {
    fn from(value: OnBehalfOf) -> Self {
        match value {
            OnBehalfOf::Personal => Authority::Personal,
            OnBehalfOf::Employer => Authority::Employer,
        }
    }
}

#[derive(Args)]
#[command(group = clap::ArgGroup::new("target").required(true))]
struct RemoveArgs {
    /// The set to remove, such as 37.2.13-36r2-4d7268a-829c076.
    #[arg(group = "target", value_parser = |s: &str| SetId::parse(s).map_err(|e| e.to_string()))]
    set: Option<SetId>,
    /// Remove every root that is neither current nor leased, and old Magisk versions.
    #[arg(long, group = "target")]
    unused: bool,
    /// Remove the zips and partial files MTPlayer downloaded, including ones from old pins.
    #[arg(long, group = "target")]
    downloads: bool,
    /// Remove a root even though instances lease it.
    #[arg(long)]
    force: bool,
}

#[derive(Args)]
struct LicenseArgs {
    /// android-sdk-license or gpl-3.0.
    id: String,
    /// Print the full text and its hash.
    #[arg(long)]
    text: bool,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::Sdk { command } => sdk_cmd::run(cli.home, cli.manifest, command),
    }
}
