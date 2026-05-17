#![allow(clippy::unwrap_used)]

pub mod support;

use predicates::prelude::*;
use support::TestEnv;

#[test]
fn self_with_no_verb_prints_help_and_exits_zero() {
    TestEnv::new().cmd().arg("self").assert().success().stdout(
        predicate::str::starts_with("codex-session")
            .and(predicate::str::contains("self config-status")),
    );
}

#[test]
fn self_with_unknown_verb_prints_guidance_and_exits_2() {
    TestEnv::new()
        .cmd()
        .args(["self", "nope"])
        .assert()
        .code(2)
        .stdout("")
        .stderr("unknown self verb: nope\nrun: codex-session self help\n");
}
