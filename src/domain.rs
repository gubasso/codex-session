//! Domain types.
//!
//! Holds pure value logic and invariants only. No direct filesystem, process,
//! or terminal I/O belongs in this module tree.
pub(crate) mod child_invocation;
pub(crate) mod config_merge;
pub(crate) mod version;
