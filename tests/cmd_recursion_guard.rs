#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

mod support;

use std::os::unix::fs::symlink;

use support::TestEnv;

#[test]
fn child_bin_pointing_at_wrapper_exits_70() {
    let env = TestEnv::new();
    let wrapper = assert_cmd::cargo::cargo_bin("codex-session");
    let symlinked = env.tmp.path().join("codex");
    symlink(&wrapper, &symlinked).unwrap();

    let assert = env
        .cmd()
        .env("CODEX_SESSION_CHILD_BIN", &symlinked)
        .args(["exec"])
        .assert()
        .failure()
        .code(70);
    let stderr = String::from_utf8(assert.get_output().stderr.clone()).unwrap();
    assert!(
        stderr.contains("resolves to the wrapper itself"),
        "missing recursion message: {stderr}"
    );
}

#[test]
fn reentry_env_exits_70_on_exec_path() {
    let env = TestEnv::new();
    env.make_fake_codex();
    env.cmd()
        .env("CODEX_SESSION_REENTRY", "1")
        .args(["exec"])
        .assert()
        .failure()
        .code(70);
}

#[test]
fn private_env_is_scrubbed_before_exec() {
    let env = TestEnv::new();
    let stub = support::fixture_path("echo-env.sh");
    let log_file_hint = env.tmp.path().join("x-log-dir");
    let log_dir_hint = env.tmp.path().join("xdir");
    let output = env
        .cmd()
        .env("CODEX_SESSION_CHILD_BIN", &stub)
        .env("CODEX_SESSION_LOG_FILE", &log_file_hint)
        .env("CODEX_SESSION_LOG_DIR", &log_dir_hint)
        .args(["exec"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(output).unwrap();

    assert!(
        stdout.contains("CODEX_SESSION_CHILD_BIN=\n"),
        "child still saw CHILD_BIN: {stdout}"
    );
    assert!(
        stdout.contains("CODEX_SESSION_LOG_FILE=\n"),
        "child still saw LOG_FILE: {stdout}"
    );
    assert!(
        stdout.contains("CODEX_SESSION_LOG_DIR=\n"),
        "child still saw LOG_DIR: {stdout}"
    );
    assert!(
        stdout.contains("CODEX_SESSION_REENTRY=1\n"),
        "missing REENTRY=1: {stdout}"
    );
}

/// The wrapper consumes a broader `CODEX_SESSION_*` namespace than the
/// four baseline keys (see `src/config/mod.rs::apply_env_layer`:
/// `LOG_VERBOSE`, `LOG_MIRROR_STDERR`, `LOG_FORMAT`, `PATHS_*`). Per
/// cli-design `06-cli-wrapper-design/process-and-posix.md`, the wrapper
/// must scrub its own env namespace before exec. This test locks in
/// that the *whole* `CODEX_SESSION_*` prefix is stripped on the exec
/// path — not just the hardcoded baseline.
#[test]
fn full_codex_session_namespace_is_scrubbed_before_exec() {
    let env = TestEnv::new();
    let stub = support::fixture_path("echo-env.sh");
    let output = env
        .cmd()
        .env("CODEX_SESSION_CHILD_BIN", &stub)
        .env("CODEX_SESSION_LOG_VERBOSE", "2")
        .env("CODEX_SESSION_PATHS_CACHE_DIR", "/tmp/never-leak")
        .args(["exec"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(output).unwrap();

    assert!(
        !stdout.contains("LEAKED:CODEX_SESSION_LOG_VERBOSE"),
        "LOG_VERBOSE leaked into child: {stdout}"
    );
    assert!(
        !stdout.contains("LEAKED:CODEX_SESSION_PATHS_CACHE_DIR"),
        "PATHS_CACHE_DIR leaked into child: {stdout}"
    );
}

/// On Unix, environment names may contain arbitrary bytes. A hostile
/// caller could inject a `CODEX_SESSION_*` key whose suffix is not
/// valid UTF-8, and an implementation that filters via `to_str()` would
/// silently miss it. This test passes a `CODEX_SESSION_<non-utf8>` key
/// from Rust (where the env API takes `OsStr`/bytes, unlike bash) and
/// asserts the wrapper scrubs it before exec. Bytewise prefix-matching
/// in `ChildEnv::scrubbed_default` is the safety net.
#[cfg(unix)]
#[test]
fn non_utf8_codex_session_key_is_scrubbed_before_exec() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt as _;

    let env = TestEnv::new();
    let stub = support::fixture_path("echo-env.sh");

    // `CODEX_SESSION_` (14 ASCII bytes) + lone 0xFF byte (invalid UTF-8).
    let mut bytes = b"CODEX_SESSION_".to_vec();
    bytes.push(0xFF);
    let hostile_key = OsString::from_vec(bytes);

    let output = env
        .cmd()
        .env("CODEX_SESSION_CHILD_BIN", &stub)
        .env(&hostile_key, "leak-me-if-you-can")
        .args(["exec"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    // The fixture emits `LEAKED:<KEY>=<VALUE>` lines for any non-baseline
    // `CODEX_SESSION_*` key it sees; we don't need the key to be UTF-8
    // to look for the leak value in stdout.
    let stdout_lossy = String::from_utf8_lossy(&output);
    assert!(
        !stdout_lossy.contains("leak-me-if-you-can"),
        "non-UTF-8 CODEX_SESSION_* key leaked into child: {stdout_lossy}"
    );
}
