#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

mod support;

use std::os::unix::fs::PermissionsExt as _;

use predicates::prelude::*;
use support::TestEnv;
use support::fixture_path;

#[test]
fn session_dir_persists_across_resume_invocations() {
    let env = TestEnv::new();
    let fixture = fixture_path("fake-codex-resume.sh");

    let output = env
        .cmd()
        .env("CODEX_SESSION_GROUP", "stable-test")
        .env("CODEX_SESSION_CHILD_BIN", &fixture)
        .args(["exec", "--json", "hi"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(output).unwrap();
    let line = stdout
        .lines()
        .find(|line| line.contains("\"thread_id\""))
        .unwrap();
    let value: serde_json::Value = serde_json::from_str(line).unwrap();
    let thread_id = value["thread_id"].as_str().unwrap();

    let group_dir = env.group_dir("stable-test");
    let mode = std::fs::metadata(&group_dir).unwrap().permissions().mode() & 0o777;
    assert!(group_dir.is_dir());
    assert_eq!(mode, 0o700);

    env.cmd()
        .env("CODEX_SESSION_GROUP", "stable-test")
        .env("CODEX_SESSION_CHILD_BIN", &fixture)
        .args(["exec", "resume", thread_id, "--json", "round 2"])
        .assert()
        .success()
        .stdout(predicate::str::contains(format!("resumed:{thread_id}")));
}
