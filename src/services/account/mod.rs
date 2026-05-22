//! Account namespace: id, registry, resolver, errors.

pub(crate) mod error;
pub(crate) mod id;
pub(crate) mod registry;
pub(crate) mod resolver;

pub(crate) use error::AccountError;
pub(crate) use id::AccountId;
