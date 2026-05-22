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
}

impl<'a> SessionMeta<'a> {
    pub(crate) fn new(profile: Option<&'a str>, group_id: &'a str, cwd: &'a Utf8Path) -> Self {
        Self {
            profile,
            group_id,
            cwd,
            started_at: started_at_now(),
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
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or_else(
            |_| "1970-01-01T00:00:00Z".to_owned(),
            |duration| format_unix_seconds(duration.as_secs()),
        )
}

fn format_unix_seconds(seconds: u64) -> String {
    let days = i64::try_from(seconds / 86_400).unwrap_or(i64::MAX);
    let seconds_of_day = seconds % 86_400;
    let (year, month, day) = civil_from_days(days);
    let hour = seconds_of_day / 3_600;
    let minute = (seconds_of_day % 3_600) / 60;
    let second = seconds_of_day % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

fn civil_from_days(days_since_epoch: i64) -> (i32, u32, u32) {
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    let year = year + i64::from(month <= 2);

    (
        i32::try_from(year).unwrap_or(i32::MAX),
        u32::try_from(month).unwrap_or(u32::MAX),
        u32::try_from(day).unwrap_or(u32::MAX),
    )
}
