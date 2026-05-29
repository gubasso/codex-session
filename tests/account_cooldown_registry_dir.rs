#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

mod support;

use predicates::prelude::*;
use support::{TestEnv, fixture_path};

#[test]
fn cooldown_honors_custom_registry_dir() {
    let env = TestEnv::new();
    let custom_registry = env.state_session_root().join("custom-accounts");

    // Seed the account directly into the custom registry directory.
    {
        use std::os::unix::fs::PermissionsExt as _;
        let account_dir = custom_registry.join("work");
        let groups_dir = account_dir.join("groups");
        std::fs::create_dir_all(&groups_dir).unwrap();
        std::fs::set_permissions(&account_dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::set_permissions(&groups_dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        let auth_path = account_dir.join("auth.json");
        std::fs::write(&auth_path, "{\"token\":\"test\"}\n").unwrap();
        std::fs::set_permissions(&auth_path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }

    env.cmd()
        .env("CODEX_SESSION_ACCOUNT_REGISTRY_DIR", &custom_registry)
        .env("CODEX_SESSION_CHILD_BIN", fixture_path("fake-429.sh"))
        .args([
            "--account",
            "auto",
            "--max-retries",
            "1",
            "exec",
            "trigger 429",
        ])
        .assert()
        .failure()
        .code(75);

    let custom_cooldown = custom_registry.join("work/cooldown.json");
    assert!(custom_cooldown.exists());
    assert!(
        !env.state_session_root()
            .join("accounts/work/cooldown.json")
            .exists()
    );

    env.cmd()
        .env("CODEX_SESSION_ACCOUNT_REGISTRY_DIR", &custom_registry)
        .args([
            "--account",
            "work",
            "account",
            "cooldown",
            "show",
            "--format",
            "json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"cooled-down\": true"));
}
