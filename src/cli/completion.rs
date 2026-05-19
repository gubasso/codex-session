//! `completion` subcommand: parse-shape.
//!
//! What this is: clap derive struct for `codex-session completion <SHELL>`.
//! What this is not: the generator — that lives in
//! `src/commands/completion.rs`.

/// Parse-shape for `codex-session completion <SHELL>`.
#[derive(Debug, clap::Args)]
pub(crate) struct CompletionArgs {
    /// Target shell.
    #[arg(value_enum)]
    pub(crate) shell: clap_complete::Shell,
}
