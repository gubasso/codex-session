//! Top-level argv dispatch.
#![allow(clippy::missing_errors_doc)]

/// Parsed dispatch target.
pub enum Dispatch {
    /// `self` subtree.
    SelfCmd(Vec<std::ffi::OsString>),
    /// Pass-through to the real `codex`.
    PassThrough(Vec<std::ffi::OsString>),
}

/// Classify the argv path.
#[must_use]
pub fn classify_argv(argv: &[std::ffi::OsString]) -> Dispatch {
    if argv.first().is_some_and(|arg| arg == "self") {
        Dispatch::SelfCmd(argv[1..].to_vec())
    } else {
        Dispatch::PassThrough(argv.to_vec())
    }
}

/// Dispatch the top-level argv vector.
pub fn run(
    ctx: &crate::context::AppContext,
    argv: &[std::ffi::OsString],
) -> Result<(), crate::error::AppError> {
    match classify_argv(argv) {
        Dispatch::SelfCmd(rest) => dispatch_self(ctx, &rest),
        Dispatch::PassThrough(rest) => crate::commands::pass_through::run(ctx, &rest),
    }
}

fn dispatch_self(
    ctx: &crate::context::AppContext,
    rest: &[std::ffi::OsString],
) -> Result<(), crate::error::AppError> {
    let verb = rest
        .first()
        .cloned()
        .unwrap_or_else(|| std::ffi::OsString::from("help"))
        .to_string_lossy()
        .into_owned();
    match verb.as_str() {
        "help" => crate::commands::self_help::run(ctx),
        "version" => crate::commands::self_version::run(ctx),
        "config-status" => crate::commands::self_config_status::run(ctx),
        "config-merge" => crate::commands::self_config_merge::run(ctx),
        "show-local" => crate::commands::self_show_local::run(ctx),
        other => Err(crate::error::AppError::UnknownSelfVerb(other.to_owned())),
    }
}
