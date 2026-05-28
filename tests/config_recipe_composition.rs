#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

mod support;

use predicates::prelude::*;
use support::TestEnv;

#[test]
fn config_recipe_compose_recurses_tables_and_replaces_arrays_scalars() {
    let env = TestEnv::new();
    env.install_config_recipe(
        "default",
        "config-layers:\n  - base\n  - work\n",
        &[
            (
                "base",
                "title = \"base\"\nitems = [1, 2]\n[model]\ndefault = \"gpt-5\"\n",
            ),
            (
                "work",
                "title = \"work\"\nitems = [3]\n[model]\neffort = \"high\"\n",
            ),
        ],
    );
    env.cmd()
        .args(["config-recipe", "compose"])
        .assert()
        .success();
    let config = std::fs::read_to_string(env.session_dir().join("config.toml")).unwrap();
    assert!(config.contains("title = \"work\""));
    assert!(config.contains("items = [3]"));
    assert!(config.contains("default = \"gpt-5\""));
    assert!(config.contains("effort = \"high\""));
}

#[test]
fn config_recipe_compose_extracts_env_from_output_config() {
    let env = TestEnv::new();
    env.install_config_recipe(
        "default",
        "config-layers:\n  - base\n",
        &[(
            "base",
            "[env]\nHELLO = \"world\"\n[model]\ndefault = \"gpt-5\"\n",
        )],
    );
    env.cmd()
        .args(["config-recipe", "compose"])
        .assert()
        .success();
    let config = std::fs::read_to_string(env.session_dir().join("config.toml")).unwrap();
    let sidecar =
        std::fs::read_to_string(env.session_dir().join(".codex-session-compose.json")).unwrap();
    assert!(!config.contains("[env]"));
    assert!(sidecar.contains("\"HELLO\""));
}

#[test]
fn config_recipe_compose_prepends_cache_layer() {
    let env = TestEnv::new();
    env.install_config_recipe(
        "default",
        "config-layers:\n  - base\n",
        &[("base", "[model]\ndefault = \"gpt-5\"\n")],
    );
    env.write_cache_config("[model]\neffort = \"high\"\n");
    env.cmd()
        .args(["config-recipe", "compose"])
        .assert()
        .success();
    let config = std::fs::read_to_string(env.session_dir().join("config.toml")).unwrap();
    assert!(config.contains("default = \"gpt-5\""));
    assert!(config.contains("effort = \"high\""));
}

#[test]
fn config_recipe_compose_preserves_machine_local_projects_table() {
    // Regression for the trust-persistence round-trip: a `[projects."<path>"]`
    // entry living in the machine-local cache layer must survive deep-merge
    // into the composed config alongside unrelated settings. Uses the
    // canonical fixtures `base.toml` + `target-with-local.toml` and asserts
    // structural equality with `expected-merged.toml`.
    let base = std::fs::read_to_string(support::fixture_path("base.toml")).unwrap();
    let local = std::fs::read_to_string(support::fixture_path("target-with-local.toml")).unwrap();
    let expected = std::fs::read_to_string(support::fixture_path("expected-merged.toml")).unwrap();

    let env = TestEnv::new();
    env.install_config_recipe("default", "config-layers:\n  - base\n", &[("base", &base)]);
    // The cache layer composes BEFORE the config-recipe layers, so the projects
    // table lives there to avoid clobbering by stow-managed sources.
    env.write_cache_config(&local);

    env.cmd()
        .args(["config-recipe", "compose"])
        .assert()
        .success();

    let actual = std::fs::read_to_string(env.session_dir().join("config.toml")).unwrap();
    let actual_table: toml::Table = toml::from_str(&actual).unwrap();
    let expected_table: toml::Table = toml::from_str(&expected).unwrap();
    assert_eq!(actual_table, expected_table);
}

