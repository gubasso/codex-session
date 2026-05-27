#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

pub mod support;

use predicates::prelude::*;
use support::TestEnv;

#[test]
fn version_with_trailing_args_returns_ex_usage() {
    TestEnv::new()
        .cmd()
        .args(["version", "junk"])
        .assert()
        .code(64)
        .stdout("")
        .stderr(predicate::str::contains("unexpected argument 'junk'"));
}

#[test]
fn config_status_with_trailing_args_returns_ex_usage() {
    TestEnv::new()
        .cmd()
        .args(["config", "status", "extra"])
        .assert()
        .code(64)
        .stdout("")
        .stderr(predicate::str::contains("unexpected argument 'extra'"));
}

#[test]
fn config_recipe_show_with_trailing_args_returns_ex_usage() {
    TestEnv::new()
        .cmd()
        .args(["config-recipe", "show", "one", "extra"])
        .assert()
        .code(64)
        .stdout("")
        .stderr(predicate::str::contains("unexpected argument 'extra'"));
}

#[cfg(unix)]
#[test]
fn non_utf8_top_level_verb_returns_ex_usage() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt as _;

    let bad = OsStr::from_bytes(b"\xff\xfe");
    TestEnv::new()
        .cmd()
        .arg(bad)
        .assert()
        .code(64)
        .stdout("")
        .stderr(predicate::str::contains("invalid UTF-8"));
}
