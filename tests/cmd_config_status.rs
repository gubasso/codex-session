#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

pub mod support;

use insta::{assert_json_snapshot, assert_snapshot};
use predicates::prelude::*;
use support::TestEnv;

#[test]
fn config_status_text_snapshot() {
    let env = TestEnv::new();
    env.install_profile(
        "default",
        "settings-layers:\n  - base\n  - work\n",
        &[
            ("base", "[model]\ndefault = \"gpt-5\"\n"),
            ("work", "[model]\ndefault = \"gpt-5-codex\"\n"),
        ],
    );
    env.make_fake_codex_printing_stdout("ignored");

    let output = env
        .cmd()
        .args(["config", "status"])
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
    env.install_profile(
        "default",
        "settings-layers:\n  - base\n",
        &[("base", "[model]\ndefault = \"gpt-5\"\n")],
    );
    env.make_fake_codex_printing_stdout("ignored");

    let output = env
        .cmd()
        .args(["config", "status", "--format", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let mut value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    env.normalize_json(&mut value);
    assert_json_snapshot!("config_status_json", value);
}

#[test]
fn config_status_with_missing_profile_surfaces_inline_error_text() {
    // Plan Phase 12, Step D6: `config status` must remain an introspection
    // command — when the active profile cannot be resolved, the text renderer
    // emits the active-profile name plus the per-layer error, instead of
    // aborting with a non-zero exit.
    let env = TestEnv::new();
    env.make_fake_codex_printing_stdout("ignored");
    env.cmd()
        .args(["--profile", "missing", "config", "status"])
        .assert()
        .success()
        .stdout(predicate::str::contains("active-profile: missing"))
        .stdout(predicate::str::contains(
            "error: profile `missing` not found",
        ));
}

#[test]
fn config_status_with_missing_profile_surfaces_inline_error_json() {
    let env = TestEnv::new();
    env.make_fake_codex_printing_stdout("ignored");
    let output = env
        .cmd()
        .args([
            "--profile",
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
    assert_eq!(value["active-profile"], "missing");
    let layer_paths = value["layer-paths"].as_array().unwrap();
    assert_eq!(layer_paths.len(), 1);
    let error = layer_paths[0]["error"].as_str().unwrap();
    assert!(
        error.contains("not found"),
        "expected `not found` in layer error; got {error}"
    );
}
