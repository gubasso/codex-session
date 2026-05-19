#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

mod support;

use insta::{assert_json_snapshot, assert_snapshot};
use support::TestEnv;

#[test]
fn profile_list_text_snapshot() {
    let env = TestEnv::new();
    env.install_profile("default", "settings-layers:\n  - base\n", &[("base", "")]);
    env.install_profile(
        "work",
        "settings-layers:\n  - base\n  - work\n",
        &[("work", "")],
    );

    let output = env
        .cmd()
        .args(["profile", "list"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_snapshot!(
        "profile_list_text",
        env.normalize_text(&String::from_utf8(output).unwrap())
    );
}

#[test]
fn profile_list_json_snapshot() {
    let env = TestEnv::new();
    env.install_profile("default", "settings-layers:\n  - base\n", &[("base", "")]);
    let output = env
        .cmd()
        .args(["profile", "list", "--format", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let mut value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    env.normalize_json(&mut value);
    assert_json_snapshot!("profile_list_json", value);
}
