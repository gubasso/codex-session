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
fn auth_bridge_seed_and_persist() {
    let env = TestEnv::new();
    let old_payload = r#"{"tokens":{"last_refresh":"2026-01-01T00:00:00Z","access_token":"old"}}"#;
    let new_payload = r#"{"tokens":{"last_refresh":"2026-06-01T00:00:00Z","access_token":"new"}}"#;

    std::fs::create_dir_all(native_dir(&env)).unwrap();
    std::fs::set_permissions(native_dir(&env), std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::write(native_auth(&env), old_payload).unwrap();
    std::fs::set_permissions(native_auth(&env), std::fs::Permissions::from_mode(0o600)).unwrap();

    let child_dir = env.make_fake_codex_in_dir(
        "auth-child",
        &format!(
            r#"#!/usr/bin/env bash
cat > "$CODEX_HOME/auth.json" <<'EOF'
{new_payload}
EOF
chmod 600 "$CODEX_HOME/auth.json"
"#
        ),
    );
    let child_bin = child_dir.join("codex");

    env.cmd()
        .env("CODEX_SESSION_CHILD_BIN", &child_bin)
        .arg("exec")
        .assert()
        .success();

    assert_eq!(
        std::fs::read_to_string(native_auth(&env))
            .unwrap()
            .trim_end(),
        new_payload
    );
    assert_eq!(
        std::fs::metadata(native_auth(&env))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert_eq!(
        std::fs::metadata(native_dir(&env))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
}

#[test]
fn watcher_propagates_refresh_back_into_running_session() {
    let env = TestEnv::new();
    let initial_payload =
        r#"{"tokens":{"last_refresh":"2026-06-01T00:00:00Z","access_token":"initial"}}"#;
    let refreshed_payload =
        r#"{"tokens":{"last_refresh":"2026-06-01T00:01:00Z","access_token":"refreshed"}}"#;

    std::fs::create_dir_all(native_dir(&env)).unwrap();
    std::fs::set_permissions(native_dir(&env), std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::write(native_auth(&env), initial_payload).unwrap();
    std::fs::set_permissions(native_auth(&env), std::fs::Permissions::from_mode(0o600)).unwrap();

    let child_dir =
        env.make_fake_codex_in_dir("watcher-refresh-back", "#!/usr/bin/env bash\nsleep 3\n");
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

    std::fs::write(native_auth(&env), refreshed_payload).unwrap();
    std::fs::set_permissions(native_auth(&env), std::fs::Permissions::from_mode(0o600)).unwrap();

    let refresh_deadline = Instant::now() + Duration::from_millis(1_500);
    while std::fs::read_to_string(&session_auth)
        .unwrap_or_default()
        .trim_end()
        != refreshed_payload
        && Instant::now() < refresh_deadline
    {
        std::thread::sleep(Duration::from_millis(50));
    }

    assert_eq!(
        std::fs::read_to_string(&session_auth).unwrap().trim_end(),
        refreshed_payload
    );
    assert!(child_a.wait().unwrap().success());
}
