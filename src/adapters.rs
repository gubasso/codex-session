//! External-system adapters.
//!
//! Holds the only code that talks to the outside world directly. No domain
//! policy or command dispatch belongs in this module tree.
pub(crate) mod fs;
pub(crate) mod process;
