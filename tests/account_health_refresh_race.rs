#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

mod support;

use std::path::PathBuf;

use support::TestEnv;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const EXPIRED_JWT: &str = "eyJhbGciOiJub25lIn0.eyJleHAiOjE3MDAwMDAwMDB9.";
const FUTURE_JWT: &str = "eyJhbGciOiJub25lIn0.eyJleHAiOjQxMDI0NDQ4MDB9.";

fn token_url(server: &MockServer) -> String {
    format!("{}/oauth/token", server.uri())
}

fn oauth_auth(access_token: &str, refresh_token: &str) -> String {
    serde_json::json!({
        "tokens": {
            "access_token": access_token,
            "refresh_token": refresh_token,
            "account_id": "acct-1",
            "plan": "pro",
        }
    })
    .to_string()
}

fn refresh_success(access_token: &str, refresh_token: &str) -> String {
    serde_json::json!({
        "access_token": access_token,
        "refresh_token": refresh_token,
        "token_type": "Bearer",
        "expires_in": 864_000,
    })
    .to_string()
}

fn install_default_recipe_with_ping(env: &TestEnv) {
    std::fs::write(
        env.wrapper_user_config_path(),
        "[config-recipe]\ndefault = \"default\"\n",
    )
    .unwrap();
    env.install_config_recipe(
        "default",
        "config-layers:\n  - base\nprofile-files:\n  - ping\n",
        &[("base", "model = \"gpt-5.4\"\n")],
    );
    env.write_profile_file(
        "ping",
        "model = \"gpt-5.4-mini\"\nmodel_reasoning_effort = \"minimal\"\n",
    );
}

fn install_probe_success_child(env: &TestEnv, dir_name: &str) -> PathBuf {
    let script = [
        r#"#!/usr/bin/env bash
if [ "${"#,
        r#"1:-}" = "--version" ]; then
    printf '%s\n' 'codex 0.134.0'
    exit 0
fi
exit 0
"#,
    ]
    .concat();
    let dir = env.make_fake_codex_in_dir(dir_name, &script);
    dir.join("codex")
}

fn install_probe_rotation_child(env: &TestEnv) -> PathBuf {
    let dir = env.make_fake_codex_in_dir(
        "probe-rotation-child",
        &format!(
            r#"#!/usr/bin/env bash
if [ "${{1:-}}" = "--version" ]; then
    printf '%s\n' 'codex 0.134.0'
    exit 0
fi
cat > "$CODEX_HOME/auth.json" << 'EOF'
{{"tokens":{{"access_token":"{FUTURE_JWT}","refresh_token":"rt","account_id":"a1"}}}}
EOF
chmod 600 "$CODEX_HOME/auth.json"
exit 0
"#,
        ),
    );
    dir.join("codex")
}

async fn mount_wham(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/backend-api/wham/usage"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw(support::quota::default_payload(), "application/json"),
        )
        .mount(server)
        .await;
}

async fn mount_token_refresh(server: &MockServer, refresh_token: &str) {
    Mock::given(method("POST"))
        .and(path("/oauth/token"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            refresh_success(FUTURE_JWT, refresh_token),
            "application/json",
        ))
        .mount(server)
        .await;
}

async fn token_request_count(server: &MockServer) -> usize {
    server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|request| request.url.path() == "/oauth/token")
        .count()
}

