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
        // The bare-invocation help path (no subcommand, no `--version`)
        // is handled in `main` before config/logging init so it stays
        // functionally equivalent to `--help` / `help` even when the
        // log directory is unwritable. Reaching `None` here would mean
        // `main` failed to short-circuit, which is a wrapper invariant
        // violation.
        None => unreachable!("bare invocation must be short-circuited in `main` before dispatch"),
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
