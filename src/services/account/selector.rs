//! Scoring formula is adapted verbatim from caam (Coding Agent Account
//! Manager) ADR D7. Source:
//! `Dicklesworthstone/coding_agent_account_manager` @ `internal/rotation/rotation.go`.

#![allow(clippy::result_large_err)]

use std::time::{Duration, SystemTime};

use super::{AccountError, AccountId, cooldown, quota, registry::Registry};

const LONG_IDLE_SECS: u64 = 7 * 24 * 60 * 60;

pub(crate) fn pick(ctx: &crate::context::AppContext) -> Result<AccountId, AccountError> {
    let registry = Registry::from_config(&ctx.config);
    let accounts = registry.list()?;
    let lru = registry.current()?;
    let ttl = Duration::from_secs(ctx.config.account.quota_ttl_secs);
    let now = SystemTime::now();

    let mut candidates = Vec::with_capacity(accounts.len());
    for entry in accounts {
        if cooldown_active(&registry, &entry.id)? {
            tracing::debug!(account = %entry.id, reason = "cooldown");
            continue;
        }

        let plan_bonus = quota::plan_bonus(ctx, &entry.id);
        let quota_state = match quota::get(ctx, &entry.id, ttl) {
            Ok(quota::QuotaResult::Ok(quota) | quota::QuotaResult::Stale(quota)) => {
                QuotaState::Known(quota)
            }
            Ok(quota::QuotaResult::ApiKeyMode) => QuotaState::ApiKeyMode,
            Err(err) => {
                tracing::warn!(
                    op = "quota.fetch",
                    account = %entry.id,
                    outcome = "error",
                    err = %err
                );
                QuotaState::Unknown
            }
        };

        candidates.push(Candidate {
            id: entry.id,
            last_used_at: entry.last_used_at,
            lru: lru.as_ref(),
            quota_state,
            plan_bonus,
        });
    }

    let picked = pick_from_candidates(
        &candidates,
        now,
        ctx.config.account.five_hour_threshold,
        ctx.config.account.weekly_floor,
        ctx.config.account.five_hour_weight,
    )?;

    registry.set_current(&picked.id)?;
    tracing::info!(
        op = "account.select",
        account = %picked.id,
        total = picked.total
    );
    Ok(picked.id)
}

#[derive(Debug, Clone)]
struct Candidate<'a> {
    id: AccountId,
    last_used_at: Option<SystemTime>,
    lru: Option<&'a AccountId>,
    quota_state: QuotaState,
    plan_bonus: i64,
}

#[derive(Debug, Clone)]
enum QuotaState {
    Known(quota::Quota),
    ApiKeyMode,
    Unknown,
}

#[derive(Debug, Clone)]
struct ScoredCandidate {
    id: AccountId,
    total: f64,
    tie_five_hour: Option<f64>,
}

