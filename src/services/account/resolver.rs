#![allow(clippy::result_large_err)]

use std::str::FromStr as _;

use super::{AccountError, AccountId, registry::Registry};

#[derive(Debug, Clone, Copy)]
pub(crate) enum AccountResolutionSource {
    Flag,
    Env,
    Auto,
    Lru,
    ConfigPinned,
    Interactive,
}

#[derive(Debug, Clone)]
pub(crate) struct ResolvedAccount {
    pub(crate) id: AccountId,
    pub(crate) source: AccountResolutionSource,
}

pub(crate) const fn source_label(source: AccountResolutionSource) -> &'static str {
    match source {
        AccountResolutionSource::Flag => "flag",
        AccountResolutionSource::Env => "env",
        AccountResolutionSource::Auto => "auto",
        AccountResolutionSource::Lru => "lru",
        AccountResolutionSource::ConfigPinned => "config-pinned",
        AccountResolutionSource::Interactive => "interactive",
    }
}

pub(crate) fn resolve(
    ctx: &crate::context::AppContext,
) -> Result<ResolvedAccount, crate::error::AppError> {
    resolve_from_inputs(ctx, std::env::var("CODEX_SESSION_ACCOUNT").ok())
}

fn resolve_from_inputs(
    ctx: &crate::context::AppContext,
    env_account: Option<String>,
) -> Result<ResolvedAccount, crate::error::AppError> {
    use crate::cli::account::AccountSelector;

    if let Some(selector) = ctx.global.account.as_ref() {
        match selector {
            AccountSelector::Named(id) => {
                tracing::info!(op = "account.resolve", source = "flag", account = %id);
                return Ok(ResolvedAccount {
                    id: id.clone(),
                    source: AccountResolutionSource::Flag,
                });
            }
            AccountSelector::Auto => {
                let id = super::selector::pick(ctx)?;
                tracing::info!(op = "account.resolve", source = "auto", account = %id);
                return Ok(ResolvedAccount {
                    id,
                    source: AccountResolutionSource::Auto,
                });
            }
        }
    }

    if let Some(raw) = env_account.filter(|s| !s.is_empty()) {
        if raw == "auto" {
            let id = super::selector::pick(ctx)?;
            tracing::info!(op = "account.resolve", source = "auto", account = %id);
            return Ok(ResolvedAccount {
                id,
                source: AccountResolutionSource::Auto,
            });
        }
        let id = AccountId::from_str(&raw)
            .map_err(|reason| AccountError::InvalidName { value: raw, reason })?;
        tracing::info!(op = "account.resolve", source = "env", account = %id);
        return Ok(ResolvedAccount {
            id,
            source: AccountResolutionSource::Env,
        });
    }

    let registry = Registry::from_config(&ctx.config);
    if let Some(id) = registry.current()? {
        tracing::info!(op = "account.resolve", source = "lru", account = %id);
        return Ok(ResolvedAccount {
            id,
            source: AccountResolutionSource::Lru,
        });
    }

    if let Some(id) = ctx.config.account.pinned.as_ref() {
        tracing::info!(op = "account.resolve", source = "config-pinned", account = %id);
        return Ok(ResolvedAccount {
            id: id.clone(),
            source: AccountResolutionSource::ConfigPinned,
        });
    }

    tracing::info!(op = "account.resolve", source = "none-resolved");
    Err(AccountError::NoneResolved.into())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::sync::Arc;

    use super::{AccountResolutionSource, resolve, resolve_from_inputs};

    fn test_ctx(
        account: Option<crate::cli::account::AccountSelector>,
        pinned: Option<crate::services::account::AccountId>,
    ) -> crate::context::AppContext {
        let temp = tempfile::tempdir().unwrap();
        let base = camino::Utf8PathBuf::try_from(temp.path().to_path_buf()).unwrap();
        let config = crate::config::Config {
            child: crate::config::ChildConfig { bin: None },
            profile: crate::config::ProfileConfig {
                active: None,
                default: None,
                config_dir: base.join("config"),
                profiles_dir: base.join("profiles"),
                settings_dir: base.join("settings"),
            },
            paths: crate::config::PathsConfig {
                cache_dir: base.join("cache"),
                state_dir: base.join("state"),
                runtime_dir: Some(base.join("runtime")),
            },
            log: crate::config::LogConfig::default(),
            account: crate::config::AccountConfig {
                pinned,
                ..crate::config::AccountConfig::default()
            },
            sources: crate::config::ConfigSources::default(),
        };
        crate::context::AppContext::new(
            Arc::new(config),
            crate::cli::GlobalArgs {
                account,
                ..crate::cli::GlobalArgs::default()
            },
            base.join("home"),
        )
    }

    #[test]
    fn flag_wins_over_env() {
        let ctx = test_ctx(
            Some(crate::cli::account::AccountSelector::Named(
                "flag".parse().unwrap(),
            )),
            None,
        );
        let resolved = resolve_from_inputs(&ctx, Some("env".to_owned())).unwrap();
        assert_eq!(resolved.id.as_str(), "flag");
        assert!(matches!(resolved.source, AccountResolutionSource::Flag));
    }

    #[test]
    fn env_wins_when_no_flag() {
        let ctx = test_ctx(None, None);
        let resolved = resolve_from_inputs(&ctx, Some("env".to_owned())).unwrap();
        assert_eq!(resolved.id.as_str(), "env");
        assert!(matches!(resolved.source, AccountResolutionSource::Env));
    }

    #[test]
    fn lru_wins_over_config() {
        let ctx = test_ctx(None, Some("pinned".parse().unwrap()));
        let registry = crate::services::account::registry::Registry::from_config(&ctx.config);
        let id = "lru".parse().unwrap();
        registry.add(&id).unwrap();
        registry.set_current(&id).unwrap();
        let resolved = resolve(&ctx).unwrap();
        assert_eq!(resolved.id.as_str(), "lru");
        assert!(matches!(resolved.source, AccountResolutionSource::Lru));
    }

    #[test]
    fn config_pinned_is_used() {
        let ctx = test_ctx(None, Some("pinned".parse().unwrap()));
        let resolved = resolve(&ctx).unwrap();
        assert_eq!(resolved.id.as_str(), "pinned");
        assert!(matches!(
            resolved.source,
            AccountResolutionSource::ConfigPinned
        ));
    }

    #[test]
    fn no_sources_returns_none_resolved() {
        let ctx = test_ctx(None, None);
        let result = resolve(&ctx);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(
                err,
                crate::error::AppError::Account(
                    crate::services::account::AccountError::NoneResolved
                )
            ),
            "expected NoneResolved, got: {err:?}"
        );
    }

    #[test]
    fn auto_invokes_selector() {
        let ctx = test_ctx(Some(crate::cli::account::AccountSelector::Auto), None);
        let registry = crate::services::account::registry::Registry::from_config(&ctx.config);
        let id = "auto".parse().unwrap();
        registry.add(&id).unwrap();
        std::fs::write(registry.group_auth_seed_path(&id).as_std_path(), b"{}").unwrap();
        let cache_dir = ctx.config.paths.state_dir.join("cache").join("quota");
        std::fs::create_dir_all(cache_dir.as_std_path()).unwrap();
        let cache_path = cache_dir.join("auto.json");
        std::fs::write(
            cache_path.as_std_path(),
            r#"{
    "fetched_at_unix": 4102444800,
    "ttl_secs": 30,
    "body": {
        "kind": "ok",
        "five_hour": { "percent_left": 80.0, "reset_at_unix": 0 },
        "weekly": { "percent_left": 80.0, "reset_at_unix": 0 }
    }
}"#,
        )
        .unwrap();
        let resolved = resolve(&ctx).unwrap();
        assert_eq!(resolved.id.as_str(), "auto");
        assert!(matches!(resolved.source, AccountResolutionSource::Auto));
    }

    #[test]
    fn auto_env_invokes_selector() {
        let ctx = test_ctx(None, None);
        let registry = crate::services::account::registry::Registry::from_config(&ctx.config);
        let id = "autoenv".parse().unwrap();
        registry.add(&id).unwrap();
        std::fs::write(registry.group_auth_seed_path(&id).as_std_path(), b"{}").unwrap();
        let cache_dir = ctx.config.paths.state_dir.join("cache").join("quota");
        std::fs::create_dir_all(cache_dir.as_std_path()).unwrap();
        let cache_path = cache_dir.join("autoenv.json");
        std::fs::write(
            cache_path.as_std_path(),
            r#"{
    "fetched_at_unix": 4102444800,
    "ttl_secs": 30,
    "body": {
        "kind": "ok",
        "five_hour": { "percent_left": 80.0, "reset_at_unix": 0 },
        "weekly": { "percent_left": 80.0, "reset_at_unix": 0 }
    }
}"#,
        )
        .unwrap();
        let resolved = resolve_from_inputs(&ctx, Some("auto".to_owned())).unwrap();
        assert_eq!(resolved.id.as_str(), "autoenv");
        assert!(matches!(resolved.source, AccountResolutionSource::Auto));
    }
}
