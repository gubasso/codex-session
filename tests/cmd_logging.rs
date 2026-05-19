#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

pub mod support;

use std::path::{Path, PathBuf};
use support::TestEnv;

fn latest_log_file(dir: &Path) -> PathBuf {
    let mut entries = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("codex-session.log"))
        })
        .collect::<Vec<_>>();
    entries.sort();
    assert!(!entries.is_empty(), "expected a rotated log file");
    entries
        .pop()
        .unwrap_or_else(|| unreachable!("entries was checked to be non-empty"))
}

#[test]
fn error_path_writes_structured_json_log_file() {
    let env = TestEnv::new();
    let state_home = env.tmp.path().join("state");

    env.cmd()
        .env("XDG_STATE_HOME", &state_home)
        .arg("exec")
        .assert()
        .code(127);

    let log_file = latest_log_file(&state_home.join("codex-session"));
    let contents = std::fs::read_to_string(&log_file).unwrap();
    assert!(!contents.trim().is_empty(), "log file must not be empty");
    let last_line = contents.lines().last().unwrap();
    let json: serde_json::Value = serde_json::from_str(last_line).unwrap();
    assert_eq!(json["fields"]["err.kind"], "child-not-found");
    assert_eq!(json["fields"]["status"], "error");
}

#[test]
fn version_parse_error_does_not_boot_logging_before_config_load() {
    let env = TestEnv::new();
    let state_home = env.tmp.path().join("state");

    // Phase 08 routes clap parse failures directly through `AppError::Usage`
    // without re-loading config or installing tracing. Parse failures should
    // still exit 64, but they must not create the log directory.
    let output = env
        .cmd()
        .env("XDG_STATE_HOME", &state_home)
        .args(["version", "junk"])
        .assert()
        .code(64)
        .get_output()
        .clone();
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("unexpected argument 'junk' found"));
    assert!(!state_home.join("codex-session").exists());
}

#[test]
fn version_parse_error_uses_plain_clap_stderr_even_with_verbose_flags() {
    let env = TestEnv::new();
    let state_home = env.tmp.path().join("state");

    let output = env
        .cmd()
        .env("XDG_STATE_HOME", &state_home)
        .args(["-v", "--log-stderr", "version", "junk"])
        .assert()
        .code(64)
        .get_output()
        .clone();
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("unexpected argument 'junk' found"),
        "clap parse stderr must still be shown; got: {stderr}"
    );
    assert!(
        !stderr.contains("command.error"),
        "parse failures must not install the structured stderr mirror: {stderr}"
    );
    assert!(!state_home.join("codex-session").exists());
}

#[test]
fn version_parse_error_after_verb_still_skips_logging_setup() {
    let env = TestEnv::new();
    let state_home = env.tmp.path().join("state");

    let output = env
        .cmd()
        .env("XDG_STATE_HOME", &state_home)
        .args(["version", "--log-stderr", "-v", "junk"])
        .assert()
        .code(64)
        .get_output()
        .clone();
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("unexpected argument 'junk' found"),
        "parse error must still render via clap; got: {stderr}"
    );
    assert!(!state_home.join("codex-session").exists());
}

#[test]
fn version_parse_error_with_stacked_verbose_cluster_skips_logging_setup() {
    let env = TestEnv::new();
    let state_home = env.tmp.path().join("state");

    let output = env
        .cmd()
        .env("XDG_STATE_HOME", &state_home)
        .args(["version", "-vvvv", "junk"])
        .assert()
        .code(64)
        .get_output()
        .clone();
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("unexpected argument 'junk' found"),
        "parse error must still render via clap; got: {stderr}"
    );
    assert!(!state_home.join("codex-session").exists());
}

#[test]
fn top_level_verbose_mirrors_logs_to_stderr_and_file() {
    let env = TestEnv::new();
    let state_home = env.tmp.path().join("state");

    let output = env
        .cmd()
        .env("XDG_STATE_HOME", &state_home)
        .args(["-v", "help"])
        .assert()
        .success()
        .get_output()
        .clone();
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("help"));

    let log_file = latest_log_file(&state_home.join("codex-session"));
    let contents = std::fs::read_to_string(&log_file).unwrap();
    assert!(contents.contains("\"op\":\"help\""));
}

#[test]
fn quiet_suppresses_stderr_but_keeps_file_sink() {
    let env = TestEnv::new();
    let state_home = env.tmp.path().join("state");

    let output = env
        .cmd()
        .env("XDG_STATE_HOME", &state_home)
        .args(["-q", "-vv", "help"])
        .assert()
        .success()
        .get_output()
        .clone();
    assert!(
        output.stderr.is_empty(),
        "quiet must suppress the stderr mirror"
    );

    let log_file = latest_log_file(&state_home.join("codex-session"));
    let contents = std::fs::read_to_string(&log_file).unwrap();
    assert!(contents.contains("\"op\":\"help\""));
}

#[test]
fn silent_suppresses_stderr_on_error_but_preserves_exit_code() {
    let env = TestEnv::new();
    let state_home = env.tmp.path().join("state");

    let output = env
        .cmd()
        .env("XDG_STATE_HOME", &state_home)
        .args(["--silent", "exec"])
        .assert()
        .code(127)
        .get_output()
        .clone();
    assert!(output.stderr.is_empty(), "silent must suppress stderr");

    let log_file = latest_log_file(&state_home.join("codex-session"));
    let contents = std::fs::read_to_string(&log_file).unwrap();
    assert!(contents.contains("\"err.kind\":\"child-not-found\""));
}

#[test]
fn log_format_json_writes_json_to_stderr() {
    let env = TestEnv::new();
    let state_home = env.tmp.path().join("state");

    let output = env
        .cmd()
        .env("XDG_STATE_HOME", &state_home)
        .args(["-v", "--log-format", "json", "help"])
        .assert()
        .success()
        .get_output()
        .clone();
    let stderr = String::from_utf8(output.stderr).unwrap();
    let last_line = stderr.lines().last().unwrap();
    let json: serde_json::Value = serde_json::from_str(last_line).unwrap();
    assert_eq!(json["fields"]["op"], "help");
}

#[test]
fn log_stderr_alone_mirrors_warnings() {
    let env = TestEnv::new();
    let state_home = env.tmp.path().join("state");

    let output = env
        .cmd()
        .env("XDG_STATE_HOME", &state_home)
        .args(["--log-stderr", "exec"])
        .assert()
        .code(127)
        .get_output()
        .clone();
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("command.error"));
}

#[test]
fn parse_failure_does_not_honor_silent_before_config_load() {
    let env = TestEnv::new();
    let state_home = env.tmp.path().join("state");

    let output = env
        .cmd()
        .env("XDG_STATE_HOME", &state_home)
        .args(["--silent", "version", "junk"])
        .assert()
        .code(64)
        .get_output()
        .clone();
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("unexpected argument 'junk' found"));
    assert!(!state_home.join("codex-session").exists());
}
