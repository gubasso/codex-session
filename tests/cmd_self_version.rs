#![allow(clippy::unwrap_used)]

pub mod support;

use support::TestEnv;

#[test]
fn self_version_matches_cargo_pkg_version() {
    TestEnv::new()
        .cmd()
        .args(["self", "version"])
        .assert()
        .success()
        .stdout(format!("codex-session {}\n", env!("CARGO_PKG_VERSION")));
}
