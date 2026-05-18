//! `version` subcommand: parse-shape.

/// Zero-argument `version`.
#[derive(Debug, Clone, Copy, Default, clap::Args)]
pub(crate) struct VersionArgs {
    /// Output format for version details.
    #[arg(long, value_enum, default_value_t = crate::cli::OutputFormat::Text)]
    pub(crate) format: crate::cli::OutputFormat,
}
