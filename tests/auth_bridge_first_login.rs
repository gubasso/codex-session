#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

mod support;

use std::os::unix::fs::MetadataExt as _;
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
fn auth_bridge_first_login() {
    let env = TestEnv::new();
    let new_payload =
        r#"{"last_refresh":"2026-06-01T00:00:00Z","tokens":{"access_token":"first-login"}}"#;

    let child_dir = env.make_fake_codex_in_dir(
        "first-login-child",
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
fn watcher_propagates_first_login_to_concurrent_session() {
    let env = TestEnv::new();
    let payload =
        r#"{"last_refresh":"2026-06-15T00:00:00Z","tokens":{"access_token":"watcher-login"}}"#;
    let ready_a = env.home.join("ready-a");

    let child_dir_a = env.make_fake_codex_in_dir(
        "watcher-first-login-a",
        &format!(
            r#"#!/usr/bin/env bash
cat > "$CODEX_HOME/auth.json" <<'EOF'
{payload}
EOF
chmod 600 "$CODEX_HOME/auth.json"
touch "{ready}"
sleep 3
"#,
            ready = ready_a.display(),
        ),
    );
    let child_bin_a = child_dir_a.join("codex");

    let mut terminal_a = env.std_cmd();
    terminal_a
        .arg("--log-stderr")
        .arg("-v")
        .arg("exec")
        .env("CODEX_SESSION_CHILD_BIN", &child_bin_a)
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    let child_a = terminal_a.spawn().unwrap();

    // Wait until the fake child has finished its first-write phase before
    // checking the eager native sync.
    let deadline = Instant::now() + Duration::from_millis(1_500);
    while !ready_a.is_file() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        ready_a.is_file(),
        "terminal A never wrote its ready sentinel"
    );

    let native_deadline = Instant::now() + Duration::from_millis(1_500);
    while (!native_auth(&env).is_file()
        || std::fs::read_to_string(native_auth(&env))
            .unwrap_or_default()
            .trim_end()
            != payload)
        && Instant::now() < native_deadline
    {
        std::thread::sleep(Duration::from_millis(50));
    }

    assert_eq!(
        std::fs::read_to_string(native_auth(&env))
            .unwrap()
            .trim_end(),
        payload
    );
    let native_meta = std::fs::metadata(native_auth(&env)).unwrap();
    assert_eq!(native_meta.permissions().mode() & 0o777, 0o600);
    assert_eq!(native_meta.uid(), rustix::process::getuid().as_raw());
    let session_a = env.session_dir();

    let child_dir_b =
        env.make_fake_codex_in_dir("watcher-first-login-b", "#!/usr/bin/env bash\nexit 0\n");
    let child_bin_b = child_dir_b.join("codex");
    env.cmd()
        .env("CODEX_SESSION_CHILD_BIN", &child_bin_b)
        .arg("exec")
        .assert()
        .success();

    let session_deadline = Instant::now() + Duration::from_millis(1_500);
    while (env.session_dirs().len() < 2
        || !env.session_dirs().iter().any(|dir| {
            std::fs::read_to_string(dir.join("auth.json"))
                .unwrap_or_default()
                .trim_end()
                == payload
        }))
        && Instant::now() < session_deadline
    {
        std::thread::sleep(Duration::from_millis(50));
    }

    assert!(
        env.session_dirs().len() >= 2,
        "terminal B never created a second session directory"
    );
    assert!(
        env.session_dirs().iter().any(|dir| {
            dir != &session_a
                && std::fs::read_to_string(dir.join("auth.json"))
                    .unwrap_or_default()
                    .trim_end()
                    == payload
        }),
        "terminal B never received the eagerly synced auth payload"
    );

    let output_a = child_a.wait_with_output().unwrap();
    let stderr = String::from_utf8(output_a.stderr).unwrap();
    assert!(
        !stderr.contains("status=error"),
        "terminal A emitted error-severity auth logs:\n{stderr}"
    );
    assert!(output_a.status.success());
}
