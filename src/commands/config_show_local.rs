//! `config show-local` command.
#![allow(clippy::missing_errors_doc)]

use crate::adapters::fs::Fs as _;

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct ShowLocalView {
    pub(crate) source_path: String,
    pub(crate) local_sections: String,
}

/// Print local-only sections that would be preserved by a merge.
pub(crate) fn run(
    ctx: &crate::context::AppContext,
    args: crate::cli::config::ShowLocalArgs,
) -> Result<(), crate::error::AppError> {
    tracing::info!(op = "config.show-local", status = "start");
    if !ctx.fs.exists(ctx.paths().target_config.as_std_path()) {
        if args.format == crate::cli::OutputFormat::Json {
            let view = ShowLocalView {
                source_path: ctx.paths().target_config.to_string(),
                local_sections: String::new(),
            };
            ctx.ui.write_show_local(&view, args.format)?;
        }
        tracing::info!(op = "config.show-local", status = "ok");
        return Ok(());
    }
    let base = if ctx.fs.exists(ctx.paths().base_config.as_std_path()) {
        ctx.fs
            .read_to_string(ctx.paths().base_config.as_std_path())?
    } else {
        String::new()
    };
    let target = ctx
        .fs
        .read_to_string(ctx.paths().target_config.as_std_path())?;
    let local = crate::domain::config_merge::extract_local_sections(&base, &target);
    let view = ShowLocalView {
        source_path: ctx.paths().target_config.to_string(),
        local_sections: local,
    };
    ctx.ui.write_show_local(&view, args.format)?;
    tracing::info!(op = "config.show-local", status = "ok");
    Ok(())
}
