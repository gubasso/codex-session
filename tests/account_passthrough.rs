#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

mod support;

use support::{TestEnv, fixture_path};

#[test]
fn passthrough_routes_to_named_account_path() {
    let env = TestEnv::new();
    env.seed_account("work", "{\"token\":\"test\"}\n");
    let output = env
        .cmd()
        .env("CODEX_SESSION_CHILD_BIN", fixture_path("echo-env.sh"))
        .args(["--account", "work", "--group", "stable", "exec"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(output).unwrap();
    assert!(stdout.contains(&format!(
        "CODEX_HOME={}",
        env.named_group_dir("work", "stable").display()
    )));
}
