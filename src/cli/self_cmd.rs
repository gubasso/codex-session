//! `self` subcommand: parse-shape.
//!
//! Holds the clap derive for the `self` subtree only. No I/O, no business logic.

use crate::cli::{
    self_config_merge::SelfConfigMergeArgs, self_config_status::SelfConfigStatusArgs,
    self_help::SelfHelpArgs, self_show_local::SelfShowLocalArgs, self_version::SelfVersionArgs,
};

#[derive(Debug, clap::Args)]
pub(crate) struct SelfArgs {
    #[command(subcommand)]
    pub(crate) command: Option<SelfCommand>,
}

#[derive(Debug, clap::Subcommand)]
pub(crate) enum SelfCommand {
    Help(SelfHelpArgs),
    Version(SelfVersionArgs),
    #[command(name = "config-status")]
    ConfigStatus(SelfConfigStatusArgs),
    #[command(name = "config-merge")]
    ConfigMerge(SelfConfigMergeArgs),
    #[command(name = "show-local")]
    ShowLocal(SelfShowLocalArgs),
}
