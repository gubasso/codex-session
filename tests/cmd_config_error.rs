#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

pub mod support;

use std::path::Path;

use predicates::prelude::*;
use support::TestEnv;

fn write_file(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }

    std::fs::write(path, contents).unwrap();
}

#[test]
fn malformed_toml_in_user_config_exits_seventy_eight() {
    let env = TestEnv::new();
    let config_path = env.wrapper_user_config_path();
    write_file(&config_path, "[log]\nverbose = [\n");

    // `version` is the canonical "benign wrapper verb" that goes through
    // the dispatch path and forces config loading. `help` is owned by
    // clap (Tier 1) and short-circuits before the config layer.
    let assert = env.cmd().arg("version").assert().code(78);
    let stderr = String::from_utf8(assert.get_output().stderr.clone()).unwrap();
    let path = config_path.to_string_lossy();
    assert!(
        stderr.contains("config:"),
        "stderr must mention a config failure: {stderr}"
    );
    assert!(
        stderr.contains(path.as_ref()),
        "stderr must include the config path {path}: {stderr}"
    );
    if stderr.contains("line ") {
        assert!(
            stderr.contains("(line "),
            "stderr advertised a line but not in the rendered where-line shape: {stderr}"
        );
    }
}

#[test]
fn unknown_key_in_user_config_mentions_the_key() {
    let env = TestEnv::new();
    write_file(&env.wrapper_user_config_path(), "surprise = true\n");

    env.cmd()
        .arg("version")
        .assert()
        .code(78)
        .stderr(predicate::str::contains("surprise"));
}

#[test]
fn explicit_missing_config_exits_seventy_eight() {
    let env = TestEnv::new();
    let missing = env.tmp.path().join("missing.toml");

    env.cmd()
        .args(["--config", missing.to_str().unwrap(), "version"])
        .assert()
        .code(78)
        .stderr(predicate::str::contains(
            "explicit config file does not exist",
        ));
}
