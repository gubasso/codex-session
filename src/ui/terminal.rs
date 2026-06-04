//! Terminal detection helpers.
//!
//! What this is: stdio terminal ownership checks that need direct access to
//! `std::io::{stdin, stdout}`.
//! What this is not: color policy or rendering.

use std::io::IsTerminal as _;

pub(crate) fn stdin_is_terminal() -> bool {
    std::io::stdin().is_terminal()
}

pub(crate) fn stdout_is_terminal() -> bool {
    std::io::stdout().is_terminal()
}
