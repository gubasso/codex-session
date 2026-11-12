//! `completion` handler.
//!
//! What this is: writes a shell-completion script for the requested
//! shell to stdout.
//! What this is not: a static completion file installer. A package
//! (`nix/package.nix`) writes the script into its output.
//!
//! The script comes only from the clap definition, so `main` runs this
//! verb before it loads the configuration, creates the log directory, or
//! prunes session state. It then works where `HOME` is not writable, such
//! as a Nix build sandbox.

#![allow(clippy::result_large_err)]

use clap::CommandFactory as _;

/// Execute the `completion <shell>` verb.
pub(crate) fn run(
    ui: &crate::ui::Ui,
    args: &crate::cli::completion::CompletionArgs,
) -> Result<(), crate::error::AppError> {
    let mut cmd = crate::cli::Cli::command();
    let bin = cmd.get_name().to_string();
    let mut output = Vec::new();
    clap_complete::generate(args.shell, &mut cmd, bin, &mut output);
    ui.write_bytes(&output)?;
    Ok(())
}
