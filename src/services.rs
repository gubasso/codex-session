//! Shared services.
//!
//! Holds orchestration reused across command paths. No direct terminal output or
//! low-level outside-world access belongs here; use `ui/` and `adapters/`.
pub(crate) mod merge;
