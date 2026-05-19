#![allow(clippy::unwrap_used)]

mod support;

use support::TestEnv;

#[test]
fn argv_translation_exec_foo_bar() {
    let env = TestEnv::new();
    let stub = support::fixture_path("echo-argv.sh");
    let output = env
        .cmd()
        .env("CODEX_SESSION_CHILD_BIN", &stub)
        .args(["exec", "foo", "--bar", "baz", "--", "--child-flag"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(output).unwrap();
    insta::assert_snapshot!("argv_translation_exec_foo_bar", stdout);
}
