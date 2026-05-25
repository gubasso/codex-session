#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

mod support;

use support::TestEnv;

/// When the account seed exists, the gate should pass through (Ready).
#[test]
fn gate_passes_when_seed_exists() {
    let env = TestEnv::new();

    let child_dir = env.make_fake_codex_in_dir("ok-child", "#!/usr/bin/env bash\nexit 0\n");
    let child_bin = child_dir.join("codex");

    env.cmd()
        .env("CODEX_SESSION_CHILD_BIN", &child_bin)
        .arg("exec")
        .assert()
        .success();
}
