#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

pub mod support;

use predicates::prelude::*;
use support::TestEnv;

fn install_minimal_profile(env: &TestEnv) {
    env.install_profile(
        "default",
        "settings-layers:\n  - base\n",
        &[("base", "[model]\ndefault = \"gpt-5\"\n")],
    );
    env.make_fake_codex_printing_stdout("codex-stub 0.0.0");
}

#[test]
fn doctor_happy_path_exits_zero() {
    let env = TestEnv::new();
    install_minimal_profile(&env);

    env.cmd()
        .arg("doctor")
        .assert()
        .success()
        .stdout(predicate::str::contains("OK"))
        .stdout(predicate::str::contains("profile.active"))
        .stdout(predicate::str::contains("manifest.default.parse"))
        .stdout(predicate::str::contains("layer.default.base.parse"))
        .stdout(predicate::str::contains("composition.default.dry-run"))
        .stdout(predicate::str::contains("0 FAIL"));
}

#[test]
fn doctor_json_shape() {
    let env = TestEnv::new();
    install_minimal_profile(&env);

    let output = env
        .cmd()
        .args(["--format", "json", "doctor"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(value["profile"], "default");
    let checks = value["checks"].as_array().unwrap();
    assert!(!checks.is_empty());
    let summary = &value["summary"];
    assert_eq!(summary["fail"], 0);
    let names: Vec<_> = checks
        .iter()
        .map(|c| c.get("name").and_then(|v| v.as_str()).unwrap_or(""))
        .collect();
    assert!(names.contains(&"profile.active"));
    assert!(names.contains(&"composition.default.dry-run"));
    assert!(names.contains(&"session.root"));
}

#[test]
fn doctor_fails_when_layer_missing() {
    let env = TestEnv::new();
    env.make_fake_codex_printing_stdout("ignored");
    env.write_profile_manifest("default", "settings-layers:\n  - missing\n");

    env.cmd()
        .arg("doctor")
        .assert()
        .code(1)
        .stdout(predicate::str::contains("FAIL"))
        .stdout(predicate::str::contains("layer.default.missing.exists"));
}

#[test]
fn doctor_fails_on_broken_layer_toml() {
    let env = TestEnv::new();
    env.make_fake_codex_printing_stdout("ignored");
    env.install_profile(
        "default",
        "settings-layers:\n  - bad\n",
        &[("bad", "not = valid = toml\n")],
    );

    env.cmd()
        .arg("doctor")
        .assert()
        .code(1)
        .stdout(predicate::str::contains("FAIL"))
        .stdout(predicate::str::contains("layer.default.bad.parse"));
}

#[test]
fn doctor_fails_on_bad_env_key() {
    let env = TestEnv::new();
    env.make_fake_codex_printing_stdout("ignored");
    env.install_profile(
        "default",
        "settings-layers:\n  - secrets\n",
        &[("secrets", "[env]\n\"1BAD\" = \"value\"\n")],
    );

    env.cmd()
        .arg("doctor")
        .assert()
        .code(1)
        .stdout(predicate::str::contains("FAIL"))
        .stdout(predicate::str::contains("layer.default.secrets.env"));
}

#[test]
fn doctor_fails_on_reserved_env_prefix() {
    let env = TestEnv::new();
    env.make_fake_codex_printing_stdout("ignored");
    env.install_profile(
        "default",
        "settings-layers:\n  - rsv\n",
        &[("rsv", "[env]\nCODEX_SESSION_LOG_VERBOSE = \"1\"\n")],
    );

    env.cmd()
        .arg("doctor")
        .assert()
        .code(1)
        .stdout(predicate::str::contains("FAIL"))
        .stdout(predicate::str::contains("reserved CODEX_SESSION_"));
}

#[test]
fn doctor_warns_on_orphan_layer() {
    let env = TestEnv::new();
    install_minimal_profile(&env);
    env.write_settings_layer("orphan", "[model]\ndefault = \"unused\"\n");

    env.cmd()
        .arg("doctor")
        .assert()
        .success()
        .stdout(predicate::str::contains("WARN"))
        .stdout(predicate::str::contains("layers.orphan"))
        .stdout(predicate::str::contains("orphan"));
}

#[test]
fn doctor_warns_when_stock_mode() {
    let env = TestEnv::new();
    env.make_fake_codex_printing_stdout("ignored");

    let stdout = String::from_utf8(
        env.cmd()
            .arg("doctor")
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .unwrap();
    assert!(
        stdout.matches("profile.active").count() == 1,
        "stock-mode should emit exactly one profile.active row, got:\n{stdout}"
    );
    assert!(stdout.contains("stock mode"));
}

#[test]
fn doctor_all_profiles_aggregates() {
    let env = TestEnv::new();
    env.make_fake_codex_printing_stdout("ignored");
    env.install_profile(
        "default",
        "settings-layers:\n  - base\n",
        &[("base", "[model]\ndefault = \"gpt-5\"\n")],
    );
    env.write_profile_manifest("alt", "settings-layers:\n  - base\n");

    env.cmd()
        .args(["doctor", "--all-profiles"])
        .assert()
        .success()
        .stdout(predicate::str::contains("manifest.default.parse"))
        .stdout(predicate::str::contains("manifest.alt.parse"))
        .stdout(predicate::str::contains("composition.default.dry-run"))
        .stdout(predicate::str::contains("composition.alt.dry-run"));
}

#[test]
fn doctor_reports_runtime_when_session_root_not_yet_initialized() {
    // Regression: on a fresh install, `XDG_RUNTIME_DIR` exists but
    // `$XDG_RUNTIME_DIR/codex-session` does not. `doctor` must still
    // classify the candidate as `source: runtime` (with a friendly
    // "not yet initialized" note), not fall through to a misleading
    // `state — runtime fallback` warning.
    let env = TestEnv::new();
    install_minimal_profile(&env);

    let runtime_session_root = env.runtime.join("codex-session");
    assert!(
        !runtime_session_root.exists(),
        "test precondition: runtime session root must not exist yet"
    );

    let stdout = String::from_utf8(
        env.cmd()
            .arg("doctor")
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .unwrap();

    assert!(
        stdout.contains("source: runtime"),
        "session.root should classify as runtime when its parent is OK; got:\n{stdout}"
    );
    assert!(
        !stdout.contains("runtime fallback"),
        "session.root must not say `runtime fallback` when runtime is usable; got:\n{stdout}"
    );
    assert!(
        stdout.contains("not yet initialized"),
        "session.root should note that the root is not yet initialized; got:\n{stdout}"
    );
}

#[test]
fn doctor_does_not_create_session_root() {
    // Regression: `doctor` must be read-only. The previous implementation
    // called `resolve_session_root`, which creates `runtime/codex-session`
    // and `runtime/codex-session/sessions` (and chmods them to 0o700) as
    // a side effect. The validator should only inspect, never mutate.
    let env = TestEnv::new();
    install_minimal_profile(&env);

    let runtime_session_root = env.runtime.join("codex-session");
    assert!(
        !runtime_session_root.exists(),
        "test precondition: session root should not exist yet"
    );

    env.cmd().arg("doctor").assert().success();

    assert!(
        !runtime_session_root.exists(),
        "doctor must not create {}",
        runtime_session_root.display()
    );
}

#[test]
fn doctor_all_profiles_fails_when_profiles_dir_unreadable() {
    // Regression: previously `discover_profiles` silently collapsed any
    // `read_dir` error into "no manifests found", masking real
    // configuration problems. With the fix, a profiles directory that
    // cannot be read (here: a regular file at that path, which produces
    // ENOTDIR on read_dir) must surface as a hard FAIL.
    let env = TestEnv::new();
    env.make_fake_codex_printing_stdout("ignored");
    let profiles_path = env.config_home.join("codex-session/profiles");
    // The TestEnv ctor creates config_home/codex-session/ but not the
    // profiles/ subdir; planting a file there guarantees read_dir fails
    // with a non-NotFound error.
    std::fs::write(&profiles_path, "not a directory\n").unwrap();

    env.cmd()
        .args(["doctor", "--all-profiles"])
        .assert()
        .code(1)
        .stdout(predicate::str::contains("FAIL"))
        .stdout(predicate::str::contains("profiles.all"))
        .stdout(predicate::str::contains(
            "could not read profiles directory",
        ));
}

#[test]
fn doctor_show_env_redacts_secrets() {
    let env = TestEnv::new();
    env.make_fake_codex_printing_stdout("ignored");
    env.install_profile(
        "default",
        "settings-layers:\n  - vars\n",
        &[(
            "vars",
            "[env]\nMY_TOKEN = \"deadbeef\"\nINNOCENT = \"hello\"\n",
        )],
    );

    env.cmd()
        .args(["doctor", "--show-env"])
        .assert()
        .success()
        .stdout(predicate::str::contains("MY_TOKEN=***"))
        .stdout(predicate::str::contains("INNOCENT=hello"))
        .stdout(predicate::str::contains("deadbeef").not());
}
