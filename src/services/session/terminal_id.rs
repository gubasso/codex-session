//! Terminal identifier derivation.
//!
//! What this is: a best-effort stable terminal id for session scoping.
//! What this is not: session-root creation or metadata writing.

/// Derive the current terminal identifier, falling back to the pid.
pub(crate) fn current() -> String {
    current_from_tty().unwrap_or_else(|| format!("pid-{}", std::process::id()))
}

fn current_from_tty() -> Option<String> {
    let output = std::process::Command::new("tty").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let tty = String::from_utf8(output.stdout).ok()?;
    let tty = tty.trim();
    let stripped = tty.strip_prefix("/dev/")?;
    Some(stripped.replace('/', "-"))
}
