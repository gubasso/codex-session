//! Scoring formula is adapted verbatim from caam (Coding Agent Account
//! Manager) ADR D7. Source:
//! `Dicklesworthstone/coding_agent_account_manager` @ `internal/rotation/rotation.go`.

#![allow(clippy::result_large_err)]

use std::time::{Duration, SystemTime};

use super::{AccountError, AccountId, cooldown, quota, registry::Registry, token_expiry};

const LONG_IDLE_SECS: u64 = 7 * 24 * 60 * 60;

pub(crate) fn pick(ctx: &crate::context::AppContext) -> Result<AccountId, AccountError> {
    let registry = Registry::from_config(&ctx.config);
    let accounts = registry.list()?;
    let lru = registry.current()?;
    let ttl = Duration::from_secs(ctx.config.account.quota_ttl_secs);
    let now = SystemTime::now();

    let mut candidates = Vec::with_capacity(accounts.len());
    for entry in accounts {
        if !entry.has_auth {
            tracing::debug!(account = %entry.id, reason = "no-auth");
            continue;
        }
        if cooldown_active(&registry, &entry.id)? {
            tracing::debug!(account = %entry.id, reason = "cooldown");
            continue;
        }
        if token_expired(ctx, &entry.id) {
            tracing::debug!(account = %entry.id, reason = "token-expired");
            continue;
        }

        let plan_bonus = quota::plan_bonus(ctx, &entry.id);
        let quota_state = match quota::get(ctx, &entry.id, ttl) {
            Ok(quota::QuotaResult::Ok(quota)) => QuotaState::Known(quota),
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

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) struct ScoreBreakdown {
    pub(crate) base: f64,
    pub(crate) plan_bonus: f64,
    pub(crate) recency: f64,
    pub(crate) recency_label: String,
    pub(crate) avail_score: f64,
    pub(crate) five_hour_pct: Option<f64>,
    pub(crate) weekly_pct: Option<f64>,
    pub(crate) five_hour_weight: f64,
    pub(crate) weekly_pressure: f64,
    pub(crate) fh_pressure: f64,
    pub(crate) pressure_label: String,
    pub(crate) total: f64,
    pub(crate) eligible: bool,
    pub(crate) ineligible_reason: Option<String>,
    pub(crate) tie_five_hour: Option<f64>,
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
    let params = ScoringParams {
        plan_bonus: candidate.plan_bonus,
        last_used_at: candidate.last_used_at,
        is_lru: candidate.lru == Some(&candidate.id),
        now,
        five_hour_threshold,
        weekly_floor,
        five_hour_weight,
    };
    let breakdown = score_for_display(&candidate.quota_state, &params);
    if !breakdown.eligible {
        return None;
    }

    Some(ScoredCandidate {
        id: candidate.id.clone(),
        total: breakdown.total,
        tie_five_hour: breakdown.tie_five_hour,
    })
}

pub(crate) struct ScoringParams {
    pub(crate) plan_bonus: i64,
    pub(crate) last_used_at: Option<SystemTime>,
    pub(crate) is_lru: bool,
    pub(crate) now: SystemTime,
    pub(crate) five_hour_threshold: f64,
    pub(crate) weekly_floor: f64,
    pub(crate) five_hour_weight: f64,
}

pub(crate) fn score_from_quota_result(
    result: &quota::QuotaResult,
    params: &ScoringParams,
) -> ScoreBreakdown {
    let quota_state = match result {
        quota::QuotaResult::Ok(value) => QuotaState::Known(value.clone()),
        quota::QuotaResult::ApiKeyMode => QuotaState::ApiKeyMode,
    };
    score_for_display(&quota_state, params)
}

fn score_for_display(quota_state: &QuotaState, params: &ScoringParams) -> ScoreBreakdown {
    let base = 100.0;
    #[allow(
        clippy::cast_precision_loss,
        reason = "plan_bonus is a small bounded integer (0, 20, or 30)"
    )]
    let plan_bonus_f = params.plan_bonus as f64;
    let (recency, recency_label) = if params.is_lru {
        (-30.0, "LRU penalty".to_owned())
    } else if params
        .last_used_at
        .and_then(|ts| params.now.duration_since(ts).ok())
        .is_some_and(|duration| duration.as_secs() > LONG_IDLE_SECS)
    {
        (20.0, "long idle bonus".to_owned())
    } else {
        (0.0, "neutral".to_owned())
    };

    let (
        avail_score,
        weekly_pressure,
        fh_pressure,
        five_hour_pct,
        weekly_pct,
        tie_five_hour,
        eligible,
        ineligible_reason,
    ) = match quota_state {
        QuotaState::Known(quota) => {
            let eligible = quota.five_hour.percent_left > params.five_hour_threshold
                && quota.weekly.percent_left > params.weekly_floor;
            let fht = params.five_hour_threshold;
            let wf = params.weekly_floor;
            let reason = (!eligible).then_some(format!("five_hour>{fht} and weekly>{wf} required"));
            let weekly_weight = 1.0 - params.five_hour_weight;
            let avail_score = params.five_hour_weight.mul_add(
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
                Some(quota.weekly.percent_left),
                Some(quota.five_hour.percent_left),
                eligible,
                reason,
            )
        }
        QuotaState::ApiKeyMode | QuotaState::Unknown => {
            (0.0, 0.0, 0.0, None, None, None, true, None)
        }
    };
    let pressure_label = if weekly_pressure < 0.0 || fh_pressure < 0.0 {
        "penalized"
    } else {
        "none"
    }
    .to_owned();
    let total = base + plan_bonus_f + recency + avail_score + weekly_pressure + fh_pressure;

    ScoreBreakdown {
        base,
        plan_bonus: plan_bonus_f,
        recency,
        recency_label,
        avail_score,
        five_hour_pct,
        weekly_pct,
        five_hour_weight: params.five_hour_weight,
        weekly_pressure,
        fh_pressure,
        pressure_label,
        total,
        eligible,
        ineligible_reason,
        tie_five_hour,
    }
}

