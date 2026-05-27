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
        Some(cli::Commands::ConfigRecipe(args)) => run_config_recipe(ctx, args).map(|()| 0),
        Some(cli::Commands::Doctor(args)) => commands::doctor::run(ctx, args),
        Some(cli::Commands::Account(args)) => commands::account::dispatch(ctx, args).map(|()| 0),
        Some(cli::Commands::External(argv)) => {
            commands::pass_through::run(ctx, &argv).map(child_exit_code)
        }
        // No subcommand: forward to `codex` with an empty child argv
        // (launches the Codex TUI when `codex` is resolvable).
        None => commands::pass_through::run(ctx, &[]).map(child_exit_code),
    }
}

fn child_exit_code(code: i32) -> u8 {
    u8::try_from(code).unwrap_or_else(|_| {
        tracing::warn!(
            op = "child.exit.clamped",
            original = code,
            clamped = u8::MAX,
            "child exit code does not fit in u8; clamping to 255",
        );
        u8::MAX
    })
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

fn run_config_recipe(
    ctx: &context::AppContext,
    args: cli::config_recipe::ConfigRecipeArgs,
) -> Result<(), error::AppError> {
    use cli::config_recipe::ConfigRecipeCommand;
    match args.command {
        ConfigRecipeCommand::List(list) => commands::config_recipe_list::run(ctx, list),
        ConfigRecipeCommand::Show(show) => commands::config_recipe_show::run(ctx, &show),
        ConfigRecipeCommand::Compose(compose) => commands::config_recipe_compose::run(ctx, compose),
    }
}
