#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

pub mod support;

use insta::assert_snapshot;
use support::{TestEnv, color};

#[test]
fn account_help_snapshot() {
    let env = TestEnv::new();
    let mut cmd = env.cmd();
    let stdout = color::with_no_color(&mut cmd)
        .args(["account", "--help"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let normalized = env.normalize_text(&String::from_utf8(stdout).unwrap());
    assert_snapshot!("account_help", normalized);
}

#[test]
fn account_cooldown_help_snapshot() {
    let env = TestEnv::new();
    let mut cmd = env.cmd();
    let stdout = color::with_no_color(&mut cmd)
        .args(["account", "cooldown", "--help"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let normalized = env.normalize_text(&String::from_utf8(stdout).unwrap());
    assert_snapshot!("account_cooldown_help", normalized);
}
