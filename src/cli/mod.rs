//! Root CLI parser.
//!
//! What this is: clap derive structs for the wrapper's command tree.
//! What this is not: business logic or dispatch; those live outside `cli/`.

use clap::{ArgAction, Parser, Subcommand};
use std::ffi::OsString;

pub(crate) mod argv;
pub(crate) mod config;
pub(crate) mod exit;
pub(crate) mod version;

use clap::ValueEnum;

/// Root wrapper CLI.
#[derive(Debug, Parser)]
#[command(
    name = "codex-session",
    bin_name = "codex-session",
    about = "Wrapper around `codex` with config-merge and machine-local preservation.",
    long_about = None,
    disable_help_subcommand = true,
    disable_version_flag = true,
    allow_external_subcommands = true,
    subcommand_negates_reqs = true
)]
pub(crate) struct Cli {
    /// Global wrapper options.
    #[command(flatten)]
    pub(crate) global: GlobalArgs,

    /// Wrapper-owned subcommands or passthrough external subcommands.
    #[command(subcommand)]
    pub(crate) command: Option<Commands>,
}

/// Top-level wrapper flags.
#[derive(Debug, Default, Clone, clap::Args)]
pub(crate) struct GlobalArgs {
    /// Increase wrapper log verbosity (-v info, -vv debug, -vvv trace).
    #[arg(short, long, action = ArgAction::Count, global = true)]
    pub(crate) verbose: u8,

    /// Mirror wrapper logs to stderr in addition to the log file.
    #[arg(long, global = true)]
    pub(crate) log_stderr: bool,

    /// Print wrapper + child version and exit.
    #[arg(long, global = true)]
    pub(crate) version: bool,

    /// Override the user/project config file with an explicit path.
    #[arg(long, value_name = "PATH", global = true)]
    pub(crate) config: Option<camino::Utf8PathBuf>,
}

/// Root subcommand set.
#[derive(Debug, Subcommand)]
pub(crate) enum Commands {
    /// Print wrapper and child version details.
    Version(version::VersionArgs),

    /// Print wrapper help.
    #[command(name = "help")]
    Help,

    /// Operate on config state managed by the wrapper.
    Config(config::ConfigArgs),

    /// Forward any unknown top-level verb to the wrapped `codex` binary.
    #[command(external_subcommand)]
    External(Vec<OsString>),
}

/// Output format for wrapper-owned read commands.
#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, ValueEnum)]
pub(crate) enum OutputFormat {
    /// Human-readable text output.
    #[default]
    Text,
    /// Machine-readable JSON output.
    Json,
}
