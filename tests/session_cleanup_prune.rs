#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

mod support;

use support::TestEnv;

#[test]
fn session_cleanup_prune() {
    let env = TestEnv::new();
    env.make_fake_codex_printing_stdout("OK");

    let sessions_root = env.session_root().join("sessions");
    let fresh = sessions_root.join("fresh");
    let old = sessions_root.join("old");
    let ancient = sessions_root.join("ancient");
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
