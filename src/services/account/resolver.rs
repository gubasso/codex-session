#![allow(clippy::result_large_err)]

use std::collections::HashSet;
use std::str::FromStr as _;

use super::{AccountError, AccountId, registry::Registry};

#[derive(Debug, Clone, Copy)]
pub(crate) enum AccountResolutionSource {
    Flag,
    Env,
    Auto,
    Interactive,
    ThreadIndex,
    RolloutScan,
}

#[derive(Debug, Clone)]
pub(crate) struct ResolvedAccount {
    pub(crate) id: AccountId,
    pub(crate) source: AccountResolutionSource,
}

#[derive(Debug, Clone)]
pub(crate) enum AccountIntent {
    Pinned {
        id: AccountId,
        source: AccountResolutionSource,
    },
    Auto,
}

#[derive(Debug, Clone)]
pub(crate) enum DisplayAccount {
    Pinned {
        id: AccountId,
        source: AccountResolutionSource,
    },
    Auto {
        last_selected: Option<AccountId>,
    },
}

pub(crate) const fn source_label(source: AccountResolutionSource) -> &'static str {
    match source {
        AccountResolutionSource::Flag => "flag",
        AccountResolutionSource::Env => "env",
        AccountResolutionSource::Auto => "auto",
        AccountResolutionSource::Interactive => "interactive",
        AccountResolutionSource::ThreadIndex => "thread-index",
        AccountResolutionSource::RolloutScan => "rollout-scan",
    }
}

pub(crate) fn intent(
    ctx: &crate::context::AppContext,
) -> Result<AccountIntent, crate::error::AppError> {
    intent_from_inputs(ctx, std::env::var("CODEX_SESSION_ACCOUNT").ok())
}

pub(crate) fn intent_from_inputs(
    ctx: &crate::context::AppContext,
    env_account: Option<String>,
) -> Result<AccountIntent, crate::error::AppError> {
    use crate::cli::account::AccountSelector;

    if let Some(selector) = ctx.global.account.as_ref() {
        return match selector {
            AccountSelector::Named(id) => {
                tracing::info!(op = "account.intent", source = "flag", account = %id);
                Ok(AccountIntent::Pinned {
                    id: id.clone(),
                    source: AccountResolutionSource::Flag,
                })
            }
            AccountSelector::Auto => {
                tracing::info!(op = "account.intent", source = "auto");
                Ok(AccountIntent::Auto)
            }
        };
    }

    if let Some(raw) = env_account.filter(|s| !s.is_empty()) {
        if raw == "auto" {
            tracing::info!(op = "account.intent", source = "auto");
            return Ok(AccountIntent::Auto);
        }
        let id = AccountId::from_str(&raw)
            .map_err(|reason| AccountError::InvalidName { value: raw, reason })?;
        tracing::info!(op = "account.intent", source = "env", account = %id);
        return Ok(AccountIntent::Pinned {
            id,
            source: AccountResolutionSource::Env,
        });
    }

    tracing::info!(op = "account.intent", source = "auto");
    Ok(AccountIntent::Auto)
}

pub(crate) fn resolve_for_exec(
    ctx: &crate::context::AppContext,
    exclude: &HashSet<AccountId>,
) -> Result<ResolvedAccount, crate::error::AppError> {
    match intent(ctx)? {
        AccountIntent::Pinned { id, source } => {
            tracing::info!(
                op = "account.resolve_exec",
                account = %id,
                source = source_label(source)
            );
            Ok(ResolvedAccount { id, source })
        }
        AccountIntent::Auto => {
            let id = super::selector::pick(ctx, exclude)?;
            tracing::info!(op = "account.resolve_exec", account = %id, source = "auto");
            Ok(ResolvedAccount {
                id,
                source: AccountResolutionSource::Auto,
            })
        }
    }
}

