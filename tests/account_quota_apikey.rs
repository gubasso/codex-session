#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

mod support;

use wiremock::MockServer;

use support::TestEnv;

fn wham_url(server: &MockServer) -> String {
    format!("{}/backend-api/wham/usage", server.uri())
}

#[tokio::test]
async fn api_key_mode_skips_http_and_caches_for_five_minutes() {
    let env = TestEnv::new();
    env.write_native_auth("{\"token\":\"test\"}\n");
    env.cmd()
        .args(["account", "add", "scratch", "--from-current"])
        .assert()
        .success();
    env.write_account_auth_seed("scratch", r#"{"OPENAI_API_KEY":"sk-test"}"#);

    let server = MockServer::start().await;
    for _ in 0..2 {
        let output = env
            .cmd()
            .env("CODEX_SESSION_WHAM_USAGE_URL", wham_url(&server))
            .args([
                "account",
                "quota",
                "--account",
                "scratch",
                "--format",
                "json",
            ])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
        assert_eq!(value["mode"], "api-key");
        assert_eq!(value["ttl-secs"], 300);
    }

    assert_eq!(server.received_requests().await.unwrap().len(), 0);
}
