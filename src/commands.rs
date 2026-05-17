//! Command handlers.
//!
//! Holds one runtime handler per wrapper verb. No clap parser definitions and
//! no reusable cross-command orchestration; shared flow lives in `services/`.
pub(crate) mod pass_through;
pub(crate) mod self_config_merge;
pub(crate) mod self_config_status;
pub(crate) mod self_help;
pub(crate) mod self_show_local;
pub(crate) mod self_version;
