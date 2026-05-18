//! Verb dispatch.
//!
//! What this is: routes a parsed `Cli` to the matching command handler.
//! What this is not: business logic — every arm is a thin call to a
//! per-verb handler.

use crate::{cli, commands, context, error};

/// Route a parsed root `Cli` to its handler.
pub(crate) fn run(ctx: &context::AppContext, cli: cli::Cli) -> Result<(), error::AppError> {
    if cli.global.version {
        return commands::version::run(ctx, cli::version::VersionArgs::default());
    }
    match cli.command {
        Some(cli::Commands::Version(args)) => commands::version::run(ctx, args),
        Some(cli::Commands::Help) | None => commands::help::run(ctx),
        Some(cli::Commands::Config(args)) => run_config(ctx, args),
        Some(cli::Commands::External(argv)) => commands::pass_through::run(ctx, &argv),
    }
}

fn run_config(
    ctx: &context::AppContext,
    args: cli::config::ConfigArgs,
) -> Result<(), error::AppError> {
    use cli::config::ConfigCommand;
    match args.command {
        ConfigCommand::Status(status) => commands::config_status::run(ctx, status),
        ConfigCommand::Merge(merge) => commands::config_merge::run(ctx, merge),
        ConfigCommand::ShowLocal(show_local) => commands::config_show_local::run(ctx, show_local),
    }
}
