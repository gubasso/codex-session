//! Group-id derivation: stable per-terminal/per-parent session identity.
//!
//! What this is: the 5-step resolution chain (flag -> env -> tty -> ppid ->
//! pid) and the visible warning on pid-N fallback.
//! What this is not: session-dir creation; see `session::dir::session_dir`.

#![allow(clippy::result_large_err)]

use std::str::FromStr;

use crate::error::AppError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GroupId(String);

impl GroupId {
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    const fn from_unchecked(value: String) -> Self {
        Self(value)
    }
}

impl FromStr for GroupId {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        validate_group_id(value)?;
        Ok(Self(value.to_owned()))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum GroupIdSource {
    Flag,
    Env,
    Tty,
    Ppid,
    Pid,
}

#[derive(Debug, Clone)]
pub(crate) struct ResolvedGroupId {
    pub(crate) id: GroupId,
    pub(crate) source: GroupIdSource,
}

pub(crate) fn current(
    ctx: &crate::context::AppContext,
) -> Result<ResolvedGroupId, crate::error::AppError> {
    resolve_from_inputs(
        ctx.global.group.clone(),
        std::env::var("CODEX_SESSION_GROUP").ok(),
        ctx,
    )
}

fn resolve_from_inputs(
    flag: Option<GroupId>,
    env: Option<String>,
    ctx: &crate::context::AppContext,
) -> Result<ResolvedGroupId, AppError> {
    if let Some(group_id) = flag {
        return Ok(ResolvedGroupId {
            id: group_id,
            source: GroupIdSource::Flag,
        });
    }

    if let Some(value) = env {
        let id = GroupId::from_str(&value).map_err(|reason| {
            clap::Error::raw(
                clap::error::ErrorKind::InvalidValue,
                format!("CODEX_SESSION_GROUP={value} rejected: {reason}"),
            )
        })?;
        return Ok(ResolvedGroupId {
            id,
            source: GroupIdSource::Env,
        });
    }

    if let Some(id) = current_from_tty() {
        return Ok(ResolvedGroupId {
            id,
            source: GroupIdSource::Tty,
        });
    }

    if let Some(id) = current_from_ppid() {
        return Ok(ResolvedGroupId {
            id,
            source: GroupIdSource::Ppid,
        });
    }

    let pid = std::process::id();
    let warning = format!(
        "warning: codex-session group-id falling back to pid-{pid}; \
            resume will not persist across invocations. \
            Set CODEX_SESSION_GROUP=<id> for stable resume."
    );
    if let Err(err) = ctx.ui.write_warning(&warning) {
        tracing::warn!(
            op = "group_id.fallback.warning",
            pid,
            err = %err,
            "failed to emit pid fallback warning"
        );
    }
    tracing::warn!(op = "group_id.fallback", pid);
    Ok(ResolvedGroupId {
        id: GroupId::from_unchecked(format!("pid-{pid}")),
        source: GroupIdSource::Pid,
    })
}

fn validate_group_id(value: &str) -> Result<(), String> {
    let len = value.len();
    if len > 64 {
        return Err("must be 64 bytes or fewer".to_owned());
    }
    if value.is_empty() {
        return Err("must not be empty".to_owned());
    }
    if len > 32 {
        return Err("must be 32 bytes or fewer".to_owned());
    }

    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return Err("must not be empty".to_owned());
    };
    if !first.is_ascii_lowercase() && !first.is_ascii_digit() {
        return Err("must start with a lowercase ASCII letter or digit".to_owned());
    }
    if let Some(invalid) = chars
        .find(|ch| !ch.is_ascii_lowercase() && !ch.is_ascii_digit() && *ch != '_' && *ch != '-')
    {
        return Err(format!(
            "contains invalid character `{invalid}`; \
                allowed: lowercase ASCII letters, digits, `-`, `_`"
        ));
    }
    Ok(())
}

