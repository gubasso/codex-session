//! External-system adapters.
//!
//! What this is: the only code that talks to the outside world directly.
//! What this is not: domain policy or command dispatch.
pub(crate) mod fs;
pub(crate) mod spawner;
