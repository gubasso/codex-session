#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

mod support;

use predicates::prelude::*;
use support::TestEnv;

#[test]
fn account_id_accepts_valid_values() {
    let env = TestEnv::new();
    env.write_native_auth("{\"token\":\"test\"}\n");
    for value in ["work", "a", "0", "a-b_c-1"] {
        env.cmd()
            .args(["account", "add", value, "--from-current"])
            .assert()
            .success()
            .stdout(predicate::str::contains(format!("account added: {value}")));
    }
}

#[test]
fn account_id_rejects_invalid_values() {
    let env = TestEnv::new();
    env.write_native_auth("{\"token\":\"test\"}\n");
    for value in [
        "BAD",
        "-foo",
        "_foo",
        "foo/bar",
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    ] {
        env.cmd()
            .args(["account", "add", value, "--from-current"])
            .assert()
            .failure()
            .code(64);
    }
}