fn pick_from_candidates(
    candidates: &[Candidate<'_>],
    now: SystemTime,
    five_hour_threshold: f64,
    weekly_floor: f64,
    five_hour_weight: f64,
) -> Result<ScoredCandidate, AccountError> {
    let mut best: Option<ScoredCandidate> = None;

    for candidate in candidates {
        let Some(scored) = score_candidate(
            candidate,
            now,
            five_hour_threshold,
            weekly_floor,
            five_hour_weight,
        ) else {
            tracing::debug!(account = %candidate.id, reason = "threshold");
            continue;
        };

        let replace =
            best.as_ref()
                .is_none_or(|current| match scored.total.partial_cmp(&current.total) {
                    Some(std::cmp::Ordering::Greater) => true,
                    Some(std::cmp::Ordering::Less) | None => false,
                    Some(std::cmp::Ordering::Equal) => {
                        let scored_tie = scored.tie_five_hour.unwrap_or(f64::NEG_INFINITY);
                        let current_tie = current.tie_five_hour.unwrap_or(f64::NEG_INFINITY);
                        match scored_tie.partial_cmp(&current_tie) {
                            Some(std::cmp::Ordering::Greater) => true,
                            Some(std::cmp::Ordering::Less) | None => false,
                            Some(std::cmp::Ordering::Equal) => {
                                scored.id.as_str() < current.id.as_str()
                            }
                        }
                    }
                });
        if replace {
            best = Some(scored);
        }
    }

    best.map_or_else(
        || {
            tracing::warn!(op = "account.select_no_eligible");
            Err(AccountError::NoEligible)
        },
        Ok,
    )
}

fn score_candidate(
    candidate: &Candidate<'_>,
    now: SystemTime,
    five_hour_threshold: f64,
    weekly_floor: f64,
    five_hour_weight: f64,
) -> Option<ScoredCandidate> {
    let health_bonus: f64 = 100.0;
    let penalty: f64 = 0.0;
    #[allow(
        clippy::cast_precision_loss,
        reason = "plan_bonus is a small bounded integer (0, 20, or 30)"
    )]
    let plan_bonus = candidate.plan_bonus as f64;
    let recency = if candidate.lru == Some(&candidate.id) {
        -30.0
    } else if candidate
        .last_used_at
        .and_then(|ts| now.duration_since(ts).ok())
        .is_some_and(|duration| duration.as_secs() > LONG_IDLE_SECS)
    {
        20.0
    } else {
        0.0
    };

    let (avail_score, weekly_pressure, fh_pressure, tie_five_hour, eligible) =
        match &candidate.quota_state {
            QuotaState::Known(quota) => {
                let eligible = quota.five_hour.percent_left > five_hour_threshold
                    && quota.weekly.percent_left > weekly_floor;
                let weekly_weight = 1.0 - five_hour_weight;
                let avail_score = five_hour_weight.mul_add(
                    quota.five_hour.percent_left,
                    weekly_weight * quota.weekly.percent_left,
                ) - 50.0;
                let weekly_pressure = if quota.weekly.percent_left < 20.0 {
                    -30.0
                } else {
                    0.0
                };
                let fh_pressure = if quota.five_hour.percent_left < 15.0 {
                    -25.0
                } else {
                    0.0
                };
                (
                    avail_score,
                    weekly_pressure,
                    fh_pressure,
                    Some(quota.five_hour.percent_left),
                    eligible,
                )
            }
            QuotaState::ApiKeyMode | QuotaState::Unknown => (0.0, 0.0, 0.0, None, true),
        };

    if !eligible {
        return None;
    }

    Some(ScoredCandidate {
        id: candidate.id.clone(),
        total: health_bonus
            + penalty
            + plan_bonus
            + recency
            + avail_score
            + weekly_pressure
            + fh_pressure,
        tie_five_hour,
    })
}