fn run_health(env: &TestEnv, server: &MockServer, child: &PathBuf) -> serde_json::Value {
    let output = env
        .cmd()
        .env("CODEX_SESSION_CHILD_BIN", child)
        .env(
            "CODEX_SESSION_WHAM_USAGE_URL",
            support::quota::wham_url(server),
        )
        .env("CODEX_SESSION_TOKEN_ENDPOINT", token_url(server))
        .args(["account", "health", "--account", "work", "--format", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    serde_json::from_slice(&output).unwrap()
}

fn assert_health_token_ok(value: &serde_json::Value) {
    let entries = value.as_array().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["account"], "work");
    assert_eq!(entries[0]["status"], "live");
    assert_eq!(entries[0]["token"], "ok");
}

#[tokio::test]
async fn expired_token_refreshes_exactly_once() {
    let env = TestEnv::new_empty();
    env.seed_account("work", &oauth_auth(EXPIRED_JWT, "old-rt"));
    install_default_recipe_with_ping(&env);
    let child = install_probe_success_child(&env, "probe-success-once");

    let server = MockServer::start().await;
    mount_wham(&server).await;
    mount_token_refresh(&server, "new-rt").await;

    let value = run_health(&env, &server, &child);
    assert_health_token_ok(&value);
    assert_eq!(token_request_count(&server).await, 1);
}

#[tokio::test]
async fn expired_token_persists_rotation_to_seed() {
    let env = TestEnv::new_empty();
    env.seed_account("work", &oauth_auth(EXPIRED_JWT, "old-rt"));
    install_default_recipe_with_ping(&env);
    let child = install_probe_success_child(&env, "probe-success-persist");

    let server = MockServer::start().await;
    mount_wham(&server).await;
    mount_token_refresh(&server, "new-rt").await;

    let value = run_health(&env, &server, &child);
    assert_health_token_ok(&value);

    let seed = std::fs::read_to_string(env.named_account_auth_seed("work")).unwrap();
    assert!(
        seed.contains("new-rt"),
        "seed should have the rotated refresh_token, got: {seed}"
    );
    assert!(
        !seed.contains("old-rt"),
        "seed should not retain the original refresh_token, got: {seed}"
    );
}

#[tokio::test]
async fn expired_token_then_quota_succeeds() {
    let env = TestEnv::new_empty();
    env.seed_account("work", &oauth_auth(EXPIRED_JWT, "old-rt"));
    install_default_recipe_with_ping(&env);
    let child = install_probe_success_child(&env, "probe-success-quota");

    let server = MockServer::start().await;
    mount_wham(&server).await;
    mount_token_refresh(&server, "new-rt").await;

    let value = run_health(&env, &server, &child);
    assert_health_token_ok(&value);

    env.cmd()
        .env(
            "CODEX_SESSION_WHAM_USAGE_URL",
            support::quota::wham_url(&server),
        )
        .env("CODEX_SESSION_TOKEN_ENDPOINT", token_url(&server))
        .args(["account", "quota", "--account", "work", "--format", "json"])
        .assert()
        .success();
}

#[tokio::test]
async fn live_token_does_not_refresh() {
    let env = TestEnv::new_empty();
    let original = oauth_auth(FUTURE_JWT, "live-rt");
    env.seed_account("work", &original);
    install_default_recipe_with_ping(&env);
    let child = install_probe_success_child(&env, "probe-success-live");

    let server = MockServer::start().await;
    mount_wham(&server).await;
    mount_token_refresh(&server, "unused-rt").await;

    let value = run_health(&env, &server, &child);
    assert_health_token_ok(&value);
    assert_eq!(token_request_count(&server).await, 0);

    let seed = std::fs::read_to_string(env.named_account_auth_seed("work")).unwrap();
    assert_eq!(seed, original);
}

#[tokio::test]
async fn expired_seed_with_existing_group_updates_quota_resolved_group_without_second_refresh() {
    let env = TestEnv::new_empty();
    let original = oauth_auth(EXPIRED_JWT, "old-rt");
    env.seed_account("work", &original);
    env.write_group_auth("work", "healthgroup", &original);
    install_default_recipe_with_ping(&env);
    let child = install_probe_success_child(&env, "probe-success-group");

    let server = MockServer::start().await;
    mount_wham(&server).await;
    mount_token_refresh(&server, "new-rt").await;

    let output = env
        .cmd()
        .env("CODEX_SESSION_CHILD_BIN", child)
        .env("CODEX_SESSION_GROUP", "healthgroup")
        .env(
            "CODEX_SESSION_WHAM_USAGE_URL",
            support::quota::wham_url(&server),
        )
        .env("CODEX_SESSION_TOKEN_ENDPOINT", token_url(&server))
        .args(["account", "health", "--account", "work", "--format", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_health_token_ok(&value);
    assert_eq!(token_request_count(&server).await, 1);

    let group_auth =
        std::fs::read_to_string(env.named_group_dir("work", "healthgroup").join("auth.json"))
            .unwrap();
    assert!(
        group_auth.contains("new-rt"),
        "group auth should receive rotated bytes, got: {group_auth}"
    );
}

#[tokio::test]
async fn probe_rotation_is_persisted_to_auth_source() {
    let env = TestEnv::new_empty();
    env.seed_account("work", &oauth_auth(FUTURE_JWT, "old-rt"));
    install_default_recipe_with_ping(&env);
    let child = install_probe_rotation_child(&env);

    let server = MockServer::start().await;
    mount_wham(&server).await;
    mount_token_refresh(&server, "unused-rt").await;

    let value = run_health(&env, &server, &child);
    assert_health_token_ok(&value);
    assert_eq!(token_request_count(&server).await, 0);

    let seed = std::fs::read_to_string(env.named_account_auth_seed("work")).unwrap();
    assert!(
        seed.contains("probe-rt"),
        "probe-rotated token should be persisted to seed, got: {seed}"
    );
}
