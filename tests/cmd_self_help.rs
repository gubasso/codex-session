#![allow(clippy::unwrap_used)]

pub mod support;

use support::TestEnv;

const HELP_TEXT: &str = include_str!("../src/ui/self_help.txt");

#[test]
fn self_help_prints_curated_help_text_exactly() {
    TestEnv::new()
        .cmd()
        .args(["self", "help"])
        .assert()
        .success()
        .stdout(HELP_TEXT);
}
