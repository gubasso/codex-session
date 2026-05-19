//! Session-directory services.
//!
//! What this is: terminal/session-root resolution and session metadata writing.
//! What this is not: child exec orchestration.

pub(crate) mod cleanup;
pub(crate) mod dir;
pub(crate) mod meta;
pub(crate) mod terminal_id;
