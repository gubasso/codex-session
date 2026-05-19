//! `version` subcommand: parse-shape.
//!
//! What this is: clap derive struct for `codex-session version`.
//! What this is not: the runtime formatter; that lives in
//! `src/commands/version.rs`.

/// Zero-argument `version`.
#[derive(Debug, Clone, Copy, Default, clap::Args)]
pub(crate) struct VersionArgs {
    /// Output format for version details.
    #[arg(long, value_enum, default_value_t = crate::cli::OutputFormat::Text)]
    pub(crate) format: crate::cli::OutputFormat,
}
