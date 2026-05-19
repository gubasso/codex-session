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
