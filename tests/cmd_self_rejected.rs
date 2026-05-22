#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

pub mod support;

use support::TestEnv;

#[test]
fn self_subcommand_is_unknown() {
    let env = TestEnv::new();
    let out = env.cmd().arg("self").arg("version").output().unwrap();
    assert!(!out.status.success());
    assert_eq!(out.status.code(), Some(64), "EX_USAGE expected");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("unrecognized subcommand") || stderr.contains("error"));
}

#[test]
fn self_account_subcommand_is_unknown() {
    let env = TestEnv::new();
    let out = env
        .cmd()
        .arg("self")
        .arg("account")
        .arg("list")
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert_eq!(out.status.code(), Some(64), "EX_USAGE expected");
}
