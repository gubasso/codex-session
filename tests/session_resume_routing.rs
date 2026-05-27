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
