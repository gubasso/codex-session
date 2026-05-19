//! `completion` handler.
//!
//! What this is: writes a shell-completion script for the requested
//! shell to stdout.
//! What this is not: a static completion file installer — that is a
//! `justfile` recipe in the repo, not a built-in.

#![allow(
    clippy::needless_pass_by_value,
    clippy::result_large_err,
    clippy::unnecessary_wraps
)]

use clap::CommandFactory as _;

/// Execute the `completion <shell>` verb.
pub(crate) fn run(
    ctx: &crate::context::AppContext,
    args: crate::cli::completion::CompletionArgs,
) -> Result<(), crate::error::AppError> {
    let mut cmd = crate::cli::Cli::command();
    let bin = cmd.get_name().to_string();
    let mut output = Vec::new();
    clap_complete::generate(args.shell, &mut cmd, bin, &mut output);
    ctx.ui.write_bytes(&output)?;
    Ok(())
}
