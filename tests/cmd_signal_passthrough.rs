#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

use std::process::{Command, Stdio};
use std::time::Duration;

fn run_signal_test(sig: i32, expected_code: i32) {
    let td = tempfile::tempdir().unwrap();
    let bin = assert_cmd::cargo::cargo_bin("codex-session");
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sleep.sh");
    let pid_file = td.path().join("child.pid");
    assert!(fixture.is_file());

    let script = concat!(
        "import pathlib, subprocess, sys; ",
        "child = subprocess.Popen([sys.argv[1], 'just-pass-through']); ",
        "pathlib.Path(sys.argv[2]).write_text(str(child.pid), encoding='utf-8'); ",
        "rc = child.wait(); ",
        "sys.exit(128 + (-rc) if rc < 0 else rc)",
    );

    let mut child = Command::new("python3")
        .arg("-c")
        .arg(script)
        .arg(&bin)
        .arg(&pid_file)
        .env_clear()
        .env("HOME", td.path())
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("CODEX_SESSION_CHILD_BIN", &fixture)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    for _ in 0..30 {
        if pid_file.is_file() {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(pid_file.is_file(), "signal test never wrote child pid");
    let pid = std::fs::read_to_string(&pid_file).unwrap();
    let pid = pid.trim();

    Command::new("kill")
        .arg(format!("-{sig}"))
        .arg(pid)
        .status()
        .unwrap();

    let status = child.wait().unwrap();
    assert_eq!(
        status.code(),
        Some(expected_code),
        "expected exit {expected_code} for signal {sig}, got {status:?}"
    );
}

#[test]
fn sigint_propagates_as_130() {
    run_signal_test(libc::SIGINT, 130);
}

#[test]
fn sigterm_propagates_as_143() {
    run_signal_test(libc::SIGTERM, 143);
}

#[test]
fn sighup_propagates_as_129() {
    run_signal_test(libc::SIGHUP, 129);
}
