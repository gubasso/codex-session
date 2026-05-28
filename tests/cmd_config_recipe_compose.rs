#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

mod support;

use std::os::unix::fs::PermissionsExt as _;

use support::TestEnv;

#[test]
fn config_recipe_compose_writes_session_artifacts_and_secure_mode() {
    let env = TestEnv::new();
    env.install_config_recipe(
        "default",
        "config-layers:\n  - base\n",
        &[(
            "base",
            "[model]\ndefault = \"gpt-5\"\n[env]\nHELLO = \"world\"\n",
        )],
    );

    env.cmd()
        .args(["config-recipe", "compose"])
        .assert()
        .success();

    let session_dir = env.session_dir();
    let config = std::fs::read_to_string(session_dir.join("config.toml")).unwrap();
    let sidecar = std::fs::read_to_string(session_dir.join(".codex-session-compose.json")).unwrap();
    let meta = std::fs::read_to_string(session_dir.join("session-meta.json")).unwrap();
    let mode = std::fs::metadata(&session_dir)
        .unwrap()
        .permissions()
        .mode()
        & 0o777;

    assert!(config.contains("gpt-5"));
    assert!(!config.contains("[env]"));
    assert!(sidecar.contains("\"HELLO\""));
    assert!(meta.contains("\"config-recipe\": \"default\""));
    assert_eq!(mode, 0o700);
}

#[test]
fn config_recipe_compose_lists_emitted_profile_siblings_in_cli_output() {
    let env = TestEnv::new();
    env.install_config_recipe(
        "default",
        "config-layers:\n  - base\nprofile-files:\n  - deep\n  - fast\n",
        &[("base", "model = \"gpt-5\"\n")],
    );
    env.write_profile_file("deep", "model = \"gpt-5\"\neffort = \"high\"\n");
    env.write_profile_file("fast", "model = \"gpt-5-mini\"\n");

    let out = env
        .cmd()
        .args(["config-recipe", "compose"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(out).unwrap();
    assert!(
        stdout.contains("profiles:"),
        "missing profiles header: {stdout}"
    );
    assert!(
        stdout.contains("deep:") && stdout.contains("deep.config.toml"),
        "deep sibling not listed in compose output: {stdout}"
    );
    assert!(
        stdout.contains("fast:") && stdout.contains("fast.config.toml"),
        "fast sibling not listed in compose output: {stdout}"
    );
}
