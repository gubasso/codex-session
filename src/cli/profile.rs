//! `profile` subcommand: parse-shape.
//!
//! What this is: clap parser types for profile inspection and composition.
//! What this is not: manifest parsing or session creation.

/// `profile` command group.
#[derive(Debug, clap::Args)]
pub(crate) struct ProfileArgs {
    /// Concrete profile operation to perform.
    #[command(subcommand)]
    pub(crate) command: ProfileCommand,
}

/// `profile` subcommands.
#[derive(Debug, clap::Subcommand)]
pub(crate) enum ProfileCommand {
    /// List available profiles.
    List(ProfileListArgs),
    /// Show a profile manifest and resolved layers.
    Show(ProfileShowArgs),
    /// Compose a profile into the current session directory.
    Compose(ProfileComposeArgs),
}

#[derive(Debug, Clone, Copy, clap::Args)]
pub(crate) struct ProfileListArgs {
    /// Output format for the reported profile list.
    #[arg(long, value_enum, default_value_t = crate::cli::OutputFormat::Text)]
    pub(crate) format: crate::cli::OutputFormat,
}

#[derive(Debug, Clone, clap::Args)]
pub(crate) struct ProfileShowArgs {
    /// Profile name. Defaults to the currently active wrapper profile.
    pub(crate) name: Option<String>,
    /// Output format for the reported profile details.
    #[arg(long, value_enum, default_value_t = crate::cli::OutputFormat::Text)]
    pub(crate) format: crate::cli::OutputFormat,
}

#[derive(Debug, Clone, clap::Args)]
pub(crate) struct ProfileComposeArgs {
    /// Profile name. Defaults to the currently active wrapper profile.
    pub(crate) name: Option<String>,
}
