//! Verb dispatch.
//!
//! What this is: routes a parsed `Cli` to the matching command handler.
//! What this is not: business logic — every arm is a thin call to a
//! per-verb handler.

#![allow(clippy::result_large_err)]

use crate::{cli, commands, context, error};

/// Route a parsed root `Cli` to its handler. Returns the process exit code
/// on success (most commands return 0; `doctor` can return 0/1/3).
pub(crate) fn run(ctx: &context::AppContext, cli: cli::Cli) -> Result<u8, error::AppError> {
    if cli.global.version {
        return commands::version::run(
            ctx,
            cli::version::VersionArgs {
                format: cli.global.format.unwrap_or_default(),
            },
        )
        .map(|()| 0);
    }
    match cli.command {
        Some(cli::Commands::Version(args)) => commands::version::run(ctx, args).map(|()| 0),
        Some(cli::Commands::Completion(args)) => commands::completion::run(ctx, args).map(|()| 0),
        Some(cli::Commands::Config(args)) => run_config(ctx, &args).map(|()| 0),
        Some(cli::Commands::Profile(args)) => run_profile(ctx, args).map(|()| 0),
        Some(cli::Commands::Doctor(args)) => commands::doctor::run(ctx, args),
        Some(cli::Commands::External(argv)) => commands::pass_through::run(ctx, &argv).map(|()| 0),
        // No subcommand: forward to `codex` with an empty child argv
        // (launches the Codex TUI when `codex` is resolvable).
        None => commands::pass_through::run(ctx, &[]).map(|()| 0),
    }
}

fn run_config(
    ctx: &context::AppContext,
    args: &cli::config::ConfigArgs,
) -> Result<(), error::AppError> {
    use cli::config::ConfigCommand;
    match args.command {
        ConfigCommand::Status(status) => commands::config_status::run(ctx, status),
    }
}

fn run_profile(
    ctx: &context::AppContext,
    args: cli::profile::ProfileArgs,
) -> Result<(), error::AppError> {
    use cli::profile::ProfileCommand;
    match args.command {
        ProfileCommand::List(list) => commands::profile_list::run(ctx, list),
        ProfileCommand::Show(show) => commands::profile_show::run(ctx, &show),
        ProfileCommand::Compose(compose) => commands::profile_compose::run(ctx, compose),
    }
}
