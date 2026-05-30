#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

mod support;

use support::{TestEnv, fixture_path};

#[test]
fn pinned_retry_warns_and_reuses_same_account() {
    let env = TestEnv::new();
    env.seed_account("work", "{\"token\":\"test\"}\n");

    let assert = env
        .cmd()
        .env("CODEX_SESSION_CHILD_BIN", fixture_path("fake-429.sh"))
        .args(["--account", "work", "--max-retries", "2", "exec", "hi"])
        .assert()
        .failure()
        .code(1);

    let stderr = String::from_utf8(assert.get_output().stderr.clone()).unwrap();
    assert!(!stderr.contains("rotating to next account"));
    let homes: Vec<_> = stderr
        .lines()
        .filter_map(|line| line.strip_prefix("marker:codex-session-fake-429 home="))
        .collect();
    assert_eq!(homes.len(), 1, "expected pinned retry to short-circuit");
    assert!(
        homes
            .iter()
            .all(|home| home.contains("/accounts/work/groups/"))
    );
    assert!(
        !env.named_account_root("work")
            .join("cooldown.json")
            .exists()
    );
}
