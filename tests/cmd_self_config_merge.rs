#![allow(clippy::unwrap_used)]

pub mod support;

use support::TestEnv;

#[test]
fn self_config_merge_forces_rewrite_when_stamp_is_fresh() {
    let env = TestEnv::new();
    env.install_base();
    env.install_target_with_local();
    std::fs::create_dir_all(env.stamp_path().parent().unwrap()).unwrap();
    std::fs::write(env.stamp_path(), "").unwrap();
    env.touch_older(&env.base_path());
    env.touch_newer(&env.stamp_path());

    let expected_stdout = format!(
        "merged: {} -> {}\n",
        env.base_path().display(),
        env.target_path().display()
    );
    env.cmd()
        .args(["self", "config-merge"])
        .assert()
        .success()
        .stdout(expected_stdout);

    let got = std::fs::read_to_string(env.target_path()).unwrap();
    let want = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/expected-merged.toml"),
    )
    .unwrap();
    assert_eq!(got, want);
}

#[test]
fn self_config_merge_without_base_errors_exactly() {
    let env = TestEnv::new();
    let expected = format!(
        "ERROR: base config not found at {}\n",
        env.base_path().display()
    );
    env.cmd()
        .args(["self", "config-merge"])
        .assert()
        .code(1)
        .stdout("")
        .stderr(expected);
}

#[test]
fn merge_replaces_target_atomically_with_no_intermediate_state() {
    let env = TestEnv::new();
    let large_base = format!("# base\npayload = \"{}\"\n", "x".repeat(1024 * 1024));
    std::fs::write(env.base_path(), large_base).unwrap();
    std::fs::write(
        env.target_path(),
        "[projects.\"/tmp/example\"]\ntrust_level = \"trusted\"\n",
    )
    .unwrap();

    env.cmd()
        .args(["self", "config-merge"])
        .assert()
        .success()
        .stdout(format!(
            "merged: {} -> {}\n",
            env.base_path().display(),
            env.target_path().display()
        ));

    let codex_dir = env.home.join(".codex");
    let leftovers: Vec<_> = std::fs::read_dir(&codex_dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.contains(".tmp."))
        .collect();
    assert!(leftovers.is_empty(), "leftover temp files: {leftovers:?}");
}
