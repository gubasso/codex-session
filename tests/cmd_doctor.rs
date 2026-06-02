#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

pub mod support;

use std::os::unix::fs::{PermissionsExt as _, symlink};

use predicates::prelude::*;
use support::{FakeCodexBehavior, TestEnv};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn install_minimal_recipe(env: &TestEnv) {
    env.install_config_recipe(
        "default",
        "config-layers:\n  - base\n",
        &[("base", "[model]\ndefault = \"gpt-5\"\n")],
    );
    env.make_fake_codex_printing_stdout("codex-stub 0.0.0");
}

fn install_ping_recipe(env: &TestEnv) {
    env.install_config_recipe(
        "default",
        "config-layers:\n  - base\nprofile-files:\n  - ping\n",
        &[("base", "[model]\ndefault = \"gpt-5\"\n")],
    );
    env.write_profile_file("ping", "model = \"gpt-5\"\n");
}

fn grouped_checks(value: &serde_json::Value) -> Vec<&serde_json::Value> {
    value["groups"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|group| group["checks"].as_array().unwrap().iter())
        .collect()
}

fn find_check<'a>(value: &'a serde_json::Value, name: &str) -> Option<&'a serde_json::Value> {
    grouped_checks(value)
        .into_iter()
        .find(|check| check["name"] == name)
}

fn has_group(value: &serde_json::Value, name: &str) -> bool {
    value["groups"]
        .as_array()
        .unwrap()
        .iter()
        .any(|group| group["name"] == name)
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
fn doctor_reports_codex_version_too_old() {
    let env = TestEnv::new();
    install_minimal_recipe(&env);
    env.make_fake_codex_with_version("codex 0.133.0", FakeCodexBehavior::Succeed);

    env.cmd()
        .arg("doctor")
        .assert()
        .code(1)
        .stdout(predicate::str::contains("codex.version"))
        .stdout(predicate::str::contains("FAIL"))
        .stdout(predicate::str::contains("0.133.0"))
        .stdout(predicate::str::contains("0.134.0"))
        .stdout(predicate::str::contains("docs/upstream-codex.md §F6c"));
}

#[test]
fn doctor_reports_codex_version_ok() {
    let env = TestEnv::new();
    install_minimal_recipe(&env);
    env.make_fake_codex_with_version("codex 0.134.0", FakeCodexBehavior::Succeed);

    env.cmd()
        .arg("doctor")
        .assert()
        .success()
        .stdout(predicate::str::contains("codex.version"))
        .stdout(predicate::str::contains("OK"))
        .stdout(predicate::str::contains("0.134.0"));
}

#[test]
fn doctor_warns_on_unparseable_codex_version() {
    let env = TestEnv::new();
    install_minimal_recipe(&env);
    env.make_fake_codex_with_version("weird-output", FakeCodexBehavior::Succeed);

    env.cmd()
        .arg("doctor")
        .assert()
        .success()
        .stdout(predicate::str::contains("codex.version"))
        .stdout(predicate::str::contains("WARN"))
        .stdout(predicate::str::contains("weird-output"));
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
    assert_eq!(value["account-source"], "auto");
    assert_eq!(value["active-account"]["name"], "work");
    assert_eq!(value["active-account"]["source"], "auto");
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
    let groups = value["groups"].as_array().unwrap();
    let all: Vec<&serde_json::Value> = groups
        .iter()
        .flat_map(|g| g["checks"].as_array().unwrap().iter())
        .collect();
    assert!(!all.is_empty());
    assert_eq!(value["summary"]["fail"], 0);
    assert!(
        value.get("checks").is_none(),
        "flat `checks` array must be gone"
    );
    let names: Vec<_> = all
        .iter()
        .map(|c| c.get("name").and_then(|v| v.as_str()).unwrap_or(""))
        .collect();
    assert!(names.contains(&"config-recipe.active"));
    assert!(names.contains(&"composition.default.dry-run"));
    assert!(names.contains(&"session.root"));
    assert!(names.contains(&"auth.native"));
}

#[test]
fn doctor_check_ping_config_warns_when_missing() {
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
    let check = find_check(&value, "config-recipe.ping-profile").unwrap();
    assert_eq!(check["status"], "warn");
}

#[test]
fn doctor_check_ping_config_ok_when_present() {
    let env = TestEnv::new();
    install_ping_recipe(&env);
    env.make_fake_codex_printing_stdout("codex-stub 0.0.0");
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
    let check = find_check(&value, "config-recipe.ping-profile").unwrap();
    assert_eq!(check["status"], "ok");
}

#[tokio::test]
async fn doctor_online_flag_runs_network_checks() {
    let env = TestEnv::new();
    install_ping_recipe(&env);
    env.make_fake_codex_with_version("codex 0.134.0", FakeCodexBehavior::Succeed);
    support::quota::add_oauth_account(&env, "work");
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/backend-api/wham/usage"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw(support::quota::default_payload(), "application/json"),
        )
        .mount(&server)
        .await;

    let output = env
        .cmd()
        .env(
            "CODEX_SESSION_WHAM_USAGE_URL",
            support::quota::wham_url(&server),
        )
        .args(["--format", "json", "doctor", "--online"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert!(has_group(&value, "online"));
    assert_eq!(
        find_check(&value, "online.token-probe").unwrap()["status"],
        "ok"
    );
    assert_eq!(
        find_check(&value, "online.quota-api").unwrap()["status"],
        "ok"
    );
}

#[test]
fn doctor_default_omits_online_group() {
    let env = TestEnv::new();
    install_ping_recipe(&env);
    env.make_fake_codex_with_version("codex 0.134.0", FakeCodexBehavior::Succeed);
    support::quota::add_oauth_account(&env, "work");

    let output = env
        .cmd()
        .args(["--format", "json", "doctor"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert!(!has_group(&value, "online"));
}

#[test]
fn doctor_trust_cache_ok_when_absent() {
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
    let check = find_check(&value, "session.trust-cache").unwrap();
    assert_eq!(check["status"], "ok");
    assert!(
        check["detail"]
            .as_str()
            .unwrap()
            .contains("no cache yet (fresh install)")
    );
}

#[test]
fn doctor_trust_cache_warns_on_stale_lock() {
    let env = TestEnv::new();
    install_minimal_recipe(&env);
    env.seed_account("work", "{\"token\":\"test\"}\n");
    env.write_cache_config("[projects]\n");
    let lock_path = env.cache.join("codex-session/.configs.toml.lock");
    std::fs::write(&lock_path, "").unwrap();
    TestEnv::touch_older(&lock_path);

    let output = env
        .cmd()
        .args(["--format", "json", "doctor"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    let check = find_check(&value, "session.trust-cache").unwrap();
    assert_eq!(check["status"], "warn");
    assert!(check["detail"].as_str().unwrap().contains("is stale"));
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
fn doctor_warns_on_nested_configs_profiles_dir() {
    let env = TestEnv::new();
    install_minimal_recipe(&env);
    let nested = env.configs_dir().join("profiles");
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::create_dir_all(env.profiles_dir()).unwrap();

    env.cmd()
        .arg("doctor")
        .assert()
        .success()
        .stdout(predicate::str::contains("obsolete nested"))
        .stdout(predicate::str::contains("move per-profile files to"))
        .stdout(predicate::str::contains("sibling of `configs/`"))
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

    // With no accounts, doctor reports auto-unset as passing. The assertions
    // below only concern session.root.
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
