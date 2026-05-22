#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]
#![allow(clippy::literal_string_with_formatting_args)]
#![allow(missing_docs)]

//! Trust persistence: end-to-end round-trip.
//!
//! These tests install a fake `codex` that writes a `[projects.…]` entry
//! into its `$CODEX_HOME/config.toml` (matching upstream codex's behavior
//! per `docs/upstream-codex.md` F1/F2) and assert the wrapper's post-flight
//! sync lands the entry in the machine-local cache settings layer.

mod support;

use std::os::unix::fs::PermissionsExt as _;

use support::TestEnv;

/// Install a fake `codex` that appends a single `[projects."<path>"]` entry
/// to its `$CODEX_HOME/config.toml` and exits 0. Mirrors how upstream codex
/// persists a trust decision (per F1/F2/F7).
fn install_trust_writing_fake_codex(env: &TestEnv, project_path: &str, level: &str) {
    let codex = env.fake_bin.join("codex");
    let script = format!(
        r#"#!/usr/bin/env bash
set -eu
: "${{CODEX_HOME:?CODEX_HOME must be set by codex-session}}"
cat >> "$CODEX_HOME/config.toml" <<TRUSTENTRY

[projects."{project_path}"]
trust_level = "{level}"
TRUSTENTRY
exit 0
"#
    );
    std::fs::write(&codex, script).unwrap();
    let mut perms = std::fs::metadata(&codex).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&codex, perms).unwrap();
}

fn cache_projects_table(env: &TestEnv) -> Option<toml::Table> {
    let path = env.cache_settings_path();
    if !path.exists() {
        return None;
    }
    let text = std::fs::read_to_string(&path).unwrap();
    let parsed: toml::Table = toml::from_str(&text).unwrap();
    parsed.get("projects").and_then(|v| v.as_table().cloned())
}

#[test]
fn single_pass_persist_lands_trust_in_cache() {
    // Acceptance test for the round-trip: after one wrapper invocation
    // during which the (fake) codex writes a trust entry, the entry must
    // appear in the machine-local cache settings layer.
    let env = TestEnv::new();
    install_trust_writing_fake_codex(&env, "/tmp/example", "trusted");

    env.cmd().assert().success();

    let projects = cache_projects_table(&env).expect("cache settings should now exist");
    let entry = projects
        .get("/tmp/example")
        .and_then(toml::Value::as_table)
        .expect("/tmp/example entry");
    assert_eq!(
        entry.get("trust_level").and_then(toml::Value::as_str),
        Some("trusted")
    );

    // Hardened-write contract: cache file lands at 0o600.
    let mode = std::fs::metadata(env.cache_settings_path())
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600);
}

#[test]
fn untrusted_decision_also_persists() {
    // F4: codex writes `trust_level = "untrusted"` when the user declines.
    // We persist that verbatim so the user is not re-prompted for a path
    // they've explicitly rejected.
    let env = TestEnv::new();
    install_trust_writing_fake_codex(&env, "/tmp/declined", "untrusted");

    env.cmd().assert().success();

    let projects = cache_projects_table(&env).expect("cache file should exist");
    assert_eq!(
        projects
            .get("/tmp/declined")
            .and_then(toml::Value::as_table)
            .and_then(|t| t.get("trust_level"))
            .and_then(toml::Value::as_str),
        Some("untrusted")
    );
}

#[test]
fn persist_runs_even_when_codex_exits_non_zero() {
    // Lifecycle regression: the user may confirm trust at the top of a
    // session and then hit an unrelated codex failure. The post-flight
    // sync must still land the trust entry in the cache before the
    // wrapper propagates the non-zero exit code.
    let env = TestEnv::new();
    // Fake codex appends a trust entry, then exits with status 7 to
    // simulate "trust confirmed, later step failed".
    let codex = env.fake_bin.join("codex");
    let script = r#"#!/usr/bin/env bash
set -eu
: "${CODEX_HOME:?}"
cat >> "$CODEX_HOME/config.toml" <<TRUSTENTRY

[projects."/tmp/late-fail"]
trust_level = "trusted"
TRUSTENTRY
exit 7
"#;
    std::fs::write(&codex, script).unwrap();
    let mut perms = std::fs::metadata(&codex).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&codex, perms).unwrap();

    // Wrapper exit code is the child's (7), but the cache must still
    // contain the trust entry.
    env.cmd().assert().code(7);

    let projects =
        cache_projects_table(&env).expect("cache must be populated despite non-zero exit");
    assert_eq!(
        projects
            .get("/tmp/late-fail")
            .and_then(toml::Value::as_table)
            .and_then(|t| t.get("trust_level"))
            .and_then(toml::Value::as_str),
        Some("trusted"),
        "trust sync must run on the non-zero-exit codex path"
    );
}

