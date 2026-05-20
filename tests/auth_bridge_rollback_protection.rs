#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

mod support;

use std::os::unix::fs::PermissionsExt as _;
use std::process::Stdio;
use std::time::{Duration, Instant};

use support::TestEnv;

fn native_dir(env: &TestEnv) -> std::path::PathBuf {
    env.home.join(".codex")
}

fn native_auth(env: &TestEnv) -> std::path::PathBuf {
    native_dir(env).join("auth.json")
}

#[test]
fn auth_bridge_rollback_protection() {
    let env = TestEnv::new();
    let native_payload =
        r#"{"tokens":{"last_refresh":"2026-06-01T00:00:00Z","access_token":"newer"}}"#;
    let stale_payload =
        r#"{"tokens":{"last_refresh":"2026-05-01T00:00:00Z","access_token":"stale"}}"#;

    std::fs::create_dir_all(native_dir(&env)).unwrap();
    std::fs::set_permissions(native_dir(&env), std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::write(native_auth(&env), native_payload).unwrap();
    std::fs::set_permissions(native_auth(&env), std::fs::Permissions::from_mode(0o600)).unwrap();

    let child_dir = env.make_fake_codex_in_dir(
        "stale-child",
        &format!(
            r#"#!/usr/bin/env bash
cat > "$CODEX_HOME/auth.json" <<'EOF'
{stale_payload}
EOF
chmod 600 "$CODEX_HOME/auth.json"
"#
        ),
    );
    let child_bin = child_dir.join("codex");

    let output = env
        .cmd()
        .args(["--log-stderr", "-v", "exec"])
        .env("CODEX_SESSION_CHILD_BIN", &child_bin)
        .assert()
        .success()
        .get_output()
        .stderr
        .clone();
    let stderr = String::from_utf8(output).unwrap();

    assert_eq!(
        std::fs::read_to_string(native_auth(&env)).unwrap(),
        native_payload
    );
    assert!(
        stderr.contains("skip-stale"),
        "missing skip-stale log: {stderr}"
    );
}

#[test]
fn watcher_skips_live_malformed_session_instead_of_rolling_back() {
    let env = TestEnv::new();
    let native_payload =
        r#"{"tokens":{"last_refresh":"2026-06-01T00:00:00Z","access_token":"native"}}"#;

    std::fs::create_dir_all(native_dir(&env)).unwrap();
    std::fs::set_permissions(native_dir(&env), std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::write(native_auth(&env), native_payload).unwrap();
    std::fs::set_permissions(native_auth(&env), std::fs::Permissions::from_mode(0o600)).unwrap();

    let child_dir = env.make_fake_codex_in_dir(
        "watcher-malformed-session",
        "#!/usr/bin/env bash\nsleep 3\n",
    );
    let child_bin = child_dir.join("codex");

    let mut terminal_a = env.std_cmd();
    terminal_a
        .arg("exec")
        .env("CODEX_SESSION_CHILD_BIN", &child_bin)
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut child_a = terminal_a.spawn().unwrap();

    let session_deadline = Instant::now() + Duration::from_millis(1_500);
    while (env.session_dirs().is_empty() || !env.session_dir().join("auth.json").is_file())
        && Instant::now() < session_deadline
    {
        std::thread::sleep(Duration::from_millis(50));
    }
    let session_auth = env.session_dir().join("auth.json");
    assert!(
        session_auth.is_file(),
        "terminal A never seeded session auth"
    );

    std::fs::write(&session_auth, r#"{"tokens": {"#).unwrap();
    std::fs::set_permissions(&session_auth, std::fs::Permissions::from_mode(0o600)).unwrap();
    std::thread::sleep(Duration::from_millis(1_500));

    assert_eq!(
        std::fs::read_to_string(native_auth(&env)).unwrap(),
        native_payload
    );
    assert!(child_a.wait().unwrap().success());
}
