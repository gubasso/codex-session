#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

mod support;

use support::TestEnv;

#[test]
fn account_id_accepts_valid_values() {
    let env = TestEnv::new();
    for value in ["work", "a", "0", "a-b_c-1"] {
        env.seed_account(value, "{\"token\":\"test\"}\n");
        assert!(env.named_account_root(value).is_dir());
    }
}

#[test]
fn account_id_rejects_invalid_values() {
    let env = TestEnv::new();
    for value in [
        "BAD",
        "-foo",
        "_foo",
        "foo/bar",
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    ] {
        env.cmd()
            .args(["account", "add", value])
            .assert()
            .failure()
            .code(64);
    }
}