fn current_from_tty() -> Option<GroupId> {
    let output = std::process::Command::new("tty").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let tty = String::from_utf8(output.stdout).ok()?;
    let stripped = tty.trim().strip_prefix("/dev/")?;
    let id = stripped.replace('/', "-");
    if id.len() > 64 {
        return None;
    }
    Some(GroupId::from_unchecked(id))
}

fn current_from_ppid() -> Option<GroupId> {
    let ppid = rustix::process::getppid()?.as_raw_pid();
    let stat = std::fs::read_to_string(format!("/proc/{ppid}/stat")).ok()?;
    let starttime = parse_stat(&stat)?;
    let id = format!("ppid-{ppid}-{starttime}");
    debug_assert!(GroupId::from_str(&id).is_ok());
    Some(GroupId::from_unchecked(id))
}

fn parse_stat(buf: &str) -> Option<u64> {
    let close = buf.rfind(')')?;
    let after = buf.get(close + 2..)?;
    after.split_ascii_whitespace().nth(19)?.parse().ok()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn test_ctx(group: Option<GroupId>) -> crate::context::AppContext {
        let temp = tempfile::tempdir().expect("tempdir");
        let base = camino::Utf8PathBuf::try_from(temp.path().to_path_buf()).expect("utf8");
        let config = crate::config::Config {
            child: crate::config::ChildConfig { bin: None },
            config_recipe: crate::config::ConfigRecipeConfig {
                active: None,
                default: None,
                config_dir: base.join("config"),
                recipes_dir: base.join("config-recipes"),
                configs_dir: base.join("configs"),
                profiles_dir: base.join("profiles"),
            },
            paths: crate::config::PathsConfig {
                cache_dir: base.join("cache"),
                state_dir: base.join("state"),
                runtime_dir: Some(base.join("runtime")),
            },
            log: crate::config::LogConfig::default(),
            account: crate::config::AccountConfig::default(),
            sources: crate::config::ConfigSources::default(),
        };
        crate::context::AppContext::new(
            Arc::new(config),
            crate::cli::GlobalArgs {
                group,
                ..crate::cli::GlobalArgs::default()
            },
            base.join("home"),
        )
    }

    #[test]
    fn group_id_accepts_valid_values() {
        for value in ["foo", "a", "a-b_c-1"] {
            assert_eq!(GroupId::from_str(value).unwrap().as_str(), value);
        }
    }

    #[test]
    fn group_id_rejects_invalid_values() {
        for value in [
            "",
            "-foo",
            "_foo",
            "FOO",
            "foo/bar",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ] {
            assert!(GroupId::from_str(value).is_err(), "{value}");
        }
    }

    #[test]
    fn parse_stat_handles_normal_comm() {
        let line = "123 (bash) S 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21";
        assert_eq!(parse_stat(line), Some(19));
    }

    #[test]
    fn parse_stat_handles_weird_comm() {
        let line = "456 (weird (name))) S 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 123456 21";
        assert_eq!(parse_stat(line), Some(123_456));
    }

    #[test]
    fn resolve_prefers_flag_over_env() {
        let ctx = test_ctx(Some(GroupId::from_str("flag").unwrap()));
        let resolved = resolve_from_inputs(ctx.global.group.clone(), Some("env".to_owned()), &ctx)
            .expect("resolved");
        assert_eq!(resolved.id.as_str(), "flag");
        assert_eq!(resolved.source, GroupIdSource::Flag);
    }

    #[test]
    fn resolve_uses_env_when_flag_missing() {
        let ctx = test_ctx(None);
        let resolved = resolve_from_inputs(None, Some("env".to_owned()), &ctx).expect("resolved");
        assert_eq!(resolved.id.as_str(), "env");
        assert_eq!(resolved.source, GroupIdSource::Env);
    }

    #[test]
    fn resolve_rejects_invalid_env() {
        let ctx = test_ctx(None);
        let err = resolve_from_inputs(None, Some("BAD!".to_owned()), &ctx).expect_err("err");
        assert!(matches!(err, AppError::Usage(_)));
    }
}
