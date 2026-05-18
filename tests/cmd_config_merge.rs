#![allow(clippy::unwrap_used)]

pub mod support;

use predicates::prelude::*;
use support::TestEnv;

#[test]
fn config_merge_forces_rewrite_when_stamp_is_fresh() {
    let env = TestEnv::new();
    env.install_base();
    env.install_target_with_local();
    std::fs::create_dir_all(env.stamp_path().parent().unwrap()).unwrap();
    std::fs::write(env.stamp_path(), "").unwrap();
    env.touch_newer(&env.stamp_path());
    env.touch_older(&env.base_path());

    env.cmd()
        .args(["config", "merge"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("merged:")
                .and(predicate::str::contains("config.base.toml"))
                .and(predicate::str::contains("config.toml")),
        );
}

#[test]
fn config_merge_without_base_errors_exactly() {
    let env = TestEnv::new();
    env.cmd()
        .args(["config", "merge"])
        .assert()
        .code(66)
        .stdout("")
        .stderr(predicate::str::contains(
            "codex-session: failed to load base config",
        ));
}
