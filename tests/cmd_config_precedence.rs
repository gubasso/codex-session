#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

pub mod support;

use support::TestEnv;

fn write_file(path: &std::path::Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

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
    let env_log_dir = env.tmp.path().join("env-log-dir");

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
        .env("CODEX_SESSION_LOG_FILE", &env_log_dir)
        .args(["config", "status", "--format", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(value["child-bin"], child_bin.to_string_lossy().as_ref());
    assert_eq!(value["log-file"], env_log_dir.to_string_lossy().as_ref());
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

#[test]
fn log_stderr_format_precedence_file_env_cli() {
    // The user config sets `stderr_format = "pretty"`. The env var should
    // override the file. The CLI `--log-format json` should override both.
    // `config status --format json` reports the resolved `log-stderr-format`,
    // which lets us assert each layer wins in turn without having to parse
    // the actual stderr mirror output.
    let env = TestEnv::new();
    write_file(
        &env.wrapper_user_config_path(),
        "[log]\nstderr_format = \"pretty\"\n",
    );

    // File layer only.
    let file_only = env
        .cmd()
        .args(["config", "status", "--format", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: serde_json::Value = serde_json::from_slice(&file_only).unwrap();
    assert_eq!(value["log-stderr-format"], "pretty");

    // Env overrides file.
    let env_wins = env
        .cmd()
        .env("CODEX_SESSION_LOG_STDERR_FORMAT", "json")
        .args(["config", "status", "--format", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: serde_json::Value = serde_json::from_slice(&env_wins).unwrap();
    assert_eq!(value["log-stderr-format"], "json");

    // CLI `--log-format` overrides env and file.
    let cli_wins = env
        .cmd()
        .env("CODEX_SESSION_LOG_STDERR_FORMAT", "json")
        .args([
            "--log-format",
            "pretty",
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
    let value: serde_json::Value = serde_json::from_slice(&cli_wins).unwrap();
    assert_eq!(value["log-stderr-format"], "pretty");
}

#[test]
fn paths_state_dir_override_moves_default_log_directory() {
    // Regression guard for F1: `Config::defaults` must not freeze the log
    // destination at the XDG state dir. When `paths.state_dir` is overridden
    // (here via env) and `log.file` is unset, the resolved `log-file` should
    // follow the new state dir, not the XDG default.
    let env = TestEnv::new();
    let overridden_state = env.tmp.path().join("overridden-state");
    std::fs::create_dir_all(&overridden_state).unwrap();

    let output = env
        .cmd()
        .env("CODEX_SESSION_PATHS_STATE_DIR", &overridden_state)
        .args(["config", "status", "--format", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(
        value["log-file"],
        overridden_state.to_string_lossy().as_ref()
    );
}

#[test]
fn log_verbose_precedence_defaults_user_project_env_cli() {
    let env = TestEnv::new();
    let project_dir = env.tmp.path().join("project");

    let value = status_json(&mut env.cmd());
    assert_eq!(value["log-verbose"], 0);

    write_file(&env.wrapper_user_config_path(), "[log]\nverbose = 1\n");
    let value = status_json(&mut env.cmd());
    assert_eq!(value["log-verbose"], 1);

    write_file(
        &project_dir.join(".codex-session/config.toml"),
        "[log]\nverbose = 2\n",
    );
    let mut cmd = env.cmd();
    cmd.current_dir(&project_dir);
    let value = status_json(&mut cmd);
    assert_eq!(value["log-verbose"], 2);

    let mut cmd = env.cmd();
    cmd.current_dir(&project_dir)
        .env("CODEX_SESSION_LOG_VERBOSE", "3");
    let value = status_json(&mut cmd);
    assert_eq!(value["log-verbose"], 3);

    let mut cmd = env.cmd();
    cmd.current_dir(&project_dir)
        .env("CODEX_SESSION_LOG_VERBOSE", "3")
        .args(["-v"]);
    let value = status_json(&mut cmd);
    assert_eq!(value["log-verbose"], 1);

    let mut cmd = env.cmd();
    cmd.current_dir(&project_dir)
        .env("CODEX_SESSION_LOG_VERBOSE", "3")
        .args(["-vvvv"]);
    let value = status_json(&mut cmd);
    assert_eq!(value["log-verbose"], 4);
}

#[test]
fn log_mirror_stderr_precedence_defaults_user_project_env_cli() {
    let env = TestEnv::new();
    let project_dir = env.tmp.path().join("project");

    let value = status_json(&mut env.cmd());
    assert_eq!(value["log-mirror-stderr"], false);

    write_file(
        &env.wrapper_user_config_path(),
        "[log]\nmirror_stderr = true\n",
    );
    let value = status_json(&mut env.cmd());
    assert_eq!(value["log-mirror-stderr"], true);

    write_file(
        &project_dir.join(".codex-session/config.toml"),
        "[log]\nmirror_stderr = false\n",
    );
    let mut cmd = env.cmd();
    cmd.current_dir(&project_dir);
    let value = status_json(&mut cmd);
    assert_eq!(value["log-mirror-stderr"], false);

    let mut cmd = env.cmd();
    cmd.current_dir(&project_dir)
        .env("CODEX_SESSION_LOG_MIRROR_STDERR", "true");
    let value = status_json(&mut cmd);
    assert_eq!(value["log-mirror-stderr"], true);

    let mut cmd = env.cmd();
    cmd.current_dir(&project_dir)
        .env("CODEX_SESSION_LOG_MIRROR_STDERR", "false")
        .args(["--log-stderr"]);
    let value = status_json(&mut cmd);
    assert_eq!(value["log-mirror-stderr"], true);
}

#[test]
fn child_bin_precedence_defaults_user_project_env_and_explicit_config() {
    let env = TestEnv::new();
    let project_dir = env.tmp.path().join("project");
    let explicit = env.tmp.path().join("explicit.toml");
    let user_bin = env.make_fake_codex_in_dir("user-child", "#!/usr/bin/env bash\nprintf 'user'");
    let project_bin =
        env.make_fake_codex_in_dir("project-child", "#!/usr/bin/env bash\nprintf 'project'");
    let env_bin = env.make_fake_codex_in_dir("env-child", "#!/usr/bin/env bash\nprintf 'env'");
    let explicit_bin =
        env.make_fake_codex_in_dir("explicit-child", "#!/usr/bin/env bash\nprintf 'explicit'");
    let user_bin = user_bin.join("codex");
    let project_bin = project_bin.join("codex");
    let env_bin = env_bin.join("codex");
    let explicit_bin = explicit_bin.join("codex");

    let value = status_json(&mut env.cmd());
    assert!(value["child-bin"].is_null());

    write_file(
        &env.wrapper_user_config_path(),
        &format!("[child]\nbin = {:?}\n", user_bin.to_string_lossy()),
    );
    let value = status_json(&mut env.cmd());
    assert_eq!(value["child-bin"], user_bin.to_string_lossy().as_ref());

    write_file(
        &project_dir.join(".codex-session/config.toml"),
        &format!("[child]\nbin = {:?}\n", project_bin.to_string_lossy()),
    );
    let mut cmd = env.cmd();
    cmd.current_dir(&project_dir);
    let value = status_json(&mut cmd);
    assert_eq!(value["child-bin"], project_bin.to_string_lossy().as_ref());

    let mut cmd = env.cmd();
    cmd.current_dir(&project_dir)
        .env("CODEX_SESSION_CHILD_BIN", &env_bin);
    let value = status_json(&mut cmd);
    assert_eq!(value["child-bin"], env_bin.to_string_lossy().as_ref());

    write_file(
        &explicit,
        &format!("[child]\nbin = {:?}\n", explicit_bin.to_string_lossy()),
    );
    let mut cmd = env.cmd();
    cmd.current_dir(&project_dir)
        .args(["--config", explicit.to_str().unwrap()]);
    let value = status_json(&mut cmd);
    assert_eq!(value["child-bin"], explicit_bin.to_string_lossy().as_ref());
}