#[test]
fn composer_emits_profile_sibling_files_one_to_one() {
    let env = TestEnv::new();
    env.install_config_recipe(
        "default",
        "config-layers:\n  - base\nprofile-files:\n  - deep\n  - fast\n",
        &[("base", "model = \"gpt-5\"\n")],
    );
    let deep = env.write_profile_file("deep", "model = \"gpt-5\"\neffort = \"high\"\n");
    let fast = env.write_profile_file("fast", "model = \"gpt-5-mini\"\n");

    env.cmd()
        .args(["config-recipe", "compose"])
        .assert()
        .success();

    assert_eq!(
        std::fs::read_to_string(env.session_dir().join("deep.config.toml")).unwrap(),
        std::fs::read_to_string(deep).unwrap()
    );
    assert_eq!(
        std::fs::read_to_string(env.session_dir().join("fast.config.toml")).unwrap(),
        std::fs::read_to_string(fast).unwrap()
    );

    let sidecar: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(env.session_dir().join(".codex-session-compose.json")).unwrap(),
    )
    .unwrap();
    let profiles = sidecar["profiles"].as_array().unwrap();
    assert_eq!(profiles.len(), 2);
    assert_eq!(profiles[0]["name"], "deep");
    assert_eq!(profiles[1]["name"], "fast");
}

