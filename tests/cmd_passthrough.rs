#![allow(clippy::unwrap_used)]

pub mod support;

use predicates::prelude::*;
use support::TestEnv;

#[test]
fn dash_dash_help_is_passed_through_to_codex() {
    let env = TestEnv::new();
    env.make_fake_codex();
    env.cmd().arg("--help").assert().success();
    assert_eq!(env.argc(), "1\n");
    assert_eq!(env.argv(), vec![String::from("--help")]);
}

#[test]
fn dash_dash_version_is_passed_through_to_codex() {
    let env = TestEnv::new();
    env.make_fake_codex();
    env.cmd().arg("--version").assert().success();
    assert_eq!(env.argc(), "1\n");
    assert_eq!(env.argv(), vec![String::from("--version")]);
}

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
}

#[test]
fn no_arg_invocation_calls_codex_with_zero_argv() {
    let env = TestEnv::new();
    env.make_fake_codex();
    env.cmd().assert().success();
    assert_eq!(env.argc(), "0\n");
    assert_eq!(std::fs::metadata(&env.argv_file).unwrap().len(), 0);
}

#[test]
fn missing_codex_on_path_errors_exactly() {
    let env = TestEnv::new();
    env.cmd()
        .assert()
        .code(1)
        .stdout("")
        .stderr("ERROR: codex binary not found in PATH\n");
}

#[test]
fn missing_codex_binary_emits_exact_error_and_exits_1() {
    let env = TestEnv::new();
    env.cmd_with_path("")
        .assert()
        .code(1)
        .stdout("")
        .stderr("ERROR: codex binary not found in PATH\n");
}

#[test]
fn missing_base_falls_through_to_codex_without_creating_target_or_stamp() {
    let env = TestEnv::new();
    env.make_fake_codex();
    env.cmd().arg("exec").assert().success();
    assert_eq!(env.argv(), vec![String::from("exec")]);
    assert!(!env.target_path().exists());
    assert!(!env.stamp_path().exists());
}

#[test]
fn fresh_stamp_skips_merge_and_leaves_target_mtime_unchanged() {
    let env = TestEnv::new();
    env.make_fake_codex();
    env.install_base();
    env.install_target_with_local();
    std::fs::create_dir_all(env.stamp_path().parent().unwrap()).unwrap();
    std::fs::write(env.stamp_path(), "").unwrap();
    env.touch_older(&env.base_path());
    env.touch_newer(&env.stamp_path());

    let before = std::fs::metadata(env.target_path())
        .unwrap()
        .modified()
        .unwrap();
    env.cmd().arg("exec").assert().success();
    let after = std::fs::metadata(env.target_path())
        .unwrap()
        .modified()
        .unwrap();
    assert_eq!(before, after);
}

#[test]
fn missing_stamp_triggers_merge_to_expected_output() {
    let env = TestEnv::new();
    env.make_fake_codex();
    env.install_base();
    env.install_target_with_local();
    env.cmd().arg("exec").assert().success();
    let got = std::fs::read_to_string(env.target_path()).unwrap();
    let want = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/expected-merged.toml"),
    )
    .unwrap();
    assert_eq!(got, want);
}

#[test]
fn base_newer_than_stamp_triggers_merge() {
    let env = TestEnv::new();
    env.make_fake_codex();
    env.install_base();
    env.install_target_with_local();
    std::fs::create_dir_all(env.stamp_path().parent().unwrap()).unwrap();
    std::fs::write(env.stamp_path(), "").unwrap();
    env.touch_older(&env.stamp_path());
    env.touch_newer(&env.base_path());
    env.cmd().arg("exec").assert().success();
    let got = std::fs::read_to_string(env.target_path()).unwrap();
    let want = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/expected-merged.toml"),
    )
    .unwrap();
    assert_eq!(got, want);
}

#[test]
fn missing_target_with_existing_base_writes_base_only() {
    let env = TestEnv::new();
    env.make_fake_codex();
    env.install_base();
    env.cmd().arg("exec").assert().success();
    let got = std::fs::read_to_string(env.target_path()).unwrap();
    let want = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/base.toml"),
    )
    .unwrap();
    assert_eq!(got, want);
    assert!(!got.contains("Machine-local"));
}

#[test]
fn zero_argv_resolves_codex_and_attempts_exec() {
    let env = TestEnv::new();
    env.make_fake_codex_printing_stdout("OK");
    env.cmd().assert().success().stdout("OK");
}

#[test]
fn passthrough_forwards_help_verbatim() {
    let env = TestEnv::new();
    env.make_fake_codex();
    env.cmd().arg("--help").assert().success();
    assert_eq!(env.argv(), vec![String::from("--help")]);
}

#[test]
fn passthrough_zero_argv() {
    let env = TestEnv::new();
    env.make_fake_codex();
    env.cmd().assert().success();
    assert_eq!(env.argc(), "0\n");
    assert!(env.argv().is_empty());
}

#[test]
fn path_order_first_match_wins() {
    let env = TestEnv::new();
    let first = env.make_fake_codex_in_dir("first-bin", "#!/usr/bin/env bash\nprintf 'FIRST' \n");
    let second =
        env.make_fake_codex_in_dir("second-bin", "#!/usr/bin/env bash\nprintf 'SECOND' \n");
    let path = format!("{}:{}:/usr/bin:/bin", first.display(), second.display());
    env.cmd_with_path(&path).assert().success().stdout("FIRST");
}

#[test]
fn resolve_skips_non_executable_files() {
    let env = TestEnv::new();
    let first = env.make_non_executable_codex_in_dir("non-exec-bin", "not executable");
    let second = env.make_fake_codex_in_dir("exec-bin", "#!/usr/bin/env bash\nprintf 'EXEC' \n");
    let path = format!("{}:{}:/usr/bin:/bin", first.display(), second.display());
    env.cmd_with_path(&path).assert().success().stdout("EXEC");
}

#[test]
fn merge_occurs_when_base_is_newer_than_stamp() {
    let env = TestEnv::new();
    env.make_fake_codex();
    env.install_base();
    env.install_target_with_local();
    std::fs::create_dir_all(env.stamp_path().parent().unwrap()).unwrap();
    std::fs::write(env.stamp_path(), "").unwrap();
    env.touch_older(&env.stamp_path());
    env.touch_newer(&env.base_path());

    env.cmd().arg("exec").assert().success();

    let got = std::fs::read_to_string(env.target_path()).unwrap();
    let want = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/expected-merged.toml"),
    )
    .unwrap();
    assert_eq!(got, want);
}

#[test]
fn merge_is_skipped_when_stamp_is_fresh() {
    let env = TestEnv::new();
    env.make_fake_codex();
    env.install_base();
    env.install_target_with_local();
    std::fs::create_dir_all(env.stamp_path().parent().unwrap()).unwrap();
    std::fs::write(env.stamp_path(), "").unwrap();
    env.touch_older(&env.base_path());
    env.touch_newer(&env.stamp_path());

    let before = std::fs::metadata(env.target_path())
        .unwrap()
        .modified()
        .unwrap();
    env.cmd().arg("exec").assert().success();
    let after = std::fs::metadata(env.target_path())
        .unwrap()
        .modified()
        .unwrap();
    assert_eq!(before, after);
}

#[test]
fn missing_home_does_not_panic() {
    let env = TestEnv::new();
    env.make_fake_codex_printing_stdout("OK");
    env.cmd_without_home()
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
        .assert()
        .success()
        .stdout("OK");
}
