#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

use std::process::{Command, Stdio};
use std::time::Duration;

#[test]
fn sigint_mid_stream_preserves_emitted_stderr() {
    let td = tempfile::tempdir().unwrap();
    let home = td.path().join("home");
    let xdg_cache = td.path().join("cache");
    let xdg_config = td.path().join("config");
    let xdg_state = td.path().join("state");
    let xdg_runtime = td.path().join("runtime");
    for dir in [&home, &xdg_cache, &xdg_config, &xdg_state, &xdg_runtime] {
        std::fs::create_dir_all(dir).unwrap();
    }

    let bin = assert_cmd::cargo::cargo_bin("codex-session");
    let fixture =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/slow-streamer.sh");
    assert!(fixture.is_file());

    let child = Command::new(&bin)
        .arg("exec")
        .env_clear()
        .env("HOME", &home)
        .env("XDG_CACHE_HOME", &xdg_cache)
        .env("XDG_CONFIG_HOME", &xdg_config)
        .env("XDG_STATE_HOME", &xdg_state)
        .env("XDG_RUNTIME_DIR", &xdg_runtime)
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("CODEX_SESSION_CHILD_BIN", &fixture)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    // 250ms gives the wrapper enough time to spawn the bash child and let
    // the tee forward the first few lines; the SIGINT then exercises the
    // mid-stream interrupt path the regression test exists to lock in.
    std::thread::sleep(Duration::from_millis(250));
    Command::new("kill")
        .arg(format!("-{}", libc::SIGINT))
        .arg(child.id().to_string())
        .status()
        .unwrap();

    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(130));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.lines().any(|l| l.contains("line-1")),
        "stderr did not contain line-1; tee likely buffered: {stderr}"
    );
}
