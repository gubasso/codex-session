#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

use std::os::unix::fs::PermissionsExt as _;
use std::process::{Command, Stdio};
use std::time::Duration;

#[test]
fn auth_bridge_signal_persists_on_sigterm() {
    let td = tempfile::tempdir().unwrap();
    let home = td.path().join("home");
    std::fs::create_dir_all(&home).unwrap();

    let fixture = td.path().join("child.sh");
    let ready_flag = td.path().join("child-ready");
    let payload = r#"{"tokens":{"last_refresh":"2026-06-01T00:00:00Z","access_token":"signaled"}}"#;
    // The fixture writes auth.json, chmods it, then touches a ready sentinel
    // outside $CODEX_HOME. The test polls that sentinel before sending
    // SIGTERM so the auth.json write is guaranteed to be complete first.
    std::fs::write(
        &fixture,
        format!(
            r#"#!/usr/bin/env bash
cat > "$CODEX_HOME/auth.json" <<'EOF'
{payload}
EOF
chmod 600 "$CODEX_HOME/auth.json"
touch "{ready}"
sleep 30
"#,
            ready = ready_flag.display(),
        ),
    )
    .unwrap();
    let mut perms = std::fs::metadata(&fixture).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&fixture, perms).unwrap();

    let bin = assert_cmd::cargo::cargo_bin("codex-session");
    let pid_file = td.path().join("child.pid");
    let script = concat!(
        "import pathlib, subprocess, sys; ",
        "child = subprocess.Popen([sys.argv[1], 'exec']); ",
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
    assert!(pid_file.is_file(), "signal test never wrote wrapper pid");
    assert!(
        ready_flag.is_file(),
        "fake child never finished writing auth.json before signal"
    );

    let pid = std::fs::read_to_string(&pid_file).unwrap();
    Command::new("kill")
        .arg(format!("-{}", libc::SIGTERM))
        .arg(pid.trim())
        .status()
        .unwrap();

    let status = child.wait().unwrap();
    assert_eq!(status.code(), Some(143));
    assert_eq!(
        std::fs::read_to_string(home.join(".codex/auth.json"))
            .unwrap()
            .trim_end(),
        payload
    );
}