fn cooldown_active(registry: &Registry, account: &AccountId) -> Result<bool, AccountError> {
    let account_root = registry.account_dir(account);
    Ok(cooldown::read(&account_root)?
        .is_some_and(|cooldown| cooldown::is_active(&cooldown, now_unix())))
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use super::{Candidate, LONG_IDLE_SECS, QuotaState, cooldown_active, pick_from_candidates};

    fn known(five_hour: f64, weekly: f64) -> QuotaState {
        QuotaState::Known(crate::services::account::quota::Quota {
            five_hour: crate::services::account::quota::Window {
                percent_left: five_hour,
                reset_at_unix: 0,
            },
            weekly: crate::services::account::quota::Window {
                percent_left: weekly,
                reset_at_unix: 0,
            },
        })
    }

    fn candidate<'a>(
        name: &str,
        quota_state: QuotaState,
        last_used_at: Option<std::time::SystemTime>,
        lru: Option<&'a crate::services::account::AccountId>,
        plan_bonus: i64,
    ) -> Candidate<'a> {
        Candidate {
            id: name.parse().unwrap(),
            last_used_at,
            lru,
            quota_state,
            plan_bonus,
        }
    }

    #[test]
    fn highest_score_wins() {
        let now = std::time::SystemTime::now();
        let candidates = vec![
            candidate("low", known(60.0, 60.0), None, None, 0),
            candidate("high", known(90.0, 80.0), None, None, 0),
        ];
        let picked = pick_from_candidates(&candidates, now, 50.0, 10.0, 0.70).unwrap();
        assert_eq!(picked.id.as_str(), "high");
    }

    #[test]
    fn below_threshold_is_excluded() {
        let now = std::time::SystemTime::now();
        let candidates = vec![candidate("low", known(49.0, 80.0), None, None, 0)];
        let err = pick_from_candidates(&candidates, now, 50.0, 10.0, 0.70).unwrap_err();
        assert!(matches!(
            err,
            crate::services::account::AccountError::NoEligible
        ));
    }

    #[test]
    fn all_below_threshold_returns_no_eligible() {
        let now = std::time::SystemTime::now();
        let candidates = vec![
            candidate("one", known(40.0, 80.0), None, None, 0),
            candidate("two", known(80.0, 5.0), None, None, 0),
        ];
        let err = pick_from_candidates(&candidates, now, 50.0, 10.0, 0.70).unwrap_err();
        assert!(matches!(
            err,
            crate::services::account::AccountError::NoEligible
        ));
    }

    #[test]
    fn api_key_mode_is_eligible_without_quota_gate() {
        let now = std::time::SystemTime::now();
        let candidates = vec![candidate("api", QuotaState::ApiKeyMode, None, None, 0)];
        let picked = pick_from_candidates(&candidates, now, 50.0, 10.0, 0.70).unwrap();
        assert_eq!(picked.id.as_str(), "api");
    }

    #[test]
    fn lru_penalty_applies() {
        let now = std::time::SystemTime::now();
        let lru: crate::services::account::AccountId = "fav".parse().unwrap();
        let candidates = vec![
            candidate("fav", known(80.0, 80.0), None, Some(&lru), 0),
            candidate("other", known(70.0, 70.0), None, Some(&lru), 0),
        ];
        let picked = pick_from_candidates(&candidates, now, 50.0, 10.0, 0.70).unwrap();
        assert_eq!(picked.id.as_str(), "other");
    }

    #[test]
    fn long_idle_reward_applies() {
        let now = std::time::SystemTime::now();
        let idle = now - Duration::from_secs(LONG_IDLE_SECS + 60);
        let candidates = vec![
            candidate("idle", known(60.0, 60.0), Some(idle), None, 0),
            candidate("fresh", known(70.0, 70.0), None, None, 0),
        ];
        let picked = pick_from_candidates(&candidates, now, 50.0, 10.0, 0.70).unwrap();
        assert_eq!(picked.id.as_str(), "idle");
    }

    #[test]
    fn quota_error_like_unknown_keeps_account_eligible() {
        let now = std::time::SystemTime::now();
        let candidates = vec![candidate("unknown", QuotaState::Unknown, None, None, 0)];
        let picked = pick_from_candidates(&candidates, now, 50.0, 10.0, 0.70).unwrap();
        assert_eq!(picked.id.as_str(), "unknown");
    }

    #[test]
    fn weighted_scoring_prefers_higher_five_hour_quota() {
        let now = std::time::SystemTime::now();
        let candidates = vec![
            candidate("a", known(45.0, 95.0), None, None, 0),
            candidate("b", known(75.0, 65.0), None, None, 0),
        ];
        let picked = pick_from_candidates(&candidates, now, 0.0, 0.0, 0.70).unwrap();
        assert_eq!(picked.id.as_str(), "b");
    }

    #[test]
    fn five_hour_pressure_penalty_below_15_percent() {
        let now = std::time::SystemTime::now();
        let candidates = vec![
            candidate("low5h", known(14.0, 80.0), None, None, 0),
            candidate("ok5h", known(55.0, 55.0), None, None, 0),
        ];
        let picked = pick_from_candidates(&candidates, now, 0.0, 0.0, 0.70).unwrap();
        assert_eq!(picked.id.as_str(), "ok5h");
    }

    #[test]
    fn cooldown_file_disqualifies_account() {
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
            account: crate::config::AccountConfig::default(),
            sources: crate::config::ConfigSources::default(),
        };
        let ctx = crate::context::AppContext::new(
            Arc::new(config),
            crate::cli::GlobalArgs::default(),
            base.join("home"),
        );
        let id: crate::services::account::AccountId = "cool".parse().unwrap();
        crate::services::account::registry::Registry::from_config(&ctx.config)
            .add(&id)
            .unwrap();
        let registry = crate::services::account::registry::Registry::from_config(&ctx.config);
        let account_root = registry.account_dir(&id);
        std::fs::create_dir_all(account_root.as_std_path()).unwrap();
        crate::services::account::cooldown::write(
            &account_root,
            &crate::services::account::cooldown::Cooldown {
                reset_at_unix: 4_102_444_800,
                reason: "429 detected".to_owned(),
                last_429_at_unix: 4_102_444_500,
                snippet_truncated: "HTTP 429 Too Many Requests".to_owned(),
            },
        )
        .unwrap();

        assert!(cooldown_active(&registry, &id).unwrap());
    }
}
