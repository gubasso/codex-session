//! `self show-local` command.
#![allow(clippy::missing_errors_doc)]

use crate::adapters::fs::Fs as _;

/// Print local-only sections that would be preserved by a merge.
pub(crate) fn run(
    ctx: &crate::context::AppContext,
    _args: crate::cli::self_show_local::SelfShowLocalArgs,
) -> Result<(), crate::error::AppError> {
    if !ctx.fs.exists(&ctx.paths.target) {
        return Ok(());
    }
    let base = if ctx.fs.exists(&ctx.paths.base) {
        ctx.fs.read_to_string(&ctx.paths.base)?
    } else {
        String::new()
    };
    let target = ctx.fs.read_to_string(&ctx.paths.target)?;
    let local = crate::domain::config_merge::extract_local_sections(&base, &target);
    ctx.ui.print_local_sections(&local)?;
    Ok(())
}
