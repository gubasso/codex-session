//! CLI parse-shape modules.
//!
//! Holds per-verb `clap::Args` structs only. No I/O, no business logic, no
//! dispatch; dispatch lives in `main.rs`.
pub(crate) mod self_config_merge;
pub(crate) mod self_config_status;
pub(crate) mod self_help;
pub(crate) mod self_show_local;
pub(crate) mod self_version;
