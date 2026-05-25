#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

mod support;

use predicates::prelude::*;
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, ResponseTemplate};

use support::TestEnv;

fn probe_url(server: &MockServer) -> String {
    format!("{}/backend-api/me", server.uri())
}

const fn oauth_auth() -> &'static str {
    r#"{"tokens":{"access_token":"test-token","account_id":"acct-123"}}"#
}

/// After `codex logout`, the token is revoked server-side but the local
/// seed file is unchanged. The gate must probe the token against the
/// server and detect the 401, reporting `AuthMissing` instead of
/// silently passing through with a revoked token.
#[tokio::test]
async fn gate_blocks_when_token_revoked() {
    let env = TestEnv::new_empty();
    env.seed_account("work", oauth_auth());

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(401).set_body_string(
            r#"{"error":{"message":"token_invalidated","code":"token_invalidated"}}"#,
        ))
        .mount(&server)
        .await;

    let child_dir = env.make_fake_codex_in_dir("never-reached", "#!/usr/bin/env bash\nexit 0\n");
    let child_bin = child_dir.join("codex");

    env.cmd()
        .env("CODEX_SESSION_CHILD_BIN", &child_bin)
        .env("CODEX_SESSION_AUTH_PROBE_URL", probe_url(&server))
        .arg("exec")
        .write_stdin("")
        .assert()
        .failure()
        .stderr(predicate::str::contains("no valid authentication"));
}

/// When the server accepts the token, the gate should pass through.
#[tokio::test]
async fn gate_passes_when_token_valid() {
    let env = TestEnv::new_empty();
    env.seed_account("work", oauth_auth());

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_string("{}"))
        .mount(&server)
        .await;

    let child_dir = env.make_fake_codex_in_dir("ok-child", "#!/usr/bin/env bash\nexit 0\n");
    let child_bin = child_dir.join("codex");

    env.cmd()
        .env("CODEX_SESSION_CHILD_BIN", &child_bin)
        .env("CODEX_SESSION_AUTH_PROBE_URL", probe_url(&server))
        .arg("exec")
        .assert()
        .success();
}

/// When the probe server is unreachable the gate should fail-open
/// (proceed with launch) rather than blocking.
#[test]
fn gate_passes_when_probe_unreachable() {
    let env = TestEnv::new();

    let child_dir = env.make_fake_codex_in_dir("ok-child", "#!/usr/bin/env bash\nexit 0\n");
    let child_bin = child_dir.join("codex");

    env.cmd()
        .env("CODEX_SESSION_CHILD_BIN", &child_bin)
        .env("CODEX_SESSION_AUTH_PROBE_URL", "http://127.0.0.1:1")
        .arg("exec")
        .assert()
        .success();
}
