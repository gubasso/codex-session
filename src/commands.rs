//! Command handlers.
//!
//! What this is: one runtime handler module per wrapper-owned verb.
//! What this is not: clap parser definitions or shared orchestration;
//! those live in `cli/` and `services/`.
pub(crate) mod completion;
pub(crate) mod config_merge;
pub(crate) mod config_show_local;
pub(crate) mod config_status;
pub(crate) mod dispatch;
pub(crate) mod help;
pub(crate) mod pass_through;
pub(crate) mod version;