#[test]
fn second_run_with_profile_observes_cached_trust_in_composed_config() {
    // Round-trip across two wrapper invocations: the first one persists a
    // trust entry into the cache layer, the second `compose()` must replay
    // it into the session config so codex sees the dir as already
    // classified.
    //
    // An active profile is required for the replay path: `compose()` is
    // the only producer that reads the cache layer
    // (`src/services/profile/mod.rs:38-48`). Stock mode is replay-blind by
    // design (no compose step). The write half still happens in stock mode
    // — see `single_pass_persist_lands_trust_in_cache` — but replay is
    // gated on profile activation. Document this in
    // `docs/upstream-codex.md` if/when the limitation matters to users.
    let env = TestEnv::new();
    env.install_profile(
        "default",
        "settings-layers:\n  - base\n",
        &[("base", "model = \"gpt-5\"\n")],
    );

    install_trust_writing_fake_codex(&env, "/tmp/example", "trusted");
    env.cmd().assert().success();

    // The second-run fake codex copies its session config into a known
    // path so the test can inspect it without guessing the PID-derived
    // session dir. This isolates the assertion from shell-side grep
    // quirks: we parse the captured TOML in Rust.
    let captured = env.tmp.path().join("second_run_session_config.toml");
    let codex = env.fake_bin.join("codex");
    let script = format!(
        r#"#!/usr/bin/env bash
set -eu
: "${{CODEX_HOME:?CODEX_HOME must be set}}"
cp "$CODEX_HOME/config.toml" "{captured}"
exit 0
"#,
        captured = captured.display(),
    );
    std::fs::write(&codex, script).unwrap();
    let mut perms = std::fs::metadata(&codex).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&codex, perms).unwrap();

    env.cmd().assert().success();
    let body = std::fs::read_to_string(&captured).expect("second run captured config");
    let parsed: toml::Table = toml::from_str(&body).expect("captured config parses");
    let projects = parsed
        .get("projects")
        .and_then(toml::Value::as_table)
        .expect("projects table replayed into session config");
    let entry = projects
        .get("/tmp/example")
        .and_then(toml::Value::as_table)
        .expect("/tmp/example entry replayed");
    assert_eq!(
        entry.get("trust_level").and_then(toml::Value::as_str),
        Some("trusted"),
        "second run must see the trust entry the first run wrote"
    );
}

#[test]
fn symlinked_cache_settings_does_not_fail_the_session() {
    // R5 / security: pre-create cache settings as a symlink. The hardened
    // writer must refuse to follow it; the trust-persist failure is
    // log-and-swallowed (matches upstream codex PR #17595 policy), so the
    // wrapper exit code stays 0 and the symlink's target is untouched.
    let env = TestEnv::new();
    install_trust_writing_fake_codex(&env, "/tmp/example", "trusted");

    let decoy = env.tmp.path().join("decoy.toml");
    std::fs::write(&decoy, "model = \"decoy\"\n").unwrap();
    std::fs::set_permissions(&decoy, std::fs::Permissions::from_mode(0o600)).unwrap();
    let cache_settings = env.cache_settings_path();
    std::fs::create_dir_all(cache_settings.parent().unwrap()).unwrap();
    std::fs::set_permissions(
        cache_settings.parent().unwrap(),
        std::fs::Permissions::from_mode(0o700),
    )
    .unwrap();
    std::os::unix::fs::symlink(&decoy, &cache_settings).unwrap();

    env.cmd().assert().success();

    // Decoy untouched.
    assert_eq!(
        std::fs::read_to_string(&decoy).unwrap(),
        "model = \"decoy\"\n"
    );
}

#[test]
fn concurrent_wrappers_serialize_writes_via_flock() {
    // Two parallel wrappers, each fake codex writes a distinct trust entry
    // to its own $CODEX_HOME. Both entries must end up in the shared
    // cache settings file — proves the flock prevents read-modify-write
    // clobbering across processes.
    let env = TestEnv::new();
    // The fake codex picks its key from $CST_TRUST_KEY (a wrapper-private
    // prefix avoids the CODEX_SESSION_* scrub).
    let codex = env.fake_bin.join("codex");
    let script = r#"#!/usr/bin/env bash
set -eu
: "${CODEX_HOME:?}"
: "${CST_TRUST_KEY:?}"
cat >> "$CODEX_HOME/config.toml" <<TRUSTENTRY

[projects."${CST_TRUST_KEY}"]
trust_level = "trusted"
TRUSTENTRY
exit 0
"#;
    std::fs::write(&codex, script).unwrap();
    let mut perms = std::fs::metadata(&codex).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&codex, perms).unwrap();

    std::thread::scope(|s| {
        let env = &env;
        for i in 0..6 {
            s.spawn(move || {
                env.cmd()
                    .env("CODEX_SESSION_GROUP", format!("trust-{i}"))
                    .env("CST_TRUST_KEY", format!("/p{i}"))
                    .assert()
                    .success();
            });
        }
    });

    let projects = cache_projects_table(&env).expect("cache must be populated");
    for i in 0..6 {
        assert!(
            projects.contains_key(&format!("/p{i}")),
            "missing /p{i} — flock did not protect concurrent writes"
        );
    }
}
