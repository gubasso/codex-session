//! Domain types.
//!
//! Holds pure value logic and invariants only. No direct filesystem, process,
//! or terminal I/O belongs in this module tree.
pub(crate) mod config_merge;
pub(crate) mod paths;
pub(crate) mod version;
