#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

mod support;

use predicates::prelude::*;
use support::TestEnv;
use support::fixture_path;

fn write_thread_index_entry(env: &TestEnv, thread_id: &str, account: &str, group_id: &str) {
    use std::io::Write;
    let path = env.state_session_root().join("thread-index.jsonl");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    let cwd = env.tmp.path().to_string_lossy();
    let entry = serde_json::json!({
        "thread-id": thread_id,
        "account": account,
        "group-id": group_id,
        "cwd": cwd.as_ref(),
        "created-at": "2026-05-22T00:00:00Z"
    });
    let line = format!("{}\n", serde_json::to_string(&entry).unwrap());
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .unwrap();
    write!(file, "{line}").unwrap();
}

fn write_rollout_for(env: &TestEnv, account: &str, group: &str, thread_id: &str) {
    let rollout_dir = env
        .named_group_dir(account, group)
        .join("sessions/2026/05/22");
    std::fs::create_dir_all(&rollout_dir).unwrap();
    std::fs::write(
        rollout_dir.join(format!("rollout-{thread_id}.jsonl")),
        "{}\n",
    )
    .unwrap();
}

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
fn routes_exec_resume_by_explicit_id_from_thread_index() {
    let env = TestEnv::new();
    env.seed_account("work", "{\"token\":\"work\"}\n");
    let fixture = fixture_path("fake-codex-resume.sh");
    let thread_id = "thread-explicit";

    write_thread_index_entry(&env, thread_id, "work", "stable-test");
    write_rollout_for(&env, "work", "stable-test", thread_id);

    env.cmd()
        .env("CODEX_SESSION_GROUP", "stable-test")
        .env("CODEX_SESSION_CHILD_BIN", &fixture)
        .args(["exec", "resume", thread_id])
        .assert()
        .success()
        .stdout(predicate::str::contains(format!("resumed:{thread_id}")));
}

#[test]
fn routes_bare_resume_by_id_from_thread_index() {
    let env = TestEnv::new();
    env.seed_account("work", "{\"token\":\"work\"}\n");
    let fixture = fixture_path("fake-codex-resume.sh");
    let thread_id = "thread-bare";

    write_thread_index_entry(&env, thread_id, "work", "stable-test");
    write_rollout_for(&env, "work", "stable-test", thread_id);

    env.cmd()
        .env("CODEX_SESSION_GROUP", "stable-test")
        .env("CODEX_SESSION_CHILD_BIN", &fixture)
        .args(["resume", thread_id])
        .assert()
        .success()
        .stdout(predicate::str::contains(format!("resumed:{thread_id}")));
}

#[test]
fn routes_exec_resume_last_via_thread_index() {
    let env = TestEnv::new();
    env.seed_account("work", "{\"token\":\"work\"}\n");
    let fixture = fixture_path("fake-codex-resume.sh");
    let thread_id = "thread-last";

    write_thread_index_entry(&env, thread_id, "work", "stable-test");
    write_rollout_for(&env, "work", "stable-test", thread_id);

    env.cmd()
        .env("CODEX_SESSION_GROUP", "stable-test")
        .env("CODEX_SESSION_CHILD_BIN", &fixture)
        .args(["exec", "resume", "--last"])
        .assert()
        .success()
        .stdout(predicate::str::contains(format!("resumed:{thread_id}")));
}

#[test]
fn routes_exec_resume_last_all_groups_via_thread_index() {
    let env = TestEnv::new();
    env.seed_account("work", "{\"token\":\"work\"}\n");
    let fixture = fixture_path("fake-codex-resume.sh");
    let thread_id = "thread-allgroups";

    write_thread_index_entry(&env, "thread-older", "work", "other-group");
    write_thread_index_entry(&env, thread_id, "work", "stable-test");
    write_rollout_for(&env, "work", "stable-test", thread_id);

    env.cmd()
        .env("CODEX_SESSION_GROUP", "stable-test")
        .env("CODEX_SESSION_CHILD_BIN", &fixture)
        .args(["exec", "resume", "--last", "--all-groups"])
        .assert()
        .success()
        .stdout(predicate::str::contains(format!("resumed:{thread_id}")));
}

#[test]
fn routes_exec_resume_by_id_from_original_group_when_current_group_differs() {
    let env = TestEnv::new();
    env.seed_account("work", "{\"token\":\"work\"}\n");
    let fixture = fixture_path("fake-codex-resume.sh");
    let thread_id = "thread-cross-group";

    write_thread_index_entry(&env, thread_id, "work", "original-group");
    write_rollout_for(&env, "work", "original-group", thread_id);

    env.cmd()
        .env("CODEX_SESSION_GROUP", "different-group")
        .env("CODEX_SESSION_CHILD_BIN", &fixture)
        .args(["exec", "resume", thread_id])
        .assert()
        .success()
        .stdout(predicate::str::contains(format!("resumed:{thread_id}")));
}

#[test]
fn routes_bare_resume_from_original_group_when_current_group_differs() {
    let env = TestEnv::new();
    env.seed_account("work", "{\"token\":\"work\"}\n");
    let fixture = fixture_path("fake-codex-resume.sh");
    let thread_id = "thread-cross-bare";

    write_thread_index_entry(&env, thread_id, "work", "original-group");
    write_rollout_for(&env, "work", "original-group", thread_id);

    env.cmd()
        .env("CODEX_SESSION_GROUP", "different-group")
        .env("CODEX_SESSION_CHILD_BIN", &fixture)
        .args(["resume", thread_id])
        .assert()
        .success()
        .stdout(predicate::str::contains(format!("resumed:{thread_id}")));
}

