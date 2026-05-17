#![allow(clippy::unwrap_used)]

pub mod support;

use support::TestEnv;

#[test]
fn self_config_status_prints_booleans_and_needs_merge() {
    let env = TestEnv::new();
    env.install_base();
    let expected = format!(
        "base:        {} (exists=true)\n\
target:      {} (exists=false)\n\
stamp:       {} (exists=false)\n\
needs_merge: yes\n",
        env.base_path().display(),
        env.target_path().display(),
        env.stamp_path().display(),
    );
    env.cmd()
        .args(["self", "config-status"])
        .assert()
        .success()
        .stdout(expected);
}

#[test]
fn self_config_status_reports_needs_merge_no_when_base_is_missing() {
    let env = TestEnv::new();
    let expected = format!(
        "base:        {} (exists=false)\n\
target:      {} (exists=false)\n\
stamp:       {} (exists=false)\n\
needs_merge: no\n",
        env.base_path().display(),
        env.target_path().display(),
        env.stamp_path().display(),
    );
    env.cmd()
        .args(["self", "config-status"])
        .assert()
        .success()
        .stdout(expected);
}
