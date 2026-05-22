#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

mod support;

use support::TestEnv;

#[test]
fn prune_stale_sessions_under_new_accounts_tree() {
    let env = TestEnv::new();
    env.make_fake_codex_printing_stdout("OK");

    let groups_root = env.groups_root();
    let fresh = groups_root.join("fresh");
    let old = groups_root.join("old");
    let ancient = groups_root.join("ancient");
    std::fs::create_dir_all(&fresh).unwrap();
    std::fs::create_dir_all(&old).unwrap();
    std::fs::create_dir_all(&ancient).unwrap();

    let now = std::time::SystemTime::now();
    let one_day = now - std::time::Duration::from_secs(24 * 3600);
    let eight_days = now - std::time::Duration::from_secs(8 * 24 * 3600);
    let thirty_days = now - std::time::Duration::from_secs(30 * 24 * 3600);
    filetime::set_file_mtime(&fresh, filetime::FileTime::from_system_time(one_day)).unwrap();
    filetime::set_file_mtime(&old, filetime::FileTime::from_system_time(eight_days)).unwrap();
    filetime::set_file_mtime(&ancient, filetime::FileTime::from_system_time(thirty_days)).unwrap();

    env.cmd().arg("exec").assert().success().stdout("OK");

    assert!(fresh.exists(), "fresh session directory should remain");
    assert!(
        !old.exists(),
        "8-day-old session directory should be pruned"
    );
    assert!(
        !ancient.exists(),
        "30-day-old session directory should be pruned"
    );
}

#[test]
fn prune_legacy_pid_dirs_runs_once() {
    let env = TestEnv::new();
    env.make_fake_codex_printing_stdout("OK");

    let runtime_legacy = env.legacy_runtime_sessions_root().join("pid-old");
    let state_legacy = env.legacy_state_sessions_root().join("pid-old");
    std::fs::create_dir_all(&runtime_legacy).unwrap();
    std::fs::create_dir_all(&state_legacy).unwrap();
    let old = std::time::SystemTime::now() - std::time::Duration::from_secs(48 * 3600);
    filetime::set_file_mtime(&runtime_legacy, filetime::FileTime::from_system_time(old)).unwrap();
    filetime::set_file_mtime(&state_legacy, filetime::FileTime::from_system_time(old)).unwrap();

    env.cmd().arg("exec").assert().success().stdout("OK");

    assert!(!runtime_legacy.exists());
    assert!(!state_legacy.exists());
    assert!(
        env.state_session_root()
            .join("state/.legacy-pruned")
            .exists()
    );

    let second = env
        .legacy_state_sessions_root()
        .join("pid-fresh-but-second-run");
    std::fs::create_dir_all(&second).unwrap();
    filetime::set_file_mtime(&second, filetime::FileTime::from_system_time(old)).unwrap();

    env.cmd().arg("exec").assert().success().stdout("OK");

    assert!(second.exists(), "legacy prune should run only once");
}
