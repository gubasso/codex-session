//! `self version` parse-shape.

/// Zero-argument `self version`.
#[derive(Debug, Clone, Copy, clap::Args)]
pub(crate) struct SelfVersionArgs {
    /// Output format for version details.
    #[arg(long, value_enum, default_value_t = crate::cli::OutputFormat::Text)]
    pub(crate) format: crate::cli::OutputFormat,
}