pub(crate) fn resolve_for_display(
    ctx: &crate::context::AppContext,
) -> Result<DisplayAccount, crate::error::AppError> {
    match intent(ctx)? {
        AccountIntent::Pinned { id, source } => Ok(DisplayAccount::Pinned { id, source }),
        AccountIntent::Auto => {
            let registry = Registry::from_config(&ctx.config);
            Ok(DisplayAccount::Auto {
                last_selected: registry.current()?,
            })
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::collections::HashSet;
    use std::sync::Arc;

    use super::{
        AccountIntent, AccountResolutionSource, DisplayAccount, intent_from_inputs,
        resolve_for_display, resolve_for_exec,
    };

    fn test_ctx(
        account: Option<crate::cli::account::AccountSelector>,
    ) -> crate::context::AppContext {
        let temp = tempfile::tempdir().unwrap();
        let base = camino::Utf8PathBuf::try_from(temp.path().to_path_buf()).unwrap();
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
                account,
                ..crate::cli::GlobalArgs::default()
            },
            base.join("home"),
        )
    }

    #[test]
    fn flag_wins_over_env() {
        let ctx = test_ctx(Some(crate::cli::account::AccountSelector::Named(
            "flag".parse().unwrap(),
        )));
        let intent = intent_from_inputs(&ctx, Some("env".to_owned())).unwrap();
        assert!(matches!(intent, AccountIntent::Pinned { .. }));
        if let AccountIntent::Pinned { id, source } = intent {
            assert_eq!(id.as_str(), "flag");
            assert!(matches!(source, AccountResolutionSource::Flag));
        }
    }

    #[test]
    fn env_wins_when_no_flag() {
        let ctx = test_ctx(None);
        let intent = intent_from_inputs(&ctx, Some("env".to_owned())).unwrap();
        assert!(matches!(intent, AccountIntent::Pinned { .. }));
        if let AccountIntent::Pinned { id, source } = intent {
            assert_eq!(id.as_str(), "env");
            assert!(matches!(source, AccountResolutionSource::Env));
        }
    }

    #[test]
    fn no_flag_no_env_defaults_to_auto() {
        let ctx = test_ctx(None);
        let intent = intent_from_inputs(&ctx, None).unwrap();
        assert!(matches!(intent, AccountIntent::Auto));
    }

    #[test]
    fn empty_env_defaults_to_auto() {
        let ctx = test_ctx(None);
        let intent = intent_from_inputs(&ctx, Some(String::new())).unwrap();
        assert!(matches!(intent, AccountIntent::Auto));
    }

    #[test]
    fn explicit_auto_flag_intent_is_auto() {
        let ctx = test_ctx(Some(crate::cli::account::AccountSelector::Auto));
        let intent = intent_from_inputs(&ctx, Some("env".to_owned())).unwrap();
        assert!(matches!(intent, AccountIntent::Auto));
    }

    #[test]
    fn explicit_auto_env_intent_is_auto() {
        let ctx = test_ctx(None);
        let intent = intent_from_inputs(&ctx, Some("auto".to_owned())).unwrap();
        assert!(matches!(intent, AccountIntent::Auto));
    }

    #[test]
    fn invalid_env_returns_invalid_name() {
        let ctx = test_ctx(None);
        let result = intent_from_inputs(&ctx, Some("BAD!".to_owned()));
        assert!(matches!(
            result.unwrap_err(),
            crate::error::AppError::Account(
                crate::services::account::AccountError::InvalidName { .. }
            )
        ));
    }

    #[test]
    fn pinned_resolve_for_exec_ignores_exclude() {
        let id = "flag".parse().unwrap();
        let ctx = test_ctx(Some(crate::cli::account::AccountSelector::Named(id)));
        let mut exclude = HashSet::new();
        exclude.insert("flag".parse().unwrap());
        let resolved = resolve_for_exec(&ctx, &exclude).unwrap();
        assert_eq!(resolved.id.as_str(), "flag");
        assert!(matches!(resolved.source, AccountResolutionSource::Flag));
    }

    #[test]
    fn auto_resolve_for_exec_invokes_selector() {
        let ctx = test_ctx(Some(crate::cli::account::AccountSelector::Auto));
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
        let resolved = resolve_for_exec(&ctx, &HashSet::new()).unwrap();
        assert_eq!(resolved.id.as_str(), "auto");
        assert!(matches!(resolved.source, AccountResolutionSource::Auto));
    }

    #[test]
    fn auto_env_resolve_for_exec_invokes_selector() {
        let ctx = test_ctx(None);
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
        let resolved = resolve_for_exec(&ctx, &HashSet::new()).unwrap();
        assert_eq!(resolved.id.as_str(), "autoenv");
        assert!(matches!(resolved.source, AccountResolutionSource::Auto));
    }

    #[test]
    fn display_auto_reads_last_selected_without_pick() {
        let ctx = test_ctx(None);
        let registry = crate::services::account::registry::Registry::from_config(&ctx.config);
        let id = "last".parse().unwrap();
        registry.add(&id).unwrap();
        registry.set_current(&id).unwrap();
        let display = resolve_for_display(&ctx).unwrap();
        assert!(matches!(
            display,
            DisplayAccount::Auto {
                last_selected: Some(_)
            }
        ));
        if let DisplayAccount::Auto {
            last_selected: Some(value),
        } = display
        {
            assert_eq!(value.as_str(), "last");
        }
    }
}
