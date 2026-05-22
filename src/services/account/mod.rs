//! Account namespace: id, registry, resolver, errors.

pub(crate) mod error;
pub(crate) mod id;
pub(crate) mod quota;
pub(crate) mod registry;
pub(crate) mod resolver;
pub(crate) mod selector;

pub(crate) use error::AccountError;
pub(crate) use id::AccountId;
