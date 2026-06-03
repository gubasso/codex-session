//! Account namespace: id, registry, resolver, errors.

pub(crate) mod codex_events;
pub(crate) mod cooldown;
pub(crate) mod error;
pub(crate) mod failover;
pub(crate) mod gate;
pub(crate) mod id;
pub(crate) mod online_probe;
pub(crate) mod quota;
pub(crate) mod registry;
pub(crate) mod resolver;
pub(crate) mod retry;
pub(crate) mod selector;
pub(crate) mod token_expiry;
pub(crate) mod token_refresh;

pub(crate) use error::AccountError;
pub(crate) use id::AccountId;
