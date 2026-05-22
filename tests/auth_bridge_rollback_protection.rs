#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

mod support;

use support::TestEnv;

#[allow(dead_code)]
fn native_dir(env: &TestEnv) -> std::path::PathBuf {
    env.home.join(".codex")
}

#[allow(dead_code)]
fn native_auth(env: &TestEnv) -> std::path::PathBuf {
    native_dir(env).join("auth.json")
}

#[test]
#[ignore = "R4: AuthBridge watcher removed; one-shot import only in R1"]
fn auth_bridge_rollback_protection() {}

#[test]
#[ignore = "R4: AuthBridge watcher removed; one-shot import only in R1"]
fn watcher_skips_live_malformed_session_instead_of_rolling_back() {}
