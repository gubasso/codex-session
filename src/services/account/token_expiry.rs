#![allow(clippy::missing_errors_doc)]

use base64::Engine as _;
use serde_json::Value;

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TokenExpiry {
    ExpiresAt(u64),
    Missing,
    Malformed,
}

pub(crate) fn jwt_exp_unix(jwt: &str) -> Option<u64> {
    let mut parts = jwt.split('.');
    let _header = parts.next()?;
    let payload = parts.next()?;
    let _sig = parts.next()?;
    if parts.next().is_some() {
        return None;
    }

    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .ok()?;
    let value: Value = serde_json::from_slice(&decoded).ok()?;
    value.get("exp").and_then(Value::as_u64)
}

pub(crate) fn token_expiry_from_auth(auth: &Value) -> TokenExpiry {
    let Some(token) = auth
        .get("tokens")
        .and_then(|tokens| tokens.get("access_token"))
        .and_then(Value::as_str)
        .filter(|token| !token.is_empty())
    else {
        return TokenExpiry::Missing;
    };

    jwt_exp_unix(token).map_or(TokenExpiry::Malformed, TokenExpiry::ExpiresAt)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::{TokenExpiry, jwt_exp_unix, token_expiry_from_auth};

    #[test]
    fn parses_valid_jwt_exp() {
        let jwt = "eyJhbGciOiJub25lIn0.eyJleHAiOjE3MDAwMDAwMDB9.";
        assert_eq!(jwt_exp_unix(jwt), Some(1_700_000_000));
    }

    #[test]
    fn returns_none_for_malformed_jwt() {
        assert_eq!(jwt_exp_unix("not-a-jwt"), None);
    }

    #[test]
    fn token_expiry_from_auth_states() {
        let good: serde_json::Value = serde_json::json!({
            "tokens": {"access_token": "eyJhbGciOiJub25lIn0.eyJleHAiOjE3MDAwMDAwMDB9."}
        });
        assert!(matches!(
            token_expiry_from_auth(&good),
            TokenExpiry::ExpiresAt(ts) if ts == 1_700_000_000
        ));

        let missing: serde_json::Value = serde_json::json!({"tokens": {}});
        assert!(matches!(
            token_expiry_from_auth(&missing),
            TokenExpiry::Missing
        ));

        let bad: serde_json::Value = serde_json::json!({"tokens": {"access_token": "bad"}});
        assert!(matches!(
            token_expiry_from_auth(&bad),
            TokenExpiry::Malformed
        ));
    }
}
