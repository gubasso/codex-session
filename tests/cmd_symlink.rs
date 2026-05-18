#![allow(clippy::unwrap_used)]

pub mod support;

use support::TestEnv;

#[test]
fn symlinked_entrypoint_still_resolves_project_root_and_prints_version() {
    let env = TestEnv::new();
    let bin = assert_cmd::cargo::cargo_bin("codex-session");
    let link = env.tmp.path().join("linkbin/codex-session");
    std::fs::create_dir_all(link.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(&bin, &link).unwrap();

    let output = std::process::Command::new(&link)
        .env_clear()
        .env("HOME", &env.home)
        .env("XDG_CACHE_HOME", &env.cache)
        .env("PATH", "/usr/bin:/bin")
        .args(["self", "version"])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!(
            "codex-session {}\ncodex unavailable\n",
            env!("CARGO_PKG_VERSION")
        )
    );
}

#[test]
fn symlinked_launcher_still_prints_version() {
    let env = TestEnv::new();
    let bin = assert_cmd::cargo::cargo_bin("codex-session");
    let link = env.tmp.path().join("alt-linkbin/codex-session");
    std::fs::create_dir_all(link.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(&bin, &link).unwrap();

    let output = std::process::Command::new(&link)
        .env_clear()
        .env("HOME", &env.home)
        .env("XDG_CACHE_HOME", &env.cache)
        .env("PATH", "/usr/bin:/bin")
        .args(["self", "version"])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!(
            "codex-session {}\ncodex unavailable\n",
            env!("CARGO_PKG_VERSION")
        )
    );
}