#[test]
fn falls_back_to_normal_resolution_when_thread_index_misses() {
    let env = TestEnv::new();
    let fixture = fixture_path("fake-codex-resume.sh");
    let thread_id = "thread-miss";

    write_rollout_for(&env, "default", "stable-test", thread_id);

    env.cmd()
        .env("CODEX_SESSION_GROUP", "stable-test")
        .env("CODEX_SESSION_CHILD_BIN", &fixture)
        .args(["exec", "resume", thread_id])
        .assert()
        .success()
        .stdout(predicate::str::contains(format!("resumed:{thread_id}")));
}

#[test]
fn resume_blocked_preflight_cooldown_does_not_spawn_child() {
    let env = TestEnv::new();
    env.seed_account("work", "{\"token\":\"work\"}\n");
    env.seed_account("personal", "{\"token\":\"personal\"}\n");
    let thread_id = "thread-cooldown-blocked";

    write_thread_index_entry(&env, thread_id, "work", "stable-test");
    write_rollout_for(&env, "work", "stable-test", thread_id);
    std::fs::write(
        env.named_account_root("work").join("cooldown.json"),
        cooldown_json(4_102_448_400),
    )
    .unwrap();

    let invoked = env.tmp.path().join("resume-preflight-should-not-run");
    let child_dir = env.make_fake_codex_in_dir(
        "resume-preflight-assert-not-invoked",
        &format!(
            "#!/usr/bin/env bash\ntouch '{}'\nprintf 'should not run\\n' >&2\nexit 99\n",
            invoked.display()
        ),
    );

    let assert = env
        .cmd()
        .env("CODEX_SESSION_GROUP", "stable-test")
        .env("CODEX_SESSION_CHILD_BIN", child_dir.join("codex"))
        .args(["exec", "resume", thread_id])
        .assert()
        .failure()
        .code(75);

    assert!(
        !invoked.exists(),
        "resume child should not spawn when owner is blocked"
    );
    let stderr = String::from_utf8(assert.get_output().stderr.clone()).unwrap();
    assert!(stderr.contains("account: resume blocked"));
    assert!(stderr.contains("thread `thread-cooldown-blocked` belongs to account `work`"));
    assert!(stderr.contains("• work  cooldown active  back in"));
    assert!(stderr.contains("other accounts cannot continue this thread"));
    assert!(stderr.contains("codex-session account cooldown clear --all"));
}

#[test]
fn resume_blocked_preflight_zero_quota_does_not_suggest_cooldown_clear() {
    let env = TestEnv::new();
    env.seed_account("work", "{\"token\":\"work\"}\n");
    env.seed_account("personal", "{\"token\":\"personal\"}\n");
    env.write_quota_cache("work", &quota_cache(0.0, 80.0));
    env.write_quota_cache("personal", &quota_cache(80.0, 80.0));
    let thread_id = "thread-quota-blocked";

    write_thread_index_entry(&env, thread_id, "work", "stable-test");
    write_rollout_for(&env, "work", "stable-test", thread_id);

    let invoked = env.tmp.path().join("resume-quota-should-not-run");
    let child_dir = env.make_fake_codex_in_dir(
        "resume-quota-assert-not-invoked",
        &format!(
            "#!/usr/bin/env bash\ntouch '{}'\nprintf 'should not run\\n' >&2\nexit 99\n",
            invoked.display()
        ),
    );

    let assert = env
        .cmd()
        .env("CODEX_SESSION_GROUP", "stable-test")
        .env("CODEX_SESSION_CHILD_BIN", child_dir.join("codex"))
        .args(["exec", "resume", thread_id])
        .assert()
        .failure()
        .code(75);

    assert!(
        !invoked.exists(),
        "resume child should not spawn when owner quota is exhausted"
    );
    let stderr = String::from_utf8(assert.get_output().stderr.clone()).unwrap();
    assert!(stderr.contains("account: resume blocked"));
    assert!(stderr.contains("• work  five-hour quota exhausted"));
    assert!(stderr.contains("5h 0.0% left"));
    assert!(stderr.contains("earliest available:"));
    assert!(!stderr.contains("codex-session account cooldown clear --all"));
    // A healthy alternate account must surface the "available for a new thread"
    // guidance the ResumeBlocked message exists to give — not a misleading
    // "not attempted  back in — (—)" line.
    assert!(stderr.contains("• personal  available for a new thread"));
    assert!(!stderr.contains("not attempted"));
    assert!(!stderr.contains("back in — (—)"));
}

#[test]
fn resume_live_429_converts_to_resume_blocked() {
    let env = TestEnv::new();
    env.seed_account("work", "{\"token\":\"work\"}\n");
    let thread_id = "thread-live-429";

    write_thread_index_entry(&env, thread_id, "work", "stable-test");
    write_rollout_for(&env, "work", "stable-test", thread_id);

    let assert = env
        .cmd()
        .env("CODEX_SESSION_GROUP", "stable-test")
        .env("CODEX_SESSION_CHILD_BIN", fixture_path("fake-429.sh"))
        .args(["exec", "resume", thread_id])
        .assert()
        .failure()
        .code(75);

    let stderr = String::from_utf8(assert.get_output().stderr.clone()).unwrap();
    assert!(stderr.contains("marker:codex-session-fake-429"));
    assert!(stderr.contains("account: resume blocked"));
    assert!(stderr.contains("• work  rate limited (429)"));
    assert!(
        env.named_account_root("work")
            .join("cooldown.json")
            .exists()
    );
}
