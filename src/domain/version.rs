//! Version helpers.
//!
//! What this is: access to the wrapper version embedded at compile time from
//! the checked-in `VERSION` file.
//! What this is not: runtime install-path probing; the Rust binary is shipped
//! via `cargo install`, so the embedded constant is sufficient.
#![allow(clippy::must_use_candidate)]

/// Return the wrapper version sourced from the checked-in `VERSION` file.
pub(crate) fn current() -> &'static str {
    include_str!("../../VERSION").trim_end_matches('\n')
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    #[test]
    fn version_file_matches_cargo_pkg() {
        assert_eq!(super::current(), env!("CARGO_PKG_VERSION"));
    }
}
