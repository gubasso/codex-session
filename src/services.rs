//! Shared services.
//!
//! What this is: orchestration reused across command paths.
//! What this is not: terminal output or low-level outside-world access; use
//! `ui/` and `adapters/`.
pub(crate) mod auth;
pub(crate) mod profile;
pub(crate) mod session;
