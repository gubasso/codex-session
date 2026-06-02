#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

mod support;

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

fn cooldown_json(reset_at_unix: u64) -> String {
    serde_json::json!({
        "reset_at_unix": reset_at_unix,
        "reason": "429 detected",
        "last_429_at_unix": reset_at_unix - 60,
        "snippet_truncated": "HTTP 429 Too Many Requests"
    })
    .to_string()
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
fn auto_exec_selects_best_below_knee_account() {
    let env = TestEnv::new_empty();
    env.seed_account("low1", "{\"token\":\"test\"}\n");
    env.seed_account("low2", "{\"token\":\"test\"}\n");
    env.write_quota_cache("low1", &quota_cache(40.0, 90.0));
    env.write_quota_cache("low2", &quota_cache(45.0, 90.0));
    std::fs::write(env.last_account_path(), "").unwrap();

    let out = env.tmp.path().join("selected-below-knee.txt");
    let child_dir = env.make_fake_codex_in_dir(
        "record-below-knee-codex-home",
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
    assert!(selected.contains("accounts/low2/groups"));
}

#[test]
fn auto_exec_no_eligible_reports_cooldown_etas() {
    let env = TestEnv::new_empty();
    env.seed_account("cool1", "{\"token\":\"test\"}\n");
    env.seed_account("cool2", "{\"token\":\"test\"}\n");
    std::fs::write(
        env.named_account_root("cool1").join("cooldown.json"),
        cooldown_json(4_102_448_400),
    )
    .unwrap();
    std::fs::write(
        env.named_account_root("cool2").join("cooldown.json"),
        cooldown_json(4_102_449_400),
    )
    .unwrap();

    let invoked = env.tmp.path().join("should-not-run");
    let child_dir = env.make_fake_codex_in_dir(
        "assert-not-invoked",
        &format!(
            "#!/usr/bin/env bash\ntouch '{}'\nprintf 'should not run\\n' >&2\nexit 99\n",
            invoked.display()
        ),
    );

    let assert = env
        .cmd()
        .env("CODEX_SESSION_CHILD_BIN", child_dir.join("codex"))
        .args(["--account", "auto", "exec"])
        .assert()
        .failure()
        .code(75);
    assert!(
        !invoked.exists(),
        "child should not run when no account is usable"
    );
    let stderr = String::from_utf8(assert.get_output().stderr.clone()).unwrap();
    assert!(stderr.contains("account: no eligible account"));
    assert!(stderr.contains("no usable account is available"));
    assert!(stderr.contains("• cool1  cooldown active  back in"));
    assert!(stderr.contains("earliest available:"));
    assert!(stderr.contains("codex-session account cooldown clear --all"));
}
