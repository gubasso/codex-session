//! Version helpers.
//!
//! Source of truth is the compile-time embedded `VERSION` file. The legacy
//! bash launcher resolved `VERSION` at runtime relative to a symlink target;
//! that mechanism existed to support a stage/relink install model. The Rust
//! crate is distributed via `cargo install`, so the embedded constant is
//! sufficient and the unit test below pins it to `CARGO_PKG_VERSION`.
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
