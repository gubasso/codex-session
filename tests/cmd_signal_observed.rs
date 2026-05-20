#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

//! When the wrapper observes a fatal signal but the wrapped child traps
//! it and exits 0, the wrapper still reports `child-signaled` exit codes
//! (128 + sig). The kernel never reports a signal on the child in this
//! case, so the wrapper's own `observed_signal` is the only signal the
//! user expects to see propagated.

use std::os::unix::fs::PermissionsExt as _;
use std::process::{Command, Stdio};
use std::time::Duration;

fn run_observed_signal_test(sig: i32, expected_code: i32) {
    let td = tempfile::tempdir().unwrap();
    let home = td.path().join("home");
    std::fs::create_dir_all(&home).unwrap();

    let fixture = td.path().join("trap-child.sh");
    let ready_flag = td.path().join("child-ready");
    // Trap the three fatal signals and exit 0 immediately. The wrapper
    // must still surface the signal it itself observed.
    std::fs::write(
        &fixture,
        format!(
            r#"#!/usr/bin/env bash
trap 'exit 0' INT TERM HUP
touch "{ready}"
# Sleep in short increments so the trap fires promptly.
for _ in $(seq 1 60); do
    sleep 0.5
done
exit 0
"#,
            ready = ready_flag.display(),
        ),
    )
    .unwrap();
    let mut perms = std::fs::metadata(&fixture).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&fixture, perms).unwrap();

    let bin = assert_cmd::cargo::cargo_bin("codex-session");
    let pid_file = td.path().join("wrapper.pid");
    let script = concat!(
        "import pathlib, subprocess, sys; ",
        "child = subprocess.Popen([sys.argv[1], 'exec']); ",
        "pathlib.Path(sys.argv[2]).write_text(str(child.pid), encoding='utf-8'); ",
        "rc = child.wait(); ",
        "sys.exit(128 + (-rc) if rc < 0 else rc)",
    );

    let mut wrapper = Command::new("python3")
        .arg("-c")
        .arg(script)
        .arg(&bin)
        .arg(&pid_file)
        .env_clear()
        .env("HOME", &home)
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("CODEX_SESSION_CHILD_BIN", &fixture)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    for _ in 0..50 {
        if pid_file.is_file() && ready_flag.is_file() {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(pid_file.is_file(), "wrapper pid never recorded");
    assert!(ready_flag.is_file(), "trap-child never installed its trap");

    let pid = std::fs::read_to_string(&pid_file).unwrap();
    Command::new("kill")
        .arg(format!("-{sig}"))
        .arg(pid.trim())
        .status()
        .unwrap();

    let status = wrapper.wait().unwrap();
    assert_eq!(
        status.code(),
        Some(expected_code),
        "wrapper observed signal {sig} but child exited 0; \
            expected wrapper exit {expected_code}, got {status:?}",
    );
}

#[test]
fn observed_sigint_surfaces_as_130_even_when_child_exits_cleanly() {
    run_observed_signal_test(libc::SIGINT, 130);
}

#[test]
fn observed_sigterm_surfaces_as_143_even_when_child_exits_cleanly() {
    run_observed_signal_test(libc::SIGTERM, 143);
}

#[test]
fn observed_sighup_surfaces_as_129_even_when_child_exits_cleanly() {
    run_observed_signal_test(libc::SIGHUP, 129);
}
