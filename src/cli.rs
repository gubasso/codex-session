//! CLI parse-shape modules.
//!
//! Holds per-verb `clap::Args` structs only. No I/O, no business logic, no
//! dispatch; dispatch lives in `main.rs`.
use clap::{ArgAction, Parser, Subcommand, ValueEnum};

pub(crate) mod self_config_merge;
pub(crate) mod self_config_status;
pub(crate) mod self_help;
pub(crate) mod self_show_local;
pub(crate) mod self_version;

/// Wrapper-only parser for the `self` subtree.
#[derive(Debug, Parser)]
#[command(
    bin_name = "codex-session self",
    disable_help_subcommand = true,
    disable_version_flag = true
)]
pub(crate) struct SelfCli {
    /// Wrapper-only global flags for the `self` subtree.
    #[command(flatten)]
    pub(crate) global: SelfGlobalArgs,

    /// Wrapper-owned `self` subcommand.
    #[command(subcommand)]
    pub(crate) command: Option<SelfCommand>,
}

/// Global wrapper-only flags under `self`.
#[derive(Debug, clap::Args, Default, Clone, Copy)]
pub(crate) struct SelfGlobalArgs {
    /// Increase wrapper log verbosity (-v info, -vv debug, -vvv trace).
    #[arg(short, long, action = ArgAction::Count, global = true)]
    pub(crate) verbose: u8,

    /// Mirror wrapper logs to stderr in addition to the log file.
    #[arg(long, global = true)]
    pub(crate) log_stderr: bool,
}

/// Wrapper-owned `self` verbs.
#[derive(Debug, Subcommand)]
pub(crate) enum SelfCommand {
    /// Show wrapper help.
    Help(self_help::SelfHelpArgs),
    /// Print wrapper and child version details.
    Version(self_version::SelfVersionArgs),
    /// Show config merge status.
    ConfigStatus(self_config_status::SelfConfigStatusArgs),
    /// Force a config merge.
    ConfigMerge(self_config_merge::SelfConfigMergeArgs),
    /// Print machine-local TOML sections preserved by merges.
    ShowLocal(self_show_local::SelfShowLocalArgs),
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
