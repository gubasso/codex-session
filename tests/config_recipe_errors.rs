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

/// What an operator reads when a manifest does not parse: the four lines of
/// the CLI style guide, the manifest path, and one parser message that names
/// the line and column where the parser has them. The message is written for
/// the operator, so it gives no advice about the YAML crate's own API. An
/// empty explicit document has no mapping to point into, so it names no place.
#[test]
fn manifest_parse_error_names_the_file_and_the_place() {
    let cases = [
        ("config-layers: [base\n", Some("line 1, column 16")),
        (
            "config-layers: [base]\nconfig-layers: [base]\n",
            Some("line 2, column 1"),
        ),
        ("config-layers: [base]\nnope: 1\n", Some("line 2, column 1")),
        ("profile-files: [a]\n", Some("line 1, column 1")),
        ("---\n", None),
    ];
    for (manifest, place) in cases {
        let env = TestEnv::new();
        env.write_config_recipe_manifest("default", manifest);
        let output = env.cmd().args(["config-recipe", "show"]).output().unwrap();
        assert_eq!(output.status.code(), Some(78), "{manifest:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        let lines: Vec<&str> = stderr.lines().collect();
        assert_eq!(lines.len(), 4, "{manifest:?}\n{stderr}");
        assert!(
            lines[0].starts_with("codex-session: config: config-recipe manifest parse error"),
            "{stderr}"
        );
        assert!(lines[1].trim_start().starts_with("where:"), "{stderr}");
        assert!(
            lines[1].ends_with("config-recipes/default.yaml"),
            "{stderr}"
        );
        assert!(lines[2].trim_start().starts_with("why:"), "{stderr}");
        let cause = lines[3]
            .trim_start()
            .strip_prefix("caused by:")
            .unwrap()
            .trim();
        assert!(!cause.is_empty(), "{stderr}");
        assert_eq!(stderr.matches(cause).count(), 1, "{stderr}");
        match place {
            Some(place) => assert!(cause.contains(place), "{manifest:?}\n{stderr}"),
            None => assert!(!cause.contains("line "), "{manifest:?}\n{stderr}"),
        }
        for advice in ["Options", "Policy", "Option<"] {
            assert!(!stderr.contains(advice), "{manifest:?}\n{stderr}");
        }
    }
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
