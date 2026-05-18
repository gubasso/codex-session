//! Command handlers.
//!
//! Holds one runtime handler per wrapper verb. No clap parser definitions and
//! no reusable cross-command orchestration; shared flow lives in `services/`.
pub(crate) mod config_merge;
pub(crate) mod config_show_local;
pub(crate) mod config_status;
pub(crate) mod dispatch;
pub(crate) mod help;
pub(crate) mod pass_through;
pub(crate) mod version;