#[test]
fn composer_emits_all_profile_files_when_manifest_omits_list() {
    let env = TestEnv::new();
    env.install_config_recipe(
        "default",
        "config-layers:\n  - base\n",
        &[("base", "model = \"gpt-5\"\n")],
    );
    env.write_profile_file("gamma", "model = \"gpt-5-gamma\"\n");
    env.write_profile_file("alpha", "model = \"gpt-5-alpha\"\n");
    env.write_profile_file("beta", "model = \"gpt-5-beta\"\n");

    env.cmd()
        .args(["config-recipe", "compose"])
        .assert()
        .success();

    assert!(env.session_dir().join("alpha.config.toml").is_file());
    assert!(env.session_dir().join("beta.config.toml").is_file());
    assert!(env.session_dir().join("gamma.config.toml").is_file());

    let sidecar: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(env.session_dir().join(".codex-session-compose.json")).unwrap(),
    )
    .unwrap();
    let names: Vec<_> = sidecar["profiles"]
        .as_array()
        .unwrap()
        .iter()
        .map(|profile| profile["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["alpha", "beta", "gamma"]);
}

#[test]
fn composer_rejects_legacy_profile_selector_in_input_layer() {
    let env = TestEnv::new();
    env.install_config_recipe(
        "default",
        "config-layers:\n  - base\n",
        &[("base", "profile = \"deep\"\nmodel = \"x\"\n")],
    );

    env.cmd()
        .args(["config-recipe", "compose"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "legacy profile syntax in config layer `base.toml`",
        ));
}

#[test]
fn composer_rejects_legacy_profiles_table_in_input_layer() {
    let env = TestEnv::new();
    env.install_config_recipe(
        "default",
        "config-layers:\n  - base\n",
        &[("base", "[profiles.deep]\nmodel = \"x\"\n")],
    );

    env.cmd()
        .args(["config-recipe", "compose"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "legacy profile syntax in config layer `base.toml`",
        ));
}

#[test]
fn composer_rejects_legacy_profile_header_in_profile_file() {
    let env = TestEnv::new();
    env.install_config_recipe(
        "default",
        "config-layers:\n  - base\nprofile-files:\n  - deep\n",
        &[("base", "model = \"gpt-5\"\n")],
    );
    env.write_profile_file("deep", "[profiles.deep]\nmodel = \"x\"\n");

    env.cmd()
        .args(["config-recipe", "compose"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "legacy profile syntax in profile file `deep.config.toml`",
        ));
}

#[test]
fn composer_rejects_missing_declared_profile_file() {
    let env = TestEnv::new();
    env.install_config_recipe(
        "default",
        "config-layers:\n  - base\nprofile-files:\n  - deep\n",
        &[("base", "model = \"gpt-5\"\n")],
    );
    // Intentionally do not write `deep.config.toml`.

    env.cmd()
        .args(["config-recipe", "compose"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "profile file `deep.config.toml` not found",
        ));
}

#[test]
fn composer_directory_scan_errors_on_filenames_with_invalid_layer_chars() {
    let env = TestEnv::new();
    env.install_config_recipe(
        "default",
        "config-layers:\n  - base\n",
        &[("base", "model = \"gpt-5\"\n")],
    );
    env.write_profile_file("deep", "model = \"gpt-5\"\n");
    // Stem "Deep Profile" contains a space, which `is_valid_layer_name`
    // forbids. The directory-scan contract is "emit every *.config.toml" —
    // silently dropping a file would let the operator see one set of
    // profiles while codex sees another, so the composer must fail loudly
    // with a clear pointer to the offending file.
    env.write_profile_file("Deep Profile", "model = \"gpt-5\"\n");

    env.cmd()
        .args(["config-recipe", "compose"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "profile file `Deep Profile.config.toml` has an invalid name",
        ));
}

#[test]
fn composer_emits_clean_base_config_when_profiles_present() {
    let env = TestEnv::new();
    env.install_config_recipe(
        "default",
        "config-layers:\n  - base\nprofile-files:\n  - deep\n  - fast\n",
        &[("base", "model = \"gpt-5\"\n")],
    );
    env.write_profile_file("deep", "model = \"gpt-5\"\neffort = \"high\"\n");
    env.write_profile_file("fast", "model = \"gpt-5-mini\"\n");

    env.cmd()
        .args(["config-recipe", "compose"])
        .assert()
        .success();

    let config = std::fs::read_to_string(env.session_dir().join("config.toml")).unwrap();
    let table: toml::Table = toml::from_str(&config).unwrap();
    assert!(!table.contains_key("profile"));
    assert!(!table.contains_key("profiles"));
    // Profile-file contents must not leak into the emitted base `config.toml`:
    // `deep.config.toml` contributes `effort = "high"`, `fast.config.toml`
    // contributes `model = "gpt-5-mini"`. The emitted base must reflect only
    // the base layer (`model = "gpt-5"`, no `effort`).
    assert!(
        !table.contains_key("effort"),
        "deep profile's `effort` key leaked into emitted base config.toml"
    );
    assert_eq!(
        table.get("model").and_then(toml::Value::as_str),
        Some("gpt-5"),
        "fast profile's `model` value leaked into emitted base config.toml"
    );
}

#[test]
fn composer_purges_stale_profile_sibling_files_between_runs() {
    // The session_dir under `accounts/<acct>/groups/<group>/` is persistent:
    // a second `compose` call must leave the emitted tree as a true mirror
    // of the current `configs/profiles/` snapshot, not the union of past
    // and current runs.
    let env = TestEnv::new();

    // Run 1: emit `deep.config.toml` and `fast.config.toml`.
    env.install_config_recipe(
        "default",
        "config-layers:\n  - base\nprofile-files:\n  - deep\n  - fast\n",
        &[("base", "model = \"gpt-5\"\n")],
    );
    env.write_profile_file("deep", "model = \"gpt-5\"\n");
    env.write_profile_file("fast", "model = \"gpt-5-mini\"\n");
    env.cmd()
        .args(["config-recipe", "compose"])
        .assert()
        .success();
    assert!(env.session_dir().join("deep.config.toml").is_file());
    assert!(env.session_dir().join("fast.config.toml").is_file());

    // Run 2: remove `fast` from the manifest. The stale sibling must be
    // purged so codex no longer sees it under $CODEX_HOME.
    env.install_config_recipe(
        "default",
        "config-layers:\n  - base\nprofile-files:\n  - deep\n",
        &[("base", "model = \"gpt-5\"\n")],
    );
    env.cmd()
        .args(["config-recipe", "compose"])
        .assert()
        .success();
    assert!(env.session_dir().join("deep.config.toml").is_file());
    assert!(
        !env.session_dir().join("fast.config.toml").exists(),
        "stale fast.config.toml from run 1 was not purged before run 2"
    );
    // The base config.toml itself must not be matched by the purge logic
    // (no leading segment before `.config.toml`).
    assert!(env.session_dir().join("config.toml").is_file());
}
