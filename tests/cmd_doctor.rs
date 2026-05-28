#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

pub mod support;

use std::os::unix::fs::{PermissionsExt as _, symlink};

use predicates::prelude::*;
use support::TestEnv;

fn install_minimal_recipe(env: &TestEnv) {
    env.install_config_recipe(
        "default",
        "config-layers:\n  - base\n",
        &[("base", "[model]\ndefault = \"gpt-5\"\n")],
    );
    env.make_fake_codex_printing_stdout("codex-stub 0.0.0");
}

#[test]
fn doctor_happy_path_exits_zero() {
    let env = TestEnv::new();
    install_minimal_recipe(&env);
    env.seed_account("work", "{\"token\":\"test\"}\n");

    env.cmd()
        .arg("doctor")
        .assert()
        .success()
        .stdout(predicate::str::contains("OK"))
        .stdout(predicate::str::contains("config-recipe.active"))
        .stdout(predicate::str::contains("manifest.default.parse"))
        .stdout(predicate::str::contains("layer.default.base.parse"))
        .stdout(predicate::str::contains("composition.default.dry-run"))
        .stdout(predicate::str::contains("0 FAIL"));
}

#[test]
fn doctor_json_shape() {
    let env = TestEnv::new();
    install_minimal_recipe(&env);
    env.seed_account("work", "{\"token\":\"test\"}\n");

    let output = env
        .cmd()
        .args(["--format", "json", "doctor"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(value["config-recipe"], "default");
    assert_eq!(value["account"], "work");
    assert_eq!(value["account-source"], "lru");
    assert_eq!(value["active-account"]["name"], "work");
    assert_eq!(value["active-account"]["source"], "lru");
    assert!(value["accounts"].as_array().is_some());
    let accounts = value["accounts"].as_array().unwrap();
    for account in accounts {
        assert_eq!(account["cooldown-active"], false);
        assert!(account["cooldown-reset-at-unix"].is_null());
        assert!(account["cooldown-reason"].is_null());
    }
    assert!(value.get("group-id").is_some());
    assert!(value.get("group-id-source").is_some());
    assert!(value.get("codex-home").is_some());
    let checks = value["checks"].as_array().unwrap();
    assert!(!checks.is_empty());
    let summary = &value["summary"];
    assert_eq!(summary["fail"], 0);
    let names: Vec<_> = checks
        .iter()
        .map(|c| c.get("name").and_then(|v| v.as_str()).unwrap_or(""))
        .collect();
    assert!(names.contains(&"config-recipe.active"));
    assert!(names.contains(&"composition.default.dry-run"));
    assert!(names.contains(&"session.root"));
    assert!(names.contains(&"auth.native"));
}

#[test]
fn doctor_json_reports_cooldown_active_account() {
    let env = TestEnv::new();
    install_minimal_recipe(&env);
    env.seed_account("work", "{\"token\":\"test\"}\n");
    let cooldown_path = env.named_account_root("work").join("cooldown.json");
    std::fs::create_dir_all(cooldown_path.parent().unwrap()).unwrap();
    let cd_json = serde_json::json!({
        "reset_at_unix": 4_102_444_800_u64,
        "reason": "rate limited",
        "last_429_at_unix": 1,
        "snippet_truncated": "429"
    });
    std::fs::write(cooldown_path, cd_json.to_string()).unwrap();

    let output = env
        .cmd()
        .args(["--format", "json", "doctor"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    let accounts = value["accounts"].as_array().unwrap();
    assert!(
        accounts
            .iter()
            .any(|a| a["name"] == "work" && a["cooldown-active"] == true)
    );
}

#[test]
fn doctor_fails_when_active_account_missing_auth() {
    let env = TestEnv::new();
    install_minimal_recipe(&env);
    env.seed_account("work", "{\"token\":\"test\"}\n");
    env.cmd()
        .args(["account", "use", "work"])
        .assert()
        .success();
    std::fs::remove_file(env.named_account_auth_seed("work")).unwrap();

    let stdout = String::from_utf8(
        env.cmd()
            .arg("doctor")
            .assert()
            .code(1)
            .get_output()
            .stdout
            .clone(),
    )
    .unwrap();

    assert!(stdout.contains("account.active.auth"));
    assert!(stdout.contains("FAIL"));
    assert!(stdout.contains("account.active.auth: run `codex-session account refresh`"));
}

#[test]
fn doctor_warn_cooldown_appears_in_next_steps() {
    let env = TestEnv::new();
    install_minimal_recipe(&env);
    env.seed_account("work", "{\"token\":\"test\"}\n");
    let cooldown_path = env.named_account_root("work").join("cooldown.json");
    std::fs::create_dir_all(cooldown_path.parent().unwrap()).unwrap();
    let cd_json = serde_json::json!({
        "reset_at_unix": 4_102_444_800_u64,
        "reason": "rate limited",
        "last_429_at_unix": 1,
        "snippet_truncated": "429"
    });
    std::fs::write(cooldown_path, cd_json.to_string()).unwrap();

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
    assert!(stdout.contains("account.cooldowns"));
    assert!(stdout.contains("account.cooldowns: wait for cooldown to expire"));
}

#[test]
fn doctor_fails_when_layer_missing() {
    let env = TestEnv::new();
    env.make_fake_codex_printing_stdout("ignored");
    env.write_config_recipe_manifest("default", "config-layers:\n  - missing\n");

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
    env.install_config_recipe(
        "default",
        "config-layers:\n  - bad\n",
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
    env.install_config_recipe(
        "default",
        "config-layers:\n  - secrets\n",
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
    env.install_config_recipe(
        "default",
        "config-layers:\n  - rsv\n",
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
    install_minimal_recipe(&env);
    env.write_config_layer("orphan", "[model]\ndefault = \"unused\"\n");

    env.cmd()
        .arg("doctor")
        .assert()
        .success()
        .stdout(predicate::str::contains("WARN"))
        .stdout(predicate::str::contains("layers.orphan"))
        .stdout(predicate::str::contains("orphan"));
}

#[test]
fn doctor_warns_on_legacy_settings_dir_present() {
    let env = TestEnv::new();
    install_minimal_recipe(&env);
    let legacy_settings = env.config_home.join("codex-session/settings");
    std::fs::create_dir_all(&legacy_settings).unwrap();

    env.cmd()
        .arg("doctor")
        .assert()
        .success()
        .stdout(predicate::str::contains("WARN"))
        .stdout(predicate::str::contains("legacy-settings-dir"))
        .stdout(predicate::str::contains("settings"))
        .stdout(predicate::str::contains("docs/upstream-codex.md §F6c"));
}

#[test]
fn doctor_warns_on_legacy_profile_selector_in_layer() {
    let env = TestEnv::new();
    env.make_fake_codex_printing_stdout("ignored");
    env.install_config_recipe(
        "default",
        "config-layers:\n  - base\n",
        &[("base", "profile = \"deep\"\n")],
    );

    env.cmd()
        .arg("doctor")
        .assert()
        .code(1)
        .stdout(predicate::str::contains("WARN"))
        .stdout(predicate::str::contains("legacy-profile-form"))
        .stdout(predicate::str::contains(
            "top-level `profile = \"...\"` selectors",
        ))
        .stdout(predicate::str::contains("docs/upstream-codex.md §F6c"));
}

#[test]
fn doctor_warns_on_legacy_profiles_table_in_profile_file() {
    let env = TestEnv::new();
    env.make_fake_codex_printing_stdout("ignored");
    env.install_config_recipe(
        "default",
        "config-layers:\n  - base\nprofile-files:\n  - clean\n",
        &[("base", "model = \"gpt-5\"\n")],
    );
    env.write_profile_file("clean", "model = \"gpt-5.4\"\n");
    env.write_profile_file("deep", "[profiles.deep]\nmodel = \"gpt-5.4\"\n");

    env.cmd()
        .arg("doctor")
        .assert()
        .success()
        .stdout(predicate::str::contains("WARN"))
        .stdout(predicate::str::contains("legacy-profile-form"))
        .stdout(predicate::str::contains("deep.config.toml"))
        .stdout(predicate::str::contains("docs/upstream-codex.md §F6c"));
}

#[test]
fn doctor_warns_on_legacy_cache_file() {
    let env = TestEnv::new();
    install_minimal_recipe(&env);
    let legacy_cache = env.cache.join("codex-session/settings.toml");
    std::fs::create_dir_all(legacy_cache.parent().unwrap()).unwrap();
    std::fs::write(&legacy_cache, "model = \"gpt-5\"\n").unwrap();

    env.cmd()
        .arg("doctor")
        .assert()
        .success()
        .stdout(predicate::str::contains("WARN"))
        .stdout(predicate::str::contains("legacy-cache-config"))
        .stdout(predicate::str::contains("settings.toml"))
        .stdout(predicate::str::contains("configs.toml"))
        .stdout(predicate::str::contains("docs/upstream-codex.md §F6c"));
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
        stdout.matches("config-recipe.active").count() == 1,
        "stock-mode should emit exactly one config-recipe.active row, got:\n{stdout}"
    );
    assert!(stdout.contains("stock mode"));
}

#[test]
fn doctor_all_recipes_aggregates() {
    let env = TestEnv::new();
    env.make_fake_codex_printing_stdout("ignored");
    env.install_config_recipe(
        "default",
        "config-layers:\n  - base\n",
        &[("base", "[model]\ndefault = \"gpt-5\"\n")],
    );
    env.write_config_recipe_manifest("alt", "config-layers:\n  - base\n");

    env.cmd()
        .args(["doctor", "--all-config-recipes"])
        .assert()
        .success()
        .stdout(predicate::str::contains("manifest.default.parse"))
        .stdout(predicate::str::contains("manifest.alt.parse"))
        .stdout(predicate::str::contains("composition.default.dry-run"))
        .stdout(predicate::str::contains("composition.alt.dry-run"));
}

#[test]
fn doctor_reports_state_when_session_root_not_yet_initialized() {
    let env = TestEnv::new_empty();
    install_minimal_recipe(&env);

    let runtime_session_root = env.runtime.join("codex-session");
    assert!(
        !runtime_session_root.exists(),
        "test precondition: runtime session root must not exist yet"
    );

    // With no accounts, doctor now reports a FAIL for session.account,
    // so exit code is 1. The assertions below only concern session.root.
    let stdout = String::from_utf8(
        env.cmd()
            .arg("doctor")
            .assert()
            .code(1)
            .get_output()
            .stdout
            .clone(),
    )
    .unwrap();

    assert!(
        stdout.contains("source: state"),
        "session.root should classify as state on a fresh install; got:\n{stdout}"
    );
    assert!(
        !stdout.contains("runtime fallback"),
        "session.root must not say `runtime fallback` when state is usable; got:\n{stdout}"
    );
    assert!(
        stdout.contains("accounts/ not yet created"),
        "session.root should note that the accounts tree is not yet initialized; got:\n{stdout}"
    );
}

#[test]
fn doctor_does_not_create_session_root() {
    // Regression: `doctor` must be read-only. The previous implementation
    // called `resolve_session_root`, which creates `runtime/codex-session`
    // and `runtime/codex-session/sessions` (and chmods them to 0o700) as
    // a side effect. The validator should only inspect, never mutate.
    let env = TestEnv::new();
    install_minimal_recipe(&env);

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
fn doctor_all_recipes_fails_when_recipes_dir_unreadable() {
    // Regression: previously `discover_recipes` silently collapsed any
    // `read_dir` error into "no manifests found", masking real
    // configuration problems. With the fix, a config-recipes directory that
    // cannot be read (here: a regular file at that path, which produces
    // ENOTDIR on read_dir) must surface as a hard FAIL.
    let env = TestEnv::new();
    env.make_fake_codex_printing_stdout("ignored");
    let recipes_path = env.config_home.join("codex-session/config-recipes");
    // The TestEnv ctor creates config_home/codex-session/ but not the
    // config-recipes/ subdir; planting a file there guarantees read_dir fails
    // with a non-NotFound error.
    std::fs::write(&recipes_path, "not a directory\n").unwrap();

    env.cmd()
        .args(["doctor", "--all-config-recipes"])
        .assert()
        .code(1)
        .stdout(predicate::str::contains("FAIL"))
        .stdout(predicate::str::contains("config-recipes.all"))
        .stdout(predicate::str::contains(
            "could not read config-recipes directory",
        ));
}

#[test]
fn doctor_show_env_redacts_secrets() {
    let env = TestEnv::new();
    env.make_fake_codex_printing_stdout("ignored");
    env.install_config_recipe(
        "default",
        "config-layers:\n  - vars\n",
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

#[test]
fn doctor_reports_missing_native_auth_as_ok() {
    let env = TestEnv::new();
    install_minimal_recipe(&env);
    let _ = std::fs::remove_dir_all(env.home.join(".codex"));

    env.cmd()
        .arg("doctor")
        .assert()
        .success()
        .stdout(predicate::str::contains("auth.native"))
        .stdout(predicate::str::contains("no native codex home yet"));
}

#[test]
fn doctor_fails_on_symlinked_native_auth() {
    let env = TestEnv::new();
    install_minimal_recipe(&env);
    let native_dir = env.home.join(".codex");
    let _ = std::fs::remove_dir_all(&native_dir);
    std::fs::create_dir_all(&native_dir).unwrap();
    std::fs::set_permissions(&native_dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    let sentinel = env.home.join("sentinel-auth.json");
    std::fs::write(&sentinel, "sentinel-data").unwrap();
    symlink(&sentinel, native_dir.join("auth.json")).unwrap();

    env.cmd()
        .arg("doctor")
        .assert()
        .code(1)
        .stdout(predicate::str::contains("auth.native"))
        .stdout(predicate::str::contains("FAIL"))
        .stdout(predicate::str::contains("auth.json is a symlink"));
}

#[test]
fn doctor_warns_on_native_auth_dir_mode() {
    let env = TestEnv::new();
    install_minimal_recipe(&env);
    let native_dir = env.home.join(".codex");
    std::fs::create_dir_all(&native_dir).unwrap();
    std::fs::set_permissions(&native_dir, std::fs::Permissions::from_mode(0o755)).unwrap();

    env.cmd()
        .arg("doctor")
        .assert()
        .success()
        .stdout(predicate::str::contains("auth.native"))
        .stdout(predicate::str::contains("WARN"))
        .stdout(predicate::str::contains(
            "mode 0755; will be chmod'd on first login",
        ));
}
