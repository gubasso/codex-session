//! `doctor` subcommand: parse-shape.
//!
//! What this is: clap parser type for `codex-session doctor`.
//! What this is not: the check implementations or rendering; those
//! live in `src/commands/doctor/`.

/// Run full validation of the codex-session config setup.
#[derive(Debug, Clone, Copy, Default, clap::Args)]
pub(crate) struct DoctorArgs {
    /// Validate every config recipe in `config-recipes/` instead of just the active one.
    #[arg(long = "all-config-recipes")]
    pub(crate) all_config_recipes: bool,

    /// Include resolved merged env in the report (secrets redacted by
    /// key suffix: `*_TOKEN`, `*_SECRET`, `*_KEY`, `*_PASSWORD`).
    #[arg(long)]
    pub(crate) show_env: bool,

    /// Run network-dependent checks (token probe, quota connectivity) in parallel.
    /// Without this flag, doctor performs only fast local checks.
    #[arg(long)]
    pub(crate) online: bool,
}
