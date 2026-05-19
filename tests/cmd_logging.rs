#![allow(clippy::unwrap_used)]

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
fn version_parse_error_still_writes_structured_log() {
    let env = TestEnv::new();
    let state_home = env.tmp.path().join("state");

    // `version junk` fails clap parsing; the log file must still be
    // created and must record the structured error event.
    env.cmd()
        .env("XDG_STATE_HOME", &state_home)
        .args(["version", "junk"])
        .assert()
        .code(64);

    let log_file = latest_log_file(&state_home.join("codex-session"));
    let contents = std::fs::read_to_string(&log_file).unwrap();
    assert!(
        !contents.trim().is_empty(),
        "log file must not be empty on parse error"
    );
    let last_line = contents.lines().last().unwrap();
    let json: serde_json::Value = serde_json::from_str(last_line).unwrap();
    assert_eq!(json["fields"]["err.kind"], "usage");
    assert_eq!(json["fields"]["status"], "error");
}

#[test]
fn version_parse_error_still_honors_verbose_and_log_stderr() {
    // Even on a parse failure, `-v` and `--log-stderr`
    // must apply to the error report so users get the requested observability.
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
    // The structured error record must appear on the mirrored stderr.
    assert!(
        stderr.contains("command.error"),
        "stderr mirror should carry the error record; got: {stderr}"
    );

    // And the file sink should be present and contain the same record.
    let log_file = latest_log_file(&state_home.join("codex-session"));
    let contents = std::fs::read_to_string(&log_file).unwrap();
    let last_line = contents.lines().last().unwrap();
    let json: serde_json::Value = serde_json::from_str(last_line).unwrap();
    assert_eq!(json["fields"]["err.kind"], "usage");
}

#[test]
fn version_parse_error_honors_global_flags_after_verb() {
    // `GlobalArgs` are clap `global = true`, so `-v` / `--log-stderr`
    // are valid before AND after the verb. The relaxed pre-scan must honor
    // post-verb placements too, otherwise `version -v junk` (a
    // perfectly legal flag position that still fails parse on the trailing
    // `junk`) silently drops the requested verbosity.
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
        stderr.contains("command.error"),
        "post-verb --log-stderr must still mirror the error record; got: {stderr}"
    );
}

#[test]
fn version_parse_error_honors_stacked_short_verbose_cluster() {
    // clap's `ArgAction::Count` accepts arbitrary short-cluster lengths
    // like `-vvvv`. The fallback scanner must mirror that, otherwise
    // `version -vvvv junk` silently falls back to `verbose = 0`.
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
    // Any verbose > 0 turns on the stderr mirror in main, so the error
    // record must show up on stderr.
    assert!(
        stderr.contains("command.error"),
        "-vvvv must enable the stderr mirror on the parse-failure path; got: {stderr}"
    );

    // And the file sink must contain the error record.
    let log_file = latest_log_file(&state_home.join("codex-session"));
    let contents = std::fs::read_to_string(&log_file).unwrap();
    let last_line = contents.lines().last().unwrap();
    let json: serde_json::Value = serde_json::from_str(last_line).unwrap();
    assert_eq!(json["fields"]["err.kind"], "usage");
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
fn parse_failure_with_silent_writes_nothing_to_stderr() {
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
    assert!(output.stderr.is_empty());

    let log_file = latest_log_file(&state_home.join("codex-session"));
    let contents = std::fs::read_to_string(&log_file).unwrap();
    assert!(contents.contains("\"err.kind\":\"usage\""));
}
