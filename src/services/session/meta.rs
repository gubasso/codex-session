//! Session metadata writing.
//!
//! What this is: serializable metadata for a composed wrapper session.
//! What this is not: profile composition or group-id resolution.

#![allow(clippy::result_large_err)]

use camino::Utf8Path;
use serde::Serialize;
use std::io::Write as _;

#[derive(Debug, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct SessionMeta<'a> {
    pub(crate) profile: Option<&'a str>,
    pub(crate) group_id: &'a str,
    pub(crate) cwd: &'a Utf8Path,
    pub(crate) started_at: String,
    pub(crate) account: &'a str,
    pub(crate) account_source: &'a str,
}

impl<'a> SessionMeta<'a> {
    pub(crate) fn new(
        profile: Option<&'a str>,
        group_id: &'a str,
        cwd: &'a Utf8Path,
        account: &'a str,
        account_source: &'a str,
    ) -> Self {
        Self {
            profile,
            group_id,
            cwd,
            started_at: started_at_now(),
            account,
            account_source,
        }
    }
}

pub(crate) fn write(
    session_dir: &Utf8Path,
    meta: &SessionMeta<'_>,
) -> Result<(), crate::config::ConfigError> {
    let path = session_dir.join("session-meta.json");
    let body = serde_json::to_string_pretty(meta).map_err(|err| {
        crate::config::ConfigError::MergeFailed {
            reason: err.to_string(),
        }
    })?;
    let Some(parent) = path.parent() else {
        return Err(crate::config::ConfigError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "session-meta.json has no parent",
        )));
    };
    std::fs::create_dir_all(parent.as_std_path())?;
    let mut temp = tempfile::Builder::new()
        .prefix(".codex-session-meta.")
        .tempfile_in(parent.as_std_path())?;
    temp.write_all(body.as_bytes())?;
    temp.persist(path.as_std_path())
        .map_err(|err| crate::config::ConfigError::Io(err.error))?;
    Ok(())
}

fn started_at_now() -> String {
    super::time_util::utc_now_rfc3339()
}
