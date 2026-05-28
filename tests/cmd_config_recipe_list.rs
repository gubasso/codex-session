#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

mod support;

use insta::{assert_json_snapshot, assert_snapshot};
use support::TestEnv;

#[test]
fn config_recipe_list_text_snapshot() {
    let env = TestEnv::new();
    env.install_config_recipe("default", "config-layers:\n  - base\n", &[("base", "")]);
    env.install_config_recipe(
        "work",
        "config-layers:\n  - base\n  - work\n",
        &[("work", "")],
    );

    let output = env
        .cmd()
        .args(["config-recipe", "list"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_snapshot!(
        "config_recipe_list_text",
        env.normalize_text(&String::from_utf8(output).unwrap())
    );
}

#[test]
fn config_recipe_list_json_snapshot() {
    let env = TestEnv::new();
    env.install_config_recipe("default", "config-layers:\n  - base\n", &[("base", "")]);
    let output = env
        .cmd()
        .args(["config-recipe", "list", "--format", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let mut value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    env.normalize_json(&mut value);
    assert_json_snapshot!("config_recipe_list_json", value);
}
