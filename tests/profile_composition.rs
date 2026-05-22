#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

mod support;

use support::TestEnv;

#[test]
fn profile_compose_recurses_tables_and_replaces_arrays_scalars() {
    let env = TestEnv::new();
    env.install_profile(
        "default",
        "settings-layers:\n  - base\n  - work\n",
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
    env.cmd().args(["profile", "compose"]).assert().success();
    let config = std::fs::read_to_string(env.session_dir().join("config.toml")).unwrap();
    assert!(config.contains("title = \"work\""));
    assert!(config.contains("items = [3]"));
    assert!(config.contains("default = \"gpt-5\""));
    assert!(config.contains("effort = \"high\""));
}

#[test]
fn profile_compose_extracts_env_from_output_config() {
    let env = TestEnv::new();
    env.install_profile(
        "default",
        "settings-layers:\n  - base\n",
        &[(
            "base",
            "[env]\nHELLO = \"world\"\n[model]\ndefault = \"gpt-5\"\n",
        )],
    );
    env.cmd().args(["profile", "compose"]).assert().success();
    let config = std::fs::read_to_string(env.session_dir().join("config.toml")).unwrap();
    let sidecar =
        std::fs::read_to_string(env.session_dir().join(".codex-session-compose.json")).unwrap();
    assert!(!config.contains("[env]"));
    assert!(sidecar.contains("\"HELLO\""));
}

#[test]
fn profile_compose_prepends_cache_layer() {
    let env = TestEnv::new();
    env.install_profile(
        "default",
        "settings-layers:\n  - base\n",
        &[("base", "[model]\ndefault = \"gpt-5\"\n")],
    );
    env.write_cache_settings("[model]\neffort = \"high\"\n");
    env.cmd().args(["profile", "compose"]).assert().success();
    let config = std::fs::read_to_string(env.session_dir().join("config.toml")).unwrap();
    assert!(config.contains("default = \"gpt-5\""));
    assert!(config.contains("effort = \"high\""));
}

#[test]
fn profile_compose_preserves_machine_local_projects_table() {
    // Regression for the trust-persistence round-trip: a `[projects."<path>"]`
    // entry living in the machine-local cache layer must survive deep-merge
    // into the composed config alongside unrelated settings. Uses the
    // canonical fixtures `base.toml` + `target-with-local.toml` and asserts
    // structural equality with `expected-merged.toml`.
    let base = std::fs::read_to_string(support::fixture_path("base.toml")).unwrap();
    let local = std::fs::read_to_string(support::fixture_path("target-with-local.toml")).unwrap();
    let expected = std::fs::read_to_string(support::fixture_path("expected-merged.toml")).unwrap();

    let env = TestEnv::new();
    env.install_profile(
        "default",
        "settings-layers:\n  - base\n",
        &[("base", &base)],
    );
    // The cache layer composes BEFORE the profile layers, so the projects
    // table lives there to avoid clobbering by stow-managed sources.
    env.write_cache_settings(&local);

    env.cmd().args(["profile", "compose"]).assert().success();

    let actual = std::fs::read_to_string(env.session_dir().join("config.toml")).unwrap();
    let actual_table: toml::Table = toml::from_str(&actual).unwrap();
    let expected_table: toml::Table = toml::from_str(&expected).unwrap();
    assert_eq!(actual_table, expected_table);
}
