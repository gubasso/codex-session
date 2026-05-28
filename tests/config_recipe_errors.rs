#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

mod support;

use std::os::unix::fs::symlink;

use predicates::prelude::*;
use support::TestEnv;

#[test]
fn missing_config_recipe_manifest_exits_seventy_eight() {
    let env = TestEnv::new();
    env.cmd()
        .args(["--config-recipe", "missing", "config-recipe", "show"])
        .assert()
        .code(78)
        .stderr(predicate::str::contains(
            "config-recipe `missing` not found",
        ));
}

#[test]
fn malformed_manifest_schema_exits_seventy_eight() {
    let env = TestEnv::new();
    env.write_config_recipe_manifest("default", "config-layers: notalist\n");
    env.cmd()
        .args(["config-recipe", "show"])
        .assert()
        .code(78)
        .stderr(predicate::str::contains("invalid config-recipe manifest"));
}

#[test]
fn missing_layer_exits_seventy_eight() {
    let env = TestEnv::new();
    env.write_config_recipe_manifest("default", "config-layers:\n  - base\n");
    env.cmd()
        .args(["config-recipe", "compose"])
        .assert()
        .code(78)
        .stderr(predicate::str::contains("config layer `base` not found"));
}

#[test]
fn malformed_toml_layer_exits_seventy_eight() {
    let env = TestEnv::new();
    env.install_config_recipe(
        "default",
        "config-layers:\n  - base\n",
        &[("base", "[model\n")],
    );
    env.cmd()
        .args(["config-recipe", "compose"])
        .assert()
        .code(78)
        .stderr(predicate::str::contains("config layer parse error"));
}

#[test]
fn invalid_env_key_exits_seventy_eight() {
    let env = TestEnv::new();
    env.install_config_recipe(
        "default",
        "config-layers:\n  - base\n",
        &[("base", "[env]\nBAD-KEY = \"x\"\n")],
    );
    env.cmd()
        .args(["config-recipe", "compose"])
        .assert()
        .code(78)
        .stderr(predicate::str::contains("invalid config-recipe env key"));
}

#[test]
fn wrapper_private_env_key_exits_seventy_eight() {
    let env = TestEnv::new();
    env.install_config_recipe(
        "default",
        "config-layers:\n  - base\n",
        &[("base", "[env]\nCODEX_SESSION_CONFIG_RECIPE = \"x\"\n")],
    );
    env.cmd()
        .env(
            "CODEX_SESSION_CHILD_BIN",
            support::fixture_path("echo-env.sh"),
        )
        .args(["exec"])
        .assert()
        .code(78)
        .stderr(predicate::str::contains("wrapper-private"));
}

#[test]
fn symlink_runtime_root_is_rejected() {
    let env = TestEnv::new();
    let runtime_link = env.tmp.path().join("runtime-link");
    symlink(env.tmp.path().join("nowhere"), &runtime_link).unwrap();
    env.cmd()
        .env("XDG_RUNTIME_DIR", &runtime_link)
        .args(["config", "status"])
        .assert()
        .success()
        .stdout(predicate::str::contains("session-source: state"));
}
