#![allow(clippy::unwrap_used)]

pub mod support;

use support::TestEnv;

fn write_file(path: &std::path::Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

#[test]
fn explicit_config_replaces_user_and_project_layers() {
    let env = TestEnv::new();
    let project_dir = env.tmp.path().join("project");
    let explicit = env.tmp.path().join("explicit.toml");

    write_file(
        &env.wrapper_user_config_path(),
        "[paths]\ntarget_config = \"/tmp/user-target.toml\"\n",
    );
    write_file(
        &project_dir.join(".codex-session/config.toml"),
        "[paths]\ntarget_config = \"/tmp/project-target.toml\"\n",
    );
    write_file(
        &explicit,
        "[paths]\ntarget_config = \"/tmp/explicit-target.toml\"\n",
    );

    let output = env
        .cmd()
        .current_dir(&project_dir)
        .args([
            "--config",
            explicit.to_str().unwrap(),
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
    assert_eq!(value["target-path"], "/tmp/explicit-target.toml");
}

#[test]
fn env_layer_wins_over_project_and_user_layers() {
    let env = TestEnv::new();
    let project_dir = env.tmp.path().join("project");
    let child_dir =
        env.make_fake_codex_in_dir("env-child", "#!/usr/bin/env bash\nprintf 'child-from-env'");
    let child_bin = child_dir.join("codex");

    write_file(
        &env.wrapper_user_config_path(),
        "[child]\nbin = \"/tmp/user-codex\"\n[log]\nfile = \"/tmp/user.log\"\n",
    );
    write_file(
        &project_dir.join(".codex-session/config.toml"),
        "[child]\nbin = \"/tmp/project-codex\"\n[log]\nfile = \"/tmp/project.log\"\n",
    );

    let output = env
        .cmd()
        .current_dir(&project_dir)
        .env("CODEX_SESSION_CHILD_BIN", &child_bin)
        .env("CODEX_SESSION_LOG_FILE", "/tmp/env.log")
        .args(["config", "status", "--format", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(value["child-bin"], child_bin.to_string_lossy().as_ref());
    assert_eq!(value["log-file"], "/tmp/env.log");
}

#[test]
fn unknown_key_in_user_config_exits_seventy_eight() {
    let env = TestEnv::new();
    write_file(&env.wrapper_user_config_path(), "surprise = true\n");

    env.cmd().arg("help").assert().code(78);
}

#[test]
fn user_config_verbose_enables_logging_without_cli_flags() {
    let env = TestEnv::new();
    write_file(&env.wrapper_user_config_path(), "[log]\nverbose = 2\n");

    let output = env
        .cmd()
        .arg("help")
        .assert()
        .success()
        .get_output()
        .clone();
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("help"),
        "expected info log on stderr, got: {stderr}"
    );
}
