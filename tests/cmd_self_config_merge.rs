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
    env.cmd()
        .args(["self", "config-merge"])
        .assert()
        .code(66)
        .stdout("")
        .stderr(predicates::str::contains(
            "codex-session: failed to load base config",
        ))
        .stderr(predicates::str::contains(
            env.base_path().to_string_lossy().as_ref(),
        ));
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

#[cfg(unix)]
#[test]
fn force_merge_writes_target_with_umask_respecting_mode() {
    use std::os::unix::fs::PermissionsExt as _;

    let env = TestEnv::new();
    env.install_base();
    env.install_target_with_local();

    // Measure the active umask by writing a sentinel file the same way bash
    // would (default-mode file via `std::fs::write`, which goes through the
    // kernel umask). Anything the merged target ends up with must match this
    // exactly — that is what bash `cat > $tmp; mv $tmp $TARGET` does too.
    let probe = env.home.join(".codex").join("umask-probe");
    std::fs::write(&probe, b"").unwrap();
    let expected = std::fs::metadata(&probe).unwrap().permissions().mode() & 0o777;
    std::fs::remove_file(&probe).unwrap();

    env.cmd().args(["self", "config-merge"]).assert().success();

    let actual = std::fs::metadata(env.target_path())
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(
        actual, expected,
        "merged config mode must match the active umask (bash parity)"
    );
}

#[cfg(unix)]
#[test]
fn force_merge_creates_target_through_dangling_symlink_stamp() {
    // Bash `touch "$STAMP"` follows a symlink and creates the target if
    // it's missing. The Rust adapter must do the same; `create_new(true)`
    // would have failed with EEXIST on the symlink path itself.
    let env = TestEnv::new();
    env.install_base();
    env.install_target_with_local();
    let stamp = env.stamp_path();
    std::fs::create_dir_all(stamp.parent().unwrap()).unwrap();
    let referent = stamp.with_file_name("real-stamp-target");
    // Ensure neither side exists yet.
    let _ = std::fs::remove_file(&stamp);
    let _ = std::fs::remove_file(&referent);
    std::os::unix::fs::symlink(&referent, &stamp).unwrap();
    assert!(!referent.exists(), "referent must be absent at start");

    env.cmd().args(["self", "config-merge"]).assert().success();

    assert!(
        referent.exists(),
        "touch must follow the dangling symlink and create the referent"
    );
}

#[cfg(unix)]
#[test]
fn force_merge_succeeds_when_stamp_path_is_an_existing_directory() {
    // Bash `touch "$STAMP"` succeeds even when STAMP is a directory; it
    // only bumps the dir's mtime. The Rust adapter must not crash with
    // EISDIR on the same input.
    let env = TestEnv::new();
    env.install_base();
    env.install_target_with_local();
    std::fs::create_dir_all(env.stamp_path().parent().unwrap()).unwrap();
    std::fs::create_dir(env.stamp_path()).unwrap();

    env.cmd().args(["self", "config-merge"]).assert().success();
    assert!(
        env.stamp_path().is_dir(),
        "stamp path must remain a directory (bash parity)"
    );
}

#[cfg(unix)]
#[test]
fn force_merge_does_not_truncate_existing_stamp_file() {
    // Defensive: even though the production stamp file is always empty in
    // practice, `touch` must not zero an existing file. If a user (or a
    // future test fixture) writes a sentinel into the stamp, a subsequent
    // forced merge must preserve those bytes — that is bash `touch`
    // semantics, not shell `: > $f` truncation.
    let env = TestEnv::new();
    env.install_base();
    env.install_target_with_local();
    std::fs::create_dir_all(env.stamp_path().parent().unwrap()).unwrap();
    std::fs::write(env.stamp_path(), "do-not-truncate").unwrap();

    env.cmd().args(["self", "config-merge"]).assert().success();

    let after = std::fs::read_to_string(env.stamp_path()).unwrap();
    assert_eq!(
        after, "do-not-truncate",
        "touch() must not truncate the stamp file"
    );
}

#[test]
fn force_merge_advances_stamp_mtime() {
    let env = TestEnv::new();
    env.install_base();
    env.install_target_with_local();
    std::fs::create_dir_all(env.stamp_path().parent().unwrap()).unwrap();
    std::fs::write(env.stamp_path(), "").unwrap();
    env.touch_older(&env.stamp_path());
    let before = std::fs::metadata(env.stamp_path())
        .unwrap()
        .modified()
        .unwrap();
    env.cmd().args(["self", "config-merge"]).assert().success();
    let after = std::fs::metadata(env.stamp_path())
        .unwrap()
        .modified()
        .unwrap();
    assert!(
        after > before,
        "stamp mtime must advance after forced merge"
    );
}
