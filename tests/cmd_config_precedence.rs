#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

pub mod support;

use support::TestEnv;

fn status_json(cmd: &mut assert_cmd::Command) -> serde_json::Value {
    let output = cmd
        .args(["config", "status", "--format", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    serde_json::from_slice(&output).unwrap()
}

#[test]
fn cli_profile_wins_over_env_file_and_default_manifest() {
    let env = TestEnv::new();
    std::fs::write(
        env.wrapper_user_config_path(),
        "[profile]\ndefault = \"from-file\"\n",
    )
    .unwrap();
    env.install_profile(
        "default",
        "settings-layers:\n  - default\n",
        &[("default", "")],
    );
    env.install_profile(
        "from-file",
        "settings-layers:\n  - from-file\n",
        &[("from-file", "")],
    );
    env.install_profile(
        "from-cli",
        "settings-layers:\n  - from-cli\n",
        &[("from-cli", "")],
    );

    let value = status_json(
        env.cmd()
            .env("CODEX_SESSION_PROFILE", "from-env")
            .args(["--profile", "from-cli"]),
    );
    assert_eq!(value["active-profile"], "from-cli");
}

#[test]
fn env_profile_wins_over_file_default() {
    let env = TestEnv::new();
    std::fs::write(
        env.wrapper_user_config_path(),
        "[profile]\ndefault = \"from-file\"\n",
    )
    .unwrap();
    env.install_profile("from-env", "settings-layers:\n  - base\n", &[("base", "")]);
    let value = status_json(env.cmd().env("CODEX_SESSION_PROFILE", "from-env"));
    assert_eq!(value["active-profile"], "from-env");
}

#[test]
fn file_default_wins_when_no_cli_or_env_override() {
    let env = TestEnv::new();
    std::fs::write(
        env.wrapper_user_config_path(),
        "[profile]\ndefault = \"from-file\"\n",
    )
    .unwrap();
    env.install_profile("from-file", "settings-layers:\n  - base\n", &[("base", "")]);
    let value = status_json(&mut env.cmd());
    assert_eq!(value["active-profile"], "from-file");
}

#[test]
fn default_manifest_is_used_when_no_other_profile_is_selected() {
    let env = TestEnv::new();
    env.install_profile("default", "settings-layers:\n  - base\n", &[("base", "")]);
    let value = status_json(&mut env.cmd());
    assert_eq!(value["active-profile"], "default");
}

#[test]
fn runtime_dir_env_override_updates_session_root() {
    let env = TestEnv::new();
    let override_root = env.tmp.path().join("runtime-override");
    std::fs::create_dir_all(&override_root).unwrap();
    let value = status_json(
        env.cmd()
            .env("CODEX_SESSION_PATHS_RUNTIME_DIR", &override_root),
    );
    assert_eq!(
        value["session-root"],
        override_root.to_string_lossy().as_ref()
    );
}

#[test]
fn env_child_bin_wins_over_user_file() {
    // User config sets `[child].bin` to a path that does not exist; the env
    // var must override and select a real binary. Regression test for the
    // `child`/`log` precedence chain — file → env → CLI — that pre-dates
    // Phase 12 and continues to be a supported wrapper contract.
    let env = TestEnv::new();
    let child_dir =
        env.make_fake_codex_in_dir("env-child", "#!/usr/bin/env bash\nprintf 'env-child'");
    let child_bin = child_dir.join("codex");
    std::fs::write(
        env.wrapper_user_config_path(),
        "[child]\nbin = \"/nonexistent/from-file\"\n",
    )
    .unwrap();

    let value = status_json(env.cmd().env("CODEX_SESSION_CHILD_BIN", &child_bin));
    assert_eq!(value["child-bin"], child_bin.to_string_lossy().as_ref());
}

#[test]
fn env_log_file_wins_over_user_file() {
    // Same precedence guarantee for `[log].file`: env var overrides the
    // user-file value.
    let env = TestEnv::new();
    let env_log = env.tmp.path().join("from-env.log");
    std::fs::write(
        env.wrapper_user_config_path(),
        "[log]\nfile = \"/tmp/from-file.log\"\n",
    )
    .unwrap();

    let value = status_json(env.cmd().env("CODEX_SESSION_LOG_FILE", &env_log));
    assert_eq!(value["log"]["file"], env_log.to_string_lossy().as_ref());
}

#[test]
fn profile_config_dir_redirects_profile_lookup() {
    // Setting `[profile].config_dir` in the user file must also re-root
    // `profiles_dir` / `settings_dir` (unless those are explicitly set), so
    // a profile installed under the alternate tree is discoverable.
    let env = TestEnv::new();
    let alt_root = env.tmp.path().join("alt-config");
    std::fs::create_dir_all(alt_root.join("profiles")).unwrap();
    std::fs::create_dir_all(alt_root.join("settings")).unwrap();
    std::fs::write(
        alt_root.join("profiles").join("alt.yaml"),
        "settings-layers:\n  - base\n",
    )
    .unwrap();
    std::fs::write(alt_root.join("settings").join("base.toml"), "").unwrap();
    std::fs::write(
        env.wrapper_user_config_path(),
        format!(
            "[profile]\ndefault = \"alt\"\nconfig_dir = \"{}\"\n",
            alt_root.display()
        ),
    )
    .unwrap();

    let value = status_json(&mut env.cmd());
    assert_eq!(value["active-profile"], "alt");
    let manifest_path = value["manifest-path"].as_str().unwrap();
    assert!(
        manifest_path.starts_with(alt_root.to_str().unwrap()),
        "manifest-path should resolve under alt config_dir; got {manifest_path}",
    );
}
