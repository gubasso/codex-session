#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

pub mod support;

use support::TestEnv;

#[test]
fn double_dash_is_consumed_by_wrapper() {
    let env = TestEnv::new();
    env.make_fake_codex();
    env.cmd().args(["--", "--help", "foo"]).assert().success();
    assert_eq!(
        env.argv(),
        vec![String::from("--help"), String::from("foo")]
    );
}

#[test]
fn double_dash_before_external_verb_is_consumed() {
    let env = TestEnv::new();
    env.make_fake_codex();
    env.cmd().args(["--", "exec", "task"]).assert().success();
    assert_eq!(env.argv(), vec![String::from("exec"), String::from("task")]);
}
