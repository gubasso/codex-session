//! `config-recipe` subcommand: parse-shape.
//!
//! What this is: clap parser types for config-recipe inspection and composition.
//! What this is not: manifest parsing or session creation.

/// `config-recipe` command group.
#[derive(Debug, clap::Args)]
pub(crate) struct ConfigRecipeArgs {
    /// Concrete config-recipe operation to perform.
    #[command(subcommand)]
    pub(crate) command: ConfigRecipeCommand,
}

/// `config-recipe` subcommands.
#[derive(Debug, clap::Subcommand)]
pub(crate) enum ConfigRecipeCommand {
    /// List available config recipes.
    List(ConfigRecipeListArgs),
    /// Show a config-recipe manifest and resolved layers.
    Show(ConfigRecipeShowArgs),
    /// Compose a config-recipe into the current session directory.
    Compose(ConfigRecipeComposeArgs),
}

#[derive(Debug, Clone, Copy, clap::Args)]
pub(crate) struct ConfigRecipeListArgs {
    /// Output format for the reported config-recipe list.
    #[arg(long, value_enum, default_value_t = crate::cli::OutputFormat::Text)]
    pub(crate) format: crate::cli::OutputFormat,
}

#[derive(Debug, Clone, clap::Args)]
pub(crate) struct ConfigRecipeShowArgs {
    /// Config-recipe name. Defaults to the currently active wrapper config-recipe.
    pub(crate) name: Option<String>,
    /// Output format for the reported config-recipe details.
    #[arg(long, value_enum, default_value_t = crate::cli::OutputFormat::Text)]
    pub(crate) format: crate::cli::OutputFormat,
}

#[derive(Debug, Clone, clap::Args)]
pub(crate) struct ConfigRecipeComposeArgs {
    /// Config-recipe name. Defaults to the currently active wrapper config-recipe.
    pub(crate) name: Option<String>,
}
