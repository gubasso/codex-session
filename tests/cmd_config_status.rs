#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

pub mod support;

use insta::{assert_json_snapshot, assert_snapshot};
use predicates::prelude::*;
use support::TestEnv;

#[test]
fn config_status_text_snapshot() {
    let env = TestEnv::new();
    env.install_config_recipe(
        "default",
        "config-layers:\n  - base\n  - work\n",
        &[
            ("base", "[model]\ndefault = \"gpt-5\"\n"),
            ("work", "[model]\ndefault = \"gpt-5-codex\"\n"),
        ],
    );
    env.make_fake_codex_printing_stdout("ignored");

    let output = env
        .cmd()
        .args(["--group", "stable", "config", "status"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = env.normalize_text(&String::from_utf8(output).unwrap());
    assert_snapshot!("config_status_text", stdout);
}

#[test]
fn config_status_json_snapshot() {
    let env = TestEnv::new();
    env.install_config_recipe(
        "default",
        "config-layers:\n  - base\n",
        &[("base", "[model]\ndefault = \"gpt-5\"\n")],
    );
    env.make_fake_codex_printing_stdout("ignored");

    let output = env
        .cmd()
        .args(["--group", "stable", "config", "status", "--format", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let mut value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    env.normalize_json(&mut value);
    assert_eq!(value["account-source"], "auto");
    assert!(value["accounts-count"].is_number());
    assert!(value["active-account-has-auth"].is_boolean());
    assert!(value["accounts-in-cooldown"].is_number());
    assert_json_snapshot!("config_status_json", value);
}

#[test]
fn config_status_shows_cooldown_count() {
    let env = TestEnv::new();
    env.make_fake_codex_printing_stdout("ignored");
    env.seed_account("acct-a", "{\"token\":\"test\"}\n");
    env.seed_account("acct-b", "{\"token\":\"test\"}\n");
    let cooldown_path = env.named_account_root("acct-a").join("cooldown.json");
    let cooldown_json = serde_json::json!({
        "reset_at_unix": 9_999_999_999_u64,
        "reason": "429",
        "last_429_at_unix": 9_999_999_000_u64,
        "snippet_truncated": "test"
    });
    std::fs::write(&cooldown_path, cooldown_json.to_string()).unwrap();
    let output = env
        .cmd()
        .args(["--group", "stable", "config", "status", "--format", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(value["accounts-in-cooldown"], 1);
}

#[test]
fn config_status_with_missing_recipe_surfaces_inline_error_text() {
    // Plan Phase 12, Step D6: `config status` must remain an introspection
    // command — when the active config-recipe cannot be resolved, the text renderer
    // emits the active-config-recipe name plus the per-layer error, instead of
    // aborting with a non-zero exit.
    let env = TestEnv::new();
    env.make_fake_codex_printing_stdout("ignored");
    env.cmd()
        .args(["--config-recipe", "missing", "config", "status"])
        .assert()
        .success()
        .stdout(predicate::str::contains("active-config-recipe: missing"))
        .stdout(predicate::str::contains(
            "error: config-recipe `missing` not found",
        ));
}

#[test]
fn config_status_with_missing_recipe_surfaces_inline_error_json() {
    let env = TestEnv::new();
    env.make_fake_codex_printing_stdout("ignored");
    let output = env
        .cmd()
        .args([
            "--config-recipe",
            "missing",
            "config",
            "status",
            "--format",
            "json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(value["active-config-recipe"], "missing");
    let layer_paths = value["layer-paths"].as_array().unwrap();
    assert_eq!(layer_paths.len(), 1);
    let error = layer_paths[0]["error"].as_str().unwrap();
    assert!(
        error.contains("not found"),
        "expected `not found` in layer error; got {error}"
    );
}
