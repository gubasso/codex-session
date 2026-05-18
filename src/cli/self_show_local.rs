//! `self show-local` parse-shape.

/// Zero-argument `self show-local`.
#[derive(Debug, Clone, Copy, clap::Args)]
pub(crate) struct SelfShowLocalArgs {
    /// Output format for the preserved local sections.
    #[arg(long, value_enum, default_value_t = crate::cli::OutputFormat::Text)]
    pub(crate) format: crate::cli::OutputFormat,
}
