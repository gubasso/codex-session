#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

pub mod support;

use predicates::prelude::*;
use support::TestEnv;

#[test]
fn exec_foo_bar_is_passed_through_to_codex() {
    let env = TestEnv::new();
    env.make_fake_codex();
    env.cmd().args(["exec", "foo", "bar"]).assert().success();
    assert_eq!(env.argc(), "3\n");
    assert_eq!(
        env.argv(),
        vec![
            String::from("exec"),
            String::from("foo"),
            String::from("bar")
        ]
    );
    assert!(env.session_dir().join("config.toml").exists());
    assert!(env.session_dir().join("session-meta.json").exists());
    let meta: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(env.session_dir().join("session-meta.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(meta["account"], "default");
    assert_eq!(meta["account-source"], "lru");
}

#[test]
fn stock_mode_writes_stable_empty_compose_sidecar() {
    // Stock mode (no active profile, no manifest) must still produce a
    // sidecar with the canonical shape so downstream consumers can read one
    // schema in both stock and composed modes.
    let env = TestEnv::new();
    env.make_fake_codex();
    env.cmd().args(["exec", "noop"]).assert().success();

    let sidecar_path = env.session_dir().join(".codex-session-compose.json");
    let sidecar: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&sidecar_path).unwrap()).unwrap();
    assert!(sidecar.is_object(), "sidecar must be a JSON object");
    assert!(
        sidecar
            .get("manifest")
            .is_some_and(serde_json::Value::is_null)
    );
    assert_eq!(sidecar["layers"], serde_json::json!([]));
    assert_eq!(sidecar["env"], serde_json::json!({}));
}

#[test]
fn wrapper_profile_flag_is_not_forwarded_to_child() {
    let env = TestEnv::new();
    env.make_fake_codex();
    env.install_profile("work", "settings-layers:\n  - base\n", &[("base", "")]);
    env.cmd()
        .args(["--profile", "work", "exec", "foo"])
        .assert()
        .success();
    assert_eq!(env.argv(), vec!["exec".to_string(), "foo".to_string()]);
}

#[test]
fn resume_is_passed_through_to_codex() {
    let env = TestEnv::new();
    env.make_fake_codex();
    env.cmd().arg("resume").assert().success();
    assert_eq!(env.argv(), vec![String::from("resume")]);
}

#[test]
fn unknown_future_verb_is_passed_through_to_codex() {
    let env = TestEnv::new();
    env.make_fake_codex();
    env.cmd().args(["future-verb", "--flag"]).assert().success();
    assert_eq!(
        env.argv(),
        vec![String::from("future-verb"), String::from("--flag")]
    );
}

#[test]
fn missing_codex_on_path_errors_exactly() {
    let env = TestEnv::new();
    env.cmd()
        .arg("exec")
        .assert()
        .code(127)
        .stdout("")
        .stderr(predicate::str::contains(
            "codex-session: failed to resolve wrapped codex binary",
        ))
        .stderr(predicate::str::contains(
            "hint:  set CODEX_SESSION_CHILD_BIN or add codex to PATH and retry",
        ));
}

#[test]
fn missing_codex_binary_uses_shell_not_found_exit_code() {
    let env = TestEnv::new();
    env.cmd_with_path("")
        .arg("exec")
        .assert()
        .code(127)
        .stdout("")
        .stderr(predicate::str::contains(
            "codex-session: failed to resolve wrapped codex binary",
        ));
}

#[test]
fn zero_argv_resolves_codex_and_attempts_exec() {
    let env = TestEnv::new();
    env.make_fake_codex_printing_stdout("OK");
    env.cmd().arg("exec").assert().success().stdout("OK");
}

#[test]
fn path_order_first_match_wins() {
    let env = TestEnv::new();
    let first = env.make_fake_codex_in_dir("first-bin", "#!/usr/bin/env bash\nprintf 'FIRST' \n");
    let second =
        env.make_fake_codex_in_dir("second-bin", "#!/usr/bin/env bash\nprintf 'SECOND' \n");
    let path = format!("{}:{}:/usr/bin:/bin", first.display(), second.display());
    env.cmd_with_path(&path)
        .arg("exec")
        .assert()
        .success()
        .stdout("FIRST");
}

#[test]
fn resolve_skips_non_executable_files() {
    let env = TestEnv::new();
    let first = env.make_non_executable_codex_in_dir("non-exec-bin", "not executable");
    let second = env.make_fake_codex_in_dir("exec-bin", "#!/usr/bin/env bash\nprintf 'EXEC' \n");
    let path = format!("{}:{}:/usr/bin:/bin", first.display(), second.display());
    env.cmd_with_path(&path)
        .arg("exec")
        .assert()
        .success()
        .stdout("EXEC");
}

#[test]
fn missing_home_does_not_panic() {
    let env = TestEnv::new();
    env.make_fake_codex_printing_stdout("OK");
    env.cmd_without_home()
        .arg("exec")
        .assert()
        .success()
        .stdout(predicate::str::contains("OK"));
}

#[test]
fn rust_log_does_not_pollute_stdout() {
    let env = TestEnv::new();
    env.make_fake_codex_printing_stdout("OK");
    env.cmd()
        .env("RUST_LOG", "trace")
        .arg("exec")
        .assert()
        .success()
        .stdout("OK");
}
