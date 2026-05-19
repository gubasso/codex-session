//! Color policy.
//!
//! What this is: the single source of truth for whether ANSI color is enabled
//! on stdout/stderr.
//! What this is not: a styling library.

use std::ffi::OsString;
use std::io::IsTerminal as _;

#[derive(Debug, Clone, Copy)]
pub(crate) enum Stream {
    Stdout,
    Stderr,
}

impl Stream {
    fn is_tty(self) -> bool {
        match self {
            Self::Stdout => std::io::stdout().is_terminal(),
            Self::Stderr => std::io::stderr().is_terminal(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct EnvSnapshot {
    no_color: Option<OsString>,
    force_color: Option<OsString>,
    clicolor_force: Option<OsString>,
    clicolor: Option<OsString>,
}

fn current_env() -> EnvSnapshot {
    EnvSnapshot {
        no_color: std::env::var_os("NO_COLOR"),
        force_color: std::env::var_os("FORCE_COLOR"),
        clicolor_force: std::env::var_os("CLICOLOR_FORCE"),
        clicolor: std::env::var_os("CLICOLOR"),
    }
}

fn is_nonempty(value: Option<&OsString>) -> bool {
    value.is_some_and(|v| !v.is_empty())
}

fn is_truthy_force(value: Option<&OsString>) -> bool {
    value
        .and_then(|v| v.to_str())
        .is_some_and(|v| !v.is_empty() && v != "0")
}

fn decide_color(env: &EnvSnapshot, is_tty: bool) -> bool {
    if is_nonempty(env.no_color.as_ref()) {
        return false;
    }
    if is_truthy_force(env.force_color.as_ref()) || is_truthy_force(env.clicolor_force.as_ref()) {
        return true;
    }
    if !is_tty {
        return false;
    }
    if env.clicolor.as_deref() == Some(std::ffi::OsStr::new("0")) {
        return false;
    }
    true
}

pub(crate) fn should_color(stream: Stream) -> bool {
    decide_color(&current_env(), stream.is_tty())
}

pub(crate) fn stderr_color() -> bool {
    should_color(Stream::Stderr)
}

pub(crate) fn stdout_color() -> bool {
    should_color(Stream::Stdout)
}

#[cfg(test)]
mod tests {
    use super::{EnvSnapshot, decide_color};
    use std::ffi::OsString;

    fn env(
        no_color: Option<&str>,
        force_color: Option<&str>,
        clicolor_force: Option<&str>,
        clicolor: Option<&str>,
    ) -> EnvSnapshot {
        EnvSnapshot {
            no_color: no_color.map(OsString::from),
            force_color: force_color.map(OsString::from),
            clicolor_force: clicolor_force.map(OsString::from),
            clicolor: clicolor.map(OsString::from),
        }
    }

    #[test]
    fn no_color_wins_absolutely() {
        let env = env(Some("1"), Some("1"), Some("1"), Some("1"));
        assert!(!decide_color(&env, true));
        assert!(!decide_color(&env, false));
    }

    #[test]
    fn force_color_overrides_non_tty() {
        let env = env(None, Some("1"), None, None);
        assert!(decide_color(&env, false));
    }

    #[test]
    fn clicolor_force_overrides_non_tty() {
        let env = env(None, None, Some("1"), None);
        assert!(decide_color(&env, false));
    }

    #[test]
    fn no_tty_without_force_disables_color() {
        let env = env(None, None, None, Some("1"));
        assert!(!decide_color(&env, false));
    }

    #[test]
    fn clicolor_zero_disables_color_on_tty() {
        let env = env(None, None, None, Some("0"));
        assert!(!decide_color(&env, true));
    }

    #[test]
    fn tty_defaults_to_color() {
        let env = env(None, Some("0"), Some("0"), None);
        assert!(decide_color(&env, true));
    }
}
