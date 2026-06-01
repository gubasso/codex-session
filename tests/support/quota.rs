use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use wiremock::{MockServer, Request, Respond, ResponseTemplate};

use super::TestEnv;

pub fn wham_url(server: &MockServer) -> String {
    format!("{}/backend-api/wham/usage", server.uri())
}

pub const fn oauth_auth() -> &'static str {
    r#"{"tokens":{"access_token":"test-token","account_id":"acct-123"}}"#
}

pub fn oauth_auth_for(account_id: &str) -> String {
    format!(r#"{{"tokens":{{"access_token":"test-token","account_id":"{account_id}"}}}}"#)
}

pub const fn default_payload() -> &'static str {
    r#"{
    "rate_limit": {
    "five_hour": { "percent_left": 73.4, "reset_time_ms": 1716393600000 },
    "weekly": { "percent_left": 87.1, "reset_time_ms": 1716998400000 }
    }
}"#
}

pub fn payload(five_hour: f64, weekly: f64) -> String {
    format!(
        r#"{{
    "rate_limit": {{
    "five_hour": {{ "percent_left": {five_hour}, "reset_time_ms": 1716393600000 }},
    "weekly": {{ "percent_left": {weekly}, "reset_time_ms": 1716998400000 }}
    }}
}}"#
    )
}

pub fn add_oauth_account(env: &TestEnv, name: &str) {
    env.seed_account(name, oauth_auth());
}

#[derive(Clone, Default)]
pub struct RequestRecorder {
    instants: Arc<Mutex<Vec<Instant>>>,
}

impl RequestRecorder {
    pub fn instants(&self) -> Vec<Instant> {
        let mut values = self.instants.lock().map_or_else(
            |poisoned| poisoned.into_inner().clone(),
            |guard| guard.clone(),
        );
        values.sort();
        values
    }

    fn record(&self, instant: Instant) {
        let mut guard = self
            .instants
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        guard.push(instant);
    }
}

pub struct RecordingDelayResponder {
    recorder: RequestRecorder,
    response: ResponseTemplate,
}

impl Respond for RecordingDelayResponder {
    fn respond(&self, _: &Request) -> ResponseTemplate {
        self.recorder.record(Instant::now());
        self.response.clone()
    }
}

pub fn delayed_quota_responder(delay: Duration) -> (RequestRecorder, RecordingDelayResponder) {
    let recorder = RequestRecorder::default();
    let response = ResponseTemplate::new(200)
        .set_body_raw(default_payload(), "application/json")
        .set_delay(delay);
    (
        recorder.clone(),
        RecordingDelayResponder { recorder, response },
    )
}
