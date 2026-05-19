//! `config` subcommand: parse-shape.
//!
//! What this is: the parser types for config-related wrapper commands.
//! What this is not: config loading or merge logic.

/// `config` command group.
#[derive(Debug, clap::Args)]
pub(crate) struct ConfigArgs {
    /// Concrete config operation to perform.
    #[command(subcommand)]
    pub(crate) command: ConfigCommand,
}

/// `config` subcommands.
#[derive(Debug, clap::Subcommand)]
pub(crate) enum ConfigCommand {
    /// Print the current config merge status.
    Status(ConfigStatusArgs),
}

/// Zero-argument `config status`.
#[derive(Debug, Clone, Copy, clap::Args)]
pub(crate) struct ConfigStatusArgs {
    /// Output format for the reported status.
    #[arg(long, value_enum, default_value_t = crate::cli::OutputFormat::Text)]
    pub(crate) format: crate::cli::OutputFormat,
}
