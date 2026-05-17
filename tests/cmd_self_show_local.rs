#![allow(clippy::unwrap_used)]

pub mod support;

use support::TestEnv;

#[test]
fn self_show_local_prints_only_local_project_sections() {
    let env = TestEnv::new();
    env.install_base();
    env.install_target_with_local();
    let expected = "[projects.\"/tmp/example\"]\ntrust_level = \"trusted\"\n\n\
                    [projects.\"/tmp/other\"]\ntrust_level = \"untrusted\"\n";
    env.cmd()
        .args(["self", "show-local"])
        .assert()
        .success()
        .stdout(expected);
}
