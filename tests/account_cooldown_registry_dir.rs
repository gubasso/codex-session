#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

mod support;

use predicates::prelude::*;
use support::{TestEnv, fixture_path};

#[test]
fn cooldown_honors_custom_registry_dir() {
    let env = TestEnv::new();
    let custom_registry = env.state_session_root().join("custom-accounts");

    env.cmd()
        .env("CODEX_SESSION_ACCOUNT_REGISTRY_DIR", &custom_registry)
        .args(["account", "add", "work"])
        .assert()
        .success();

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
        .args(["--account", "work", "account", "cooldown", "show", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"cooled-down\": true"));
}
