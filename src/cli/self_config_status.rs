//! `self config-status` parse-shape.

/// Zero-argument `self config-status`.
#[derive(Debug, Clone, Copy, clap::Args)]
pub(crate) struct SelfConfigStatusArgs {
    /// Output format for the reported status.
    #[arg(long, value_enum, default_value_t = crate::cli::OutputFormat::Text)]
    pub(crate) format: crate::cli::OutputFormat,
}
