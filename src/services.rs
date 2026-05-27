//! Shared services.
//!
//! What this is: orchestration reused across command paths.
//! What this is not: terminal output or low-level outside-world access; use
//! `ui/` and `adapters/`.
pub(crate) mod account;
pub(crate) mod auth;
pub(crate) mod auth_inspect;
pub(crate) mod config_recipe;
pub(crate) mod session;
pub(crate) mod trust_sync;
