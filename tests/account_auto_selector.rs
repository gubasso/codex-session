#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

mod support;

use predicates::prelude::*;

use support::TestEnv;

fn quota_cache(five_hour: f64, weekly: f64) -> String {
    format!(
        r#"{{
    "fetched_at_unix": 4102444800,
    "ttl_secs": 30,
    "body": {{
    "kind": "ok",
    "five_hour": {{ "percent_left": {five_hour}, "reset_at_unix": 4102448400 }},
    "weekly": {{ "percent_left": {weekly}, "reset_at_unix": 4103053200 }}
    }}
}}"#
    )
}

#[test]
fn auto_exec_picks_highest_scoring_account() {
    let env = TestEnv::new();
    env.seed_account("high", "{\"token\":\"test\"}\n");
    env.seed_account("low", "{\"token\":\"test\"}\n");
    env.write_quota_cache("high", &quota_cache(90.0, 90.0));
    env.write_quota_cache("low", &quota_cache(55.0, 55.0));

    let out = env.tmp.path().join("selected.txt");
    let child_dir = env.make_fake_codex_in_dir(
        "record-codex-home",
        &format!(
            "#!/usr/bin/env bash\nprintf '%s' \"$CODEX_HOME\" > '{}'\nexit 0\n",
            out.display()
        ),
    );

    env.cmd()
        .env("CODEX_SESSION_CHILD_BIN", child_dir.join("codex"))
        .args(["--account", "auto", "exec"])
        .assert()
        .success();

    let selected = std::fs::read_to_string(out).unwrap();
    assert!(selected.contains("accounts/high/groups"));
}

#[test]
fn auto_exec_returns_tempfail_when_all_are_below_threshold() {
    let env = TestEnv::new_empty();
    env.seed_account("low1", "{\"token\":\"test\"}\n");
    env.seed_account("low2", "{\"token\":\"test\"}\n");
    env.write_quota_cache("low1", &quota_cache(40.0, 90.0));
    env.write_quota_cache("low2", &quota_cache(90.0, 5.0));

    env.make_fake_codex();
    env.cmd()
        .args(["--account", "auto", "exec"])
        .assert()
        .failure()
        .code(75)
        .stderr(predicate::str::contains("auto-selection exhausted"))
        .stderr(predicate::str::contains("below quota threshold"));
}