fn cooldown_active(registry: &Registry, account: &AccountId) -> Result<bool, AccountError> {
    let account_root = registry.account_dir(account);
    Ok(cooldown::read(&account_root)?
        .is_some_and(|cooldown| cooldown::is_active(&cooldown, now_unix())))
}

fn token_expired(ctx: &crate::context::AppContext, account: &AccountId) -> bool {
    let Ok(auth_path) = quota::resolve_auth_path(ctx, account) else {
        return false;
    };
    let Ok(data) = std::fs::read_to_string(auth_path.as_std_path()) else {
        return false;
    };
    let Ok(auth) = serde_json::from_str::<serde_json::Value>(&data) else {
        return false;
    };
    match token_expiry::token_expiry_from_auth(&auth) {
        token_expiry::TokenExpiry::ExpiresAt(exp) => now_unix() + 60 >= exp,
        _ => false,
    }
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

    use super::{
        Candidate, LONG_IDLE_SECS, QuotaState, ScoringParams, cooldown_active,
        pick_from_candidates, score_from_quota_result, token_expired,
    };

    fn test_ctx() -> (
        tempfile::TempDir,
        crate::context::AppContext,
        crate::services::account::registry::Registry,
    ) {
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
        let ctx = crate::context::AppContext::new(
            Arc::new(config),
            crate::cli::GlobalArgs::default(),
            base.join("home"),
        );
        let registry = crate::services::account::registry::Registry::from_config(&ctx.config);
        (temp, ctx, registry)
    }

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
    fn score_from_quota_result_returns_breakdown() {
        let result = crate::services::account::quota::QuotaResult::Ok(
            crate::services::account::quota::Quota {
                five_hour: crate::services::account::quota::Window {
                    percent_left: 80.0,
                    reset_at_unix: 0,
                },
                weekly: crate::services::account::quota::Window {
                    percent_left: 60.0,
                    reset_at_unix: 0,
                },
            },
        );
        let now = std::time::SystemTime::now();
        let score = score_from_quota_result(
            &result,
            &ScoringParams {
                plan_bonus: 20,
                last_used_at: None,
                is_lru: false,
                now,
                five_hour_threshold: 50.0,
                weekly_floor: 10.0,
                five_hour_weight: 0.70,
            },
        );
        assert!(score.eligible);
        assert!(score.total > 100.0);
        assert_eq!(score.five_hour_pct, Some(80.0));
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
        let (_temp, _ctx, registry) = test_ctx();
        let id: crate::services::account::AccountId = "cool".parse().unwrap();
        registry.add(&id).unwrap();
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

    #[test]
    fn token_expired_true_when_near_or_past_expiry() {
        let (_temp, ctx, registry) = test_ctx();
        let id: crate::services::account::AccountId = "expired".parse().unwrap();
        registry.add(&id).unwrap();
        let auth_path = registry.group_auth_seed_path(&id);
        std::fs::write(
            auth_path,
            r#"{"tokens":{"access_token":"eyJhbGciOiJub25lIn0.eyJleHAiOjE3MDAwMDAwMDB9."}}"#,
        )
        .unwrap();

        assert!(token_expired(&ctx, &id));
    }

    #[test]
    fn token_expired_false_when_valid() {
        let (_temp, ctx, registry) = test_ctx();
        let id: crate::services::account::AccountId = "valid".parse().unwrap();
        registry.add(&id).unwrap();
        let auth_path = registry.group_auth_seed_path(&id);
        std::fs::write(
            auth_path,
            r#"{"tokens":{"access_token":"eyJhbGciOiJub25lIn0.eyJleHAiOjQxMDI0NDQ4MDB9."}}"#,
        )
        .unwrap();

        assert!(!token_expired(&ctx, &id));
    }

    #[test]
    fn token_expired_false_when_missing_token() {
        let (_temp, ctx, registry) = test_ctx();
        let id: crate::services::account::AccountId = "missing".parse().unwrap();
        registry.add(&id).unwrap();
        let auth_path = registry.group_auth_seed_path(&id);
        std::fs::write(auth_path, r#"{"tokens":{}}"#).unwrap();

        assert!(!token_expired(&ctx, &id));
    }

    #[test]
    fn token_expired_false_when_malformed_jwt() {
        let (_temp, ctx, registry) = test_ctx();
        let id: crate::services::account::AccountId = "badjwt".parse().unwrap();
        registry.add(&id).unwrap();
        let auth_path = registry.group_auth_seed_path(&id);
        std::fs::write(auth_path, r#"{"tokens":{"access_token":"bad"}}"#).unwrap();

        assert!(!token_expired(&ctx, &id));
    }

    #[test]
    fn token_expired_false_for_api_key_mode() {
        let (_temp, ctx, registry) = test_ctx();
        let id: crate::services::account::AccountId = "apikey".parse().unwrap();
        registry.add(&id).unwrap();
        let auth_path = registry.group_auth_seed_path(&id);
        std::fs::write(auth_path, r#"{"api_key":"sk-test"}"#).unwrap();

        assert!(!token_expired(&ctx, &id));
    }

    #[test]
    fn token_expired_false_when_auth_missing() {
        let (_temp, ctx, registry) = test_ctx();
        let id: crate::services::account::AccountId = "missingfile".parse().unwrap();
        registry.add(&id).unwrap();

        assert!(!token_expired(&ctx, &id));
    }
}
