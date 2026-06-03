#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

mod support;

use insta::assert_snapshot;
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

fn normalize_stderr(env: &TestEnv, stderr: &str) -> String {
    let normalized = stderr.replace(&env.tmp.path().display().to_string(), "[TMP]");
    let age_pattern =
        regex::Regex::new(r"\b\d+d \d+h ago\b|\b\d+h \d+m ago\b|\b\d+m \d+s ago\b|\b\d+ s ago\b")
            .unwrap();
    age_pattern.replace_all(&normalized, "[AGE]").into_owned()
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
fn resume_owner_account_deleted_reports_not_found_not_auth_missing() {
    // A thread-index hit whose account has been removed from the registry must
    // surface `NotFound` (exit 78) with a real remediation, not `AuthMissing`
    // (exit 75) hinting `account refresh <name>` for a nonexistent account.
    let env = TestEnv::new();
    // Note: no `seed_account` for "ghost" — the index points at a missing account.
    let thread_id = "thread-deleted-owner";
    write_thread_index_entry(&env, thread_id, "ghost", "stable-test");

    let invoked = env.tmp.path().join("deleted-owner-should-not-run");
    let child_dir = env.make_fake_codex_in_dir(
        "resume-deleted-owner-assert-not-invoked",
        &format!(
            "#!/usr/bin/env bash\ntouch '{}'\nprintf 'should not run\\n' >&2\nexit 99\n",
            invoked.display()
        ),
    );

    env.cmd()
        .env("CODEX_SESSION_GROUP", "stable-test")
        .env("CODEX_SESSION_CHILD_BIN", child_dir.join("codex"))
        .args(["exec", "resume", thread_id])
        .assert()
        .failure()
        .code(78);

    assert!(
        !invoked.exists(),
        "child must not be invoked when the owner account is gone"
    );
}

#[test]
fn recovers_owner_from_rollout_scan_when_thread_index_misses() {
    let env = TestEnv::new();
    env.seed_account("work", "{\"token\":\"work\"}\n");
    let fixture = fixture_path("fake-codex-resume.sh");
    let thread_id = "thread-miss";

    write_rollout_for(&env, "work", "stable-test", thread_id);

    let assert = env
        .cmd()
        .env("CODEX_SESSION_GROUP", "stable-test")
        .env("CODEX_SESSION_CHILD_BIN", &fixture)
        .args(["exec", "resume", thread_id])
        .assert()
        .success()
        .stdout(predicate::str::contains(format!("resumed:{thread_id}")));
    let stderr = String::from_utf8(assert.get_output().stderr.clone()).unwrap();
    assert!(stderr.contains("warning: thread index had no entry"));
    assert!(stderr.contains("recovered owner 'work'"));
    assert!(!stderr.contains("auto-selection enabled"));
}

#[test]
fn owner_missing_returns_classified_error_with_recent_candidates() {
    let env = TestEnv::new();
    env.seed_account("work", "{\"token\":\"work\"}\n");
    let thread_id = "thread-missing";

    write_thread_index_entry(&env, "known-1", "work", "stable-test");
    write_thread_index_entry(&env, "known-2", "default", "other-group");

    let invoked = env.tmp.path().join("resume-owner-missing-should-not-run");
    let child_dir = env.make_fake_codex_in_dir(
        "resume-owner-missing-assert-not-invoked",
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
        "resume child should not spawn on missing owner"
    );
    let stderr = String::from_utf8(assert.get_output().stderr.clone()).unwrap();
    assert!(stderr.contains("account: no rollout for thread thread-missing"));
    assert!(
        stderr.contains("not found in the thread index or any registered account's rollout store")
    );
    assert!(stderr.contains("recent threads:"));
    assert!(stderr.contains("known-2 (default"));
    assert!(!stderr.contains("auto-selection enabled"));
    assert_snapshot!(
        "resume_owner_missing_stderr",
        normalize_stderr(&env, &stderr)
    );
}

#[test]
fn resume_last_miss_returns_index_empty_for_current_group() {
    let env = TestEnv::new();

    env.cmd()
        .env("CODEX_SESSION_GROUP", "stable-test")
        .env(
            "CODEX_SESSION_CHILD_BIN",
            fixture_path("fake-codex-resume.sh"),
        )
        .args(["exec", "resume", "--last"])
        .assert()
        .failure()
        .code(75)
        .stderr(predicate::str::contains(
            "account: no recorded threads to resume",
        ));
}

#[test]
fn pinned_resume_missing_rollout_is_classified() {
    let env = TestEnv::new();
    env.seed_account("work", "{\"token\":\"work\"}\n");
    let thread_id = "thread-no-rollout";
    write_thread_index_entry(&env, thread_id, "work", "stable-test");

    let assert = env
        .cmd()
        .env("CODEX_SESSION_GROUP", "stable-test")
        .env(
            "CODEX_SESSION_CHILD_BIN",
            fixture_path("fake-codex-resume.sh"),
        )
        .args(["exec", "resume", thread_id])
        .assert()
        .failure()
        .code(75);

    let stderr = String::from_utf8(assert.get_output().stderr.clone()).unwrap();
    assert!(stderr.contains("account: resume failed for thread thread-no-rollout"));
    assert!(stderr.contains("no longer has the rollout (absent or deleted)"));
}

#[test]
fn pinned_resume_sandbox_mismatch_is_classified() {
    let env = TestEnv::new();
    env.seed_account("work", "{\"token\":\"work\"}\n");
    let thread_id = "thread-sandbox-mismatch";
    write_thread_index_entry(&env, thread_id, "work", "stable-test");
    write_rollout_for(&env, "work", "stable-test", thread_id);

    let err_json = format!(
        "{{\"type\":\"turn.failed\",\"message\":\"thread/resume failed: no rollout \
        found for thread id {thread_id} (code -32600)\",\"error\":{{\"error_code\":\
        \"invalid_request\",\"http_status_code\":400}}}}"
    );
    let script = format!(
        "#!/usr/bin/env bash\nif [ \"${{1:-}}\" = \"--version\" ]; then\n  \
        exit 0\nfi\nprintf '{}\\n'\nexit 2\n",
        err_json
    );
    let child_dir = env.make_fake_codex_in_dir("resume-no-rollout-structured", &script);

    let assert = env
        .cmd()
        .env("CODEX_SESSION_GROUP", "stable-test")
        .env("CODEX_SESSION_CHILD_BIN", child_dir.join("codex"))
        .args(["exec", "resume", thread_id, "--json"])
        .assert()
        .failure()
        .code(75);

    let stderr = String::from_utf8(assert.get_output().stderr.clone()).unwrap();
    assert!(stderr.contains("account: resume failed for thread thread-sandbox-mismatch"));
    assert!(stderr.contains("still has the rollout locally"));
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
        .env(
            "CODEX_SESSION_CHILD_BIN",
            fixture_path("fake-429-usage-jsonl.sh"),
        )
        .args(["exec", "resume", thread_id])
        .assert()
        .failure()
        .code(75);

    let stderr = String::from_utf8(assert.get_output().stderr.clone()).unwrap();
    assert!(stderr.contains("marker:codex-session-fake-429-usage"));
    assert!(stderr.contains("account: resume blocked"));
    assert!(stderr.contains("• work  rate limited (429)"));
    assert!(
        env.named_account_root("work")
            .join("cooldown.json")
            .exists()
    );
    let cooldown: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(env.named_account_root("work").join("cooldown.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(cooldown["reset_source"], "retry-after");
}

#[test]
fn resume_unhandled_error_surfaces_styled_message_without_cooldown() {
    // The resume path is account-bound and never rotates, but an unhandled
    // codex error (context-window/server/unclassified) must still get the same
    // styled stderr + log-pointer UX that `run_auto` emits — not a silent
    // pass-through of codex's raw output. It must not write a cooldown and must
    // preserve codex's own exit code.
    let env = TestEnv::new();
    env.seed_account("work", "{\"token\":\"work\"}\n");
    let thread_id = "thread-live-unhandled";

    write_thread_index_entry(&env, thread_id, "work", "stable-test");
    write_rollout_for(&env, "work", "stable-test", thread_id);

    let assert = env
        .cmd()
        .env("CODEX_SESSION_GROUP", "stable-test")
        .env(
            "CODEX_SESSION_CHILD_BIN",
            fixture_path("fake-unhandled-jsonl.sh"),
        )
        .args(["exec", "resume", thread_id])
        .assert()
        .failure()
        .code(1);

    let stderr = String::from_utf8(assert.get_output().stderr.clone()).unwrap();
    assert!(stderr.contains("marker:codex-session-fake-unhandled"));
    assert!(stderr.contains("codex returned an unhandled error"));
    assert!(stderr.contains("context-window-exceeded: context window exceeded for request"));
    assert!(stderr.contains("codex-session.log"));
    assert!(!stderr.contains("account: resume blocked"));
    assert!(
        !env.named_account_root("work")
            .join("cooldown.json")
            .exists(),
        "unhandled resume errors must not write a cooldown"
    );
}
