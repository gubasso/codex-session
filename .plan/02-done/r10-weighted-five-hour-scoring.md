# Weighted 5-hour quota scoring + five-hour pressure penalty

## Context

The account auto-selector's `avail_score` currently uses an equal-weight midpoint
of the 5-hour and weekly quota percentages. In practice the 5-hour window is the
binding constraint — it drains fast and resets every 5 hours, while the weekly
quota rarely runs out. Giving both windows equal weight masks this asymmetry and
can cause the selector to pick an account with low immediate headroom just because
its weekly looks good.

This change:
1. Replaces the equal-weight midpoint with a **configurable weighted average** (default 70/30 favoring 5-hour).
2. Adds a **five-hour pressure penalty** (analogous to the existing weekly one) so accounts near 5-hour exhaustion get a hard shove.
3. Exposes the weight as a config knob with a sane default.

## Files to modify

### 1. `src/config/mod.rs` — add `five_hour_weight` config field

**`AccountConfig`** (line 68): add field `five_hour_weight: f64`.

**`Default for AccountConfig`** (line 237): set default to `0.70`.

**`FileAccountConfig`** (line 169): add `five_hour_weight: Option<f64>`.

**`apply_file_config`** (line 409 block): add arm for `five_hour_weight`.

**`apply_env_layer`** (line 433 block): add `ACCOUNT_FIVE_HOUR_WEIGHT` env var.

### 2. `src/services/account/selector.rs` — weighted scoring + pressure penalty

**`pick_from_candidates`** (line 92): add `five_hour_weight: f64` parameter, thread it to `score_candidate`.

**`pick`** (line 53): pass `ctx.config.account.five_hour_weight` to `pick_from_candidates`.

**`score_candidate`** (line 138): add `five_hour_weight: f64` parameter.

In the `QuotaState::Known` arm (line 165):
- Replace `f64::midpoint(...)` with weighted average:
  `five_hour_weight * five_hour% + (1.0 - five_hour_weight) * weekly% - 50.0`
- Add five-hour pressure penalty: `-25.0` when `five_hour% < 15.0`, else `0.0`.

Update the total score formula (line 191) to include `five_hour_pressure_penalty`.

### 3. `src/services/account/selector.rs` tests — update call sites

All `pick_from_candidates` calls in tests (lines 257, 265, 279, 289, 302, 314, 324)
gain the new `five_hour_weight` parameter — pass `0.70` everywhere.

Add new test: verify that the weighted formula picks the account with higher 5-hour
quota when weekly is similar (the scenario from the /ask analysis: A=45/95 vs B=75/65
should no longer tie).

Add new test: verify five-hour pressure penalty kicks in below 15%.

### 4. `docs/account-auto-selector.md` — update documentation

- Update the `avail_score` section to show the weighted formula and explain why 70/30.
- Add a `five_hour_pressure_penalty` section parallel to `weekly_pressure_penalty`.
- Update the worked example table and totals.
- Add `five_hour_weight` to the Configuration table at the bottom.
- Update the scoring formula summary to include `five_hour_pressure_penalty`.

## Constants and defaults

| Item | Value | Rationale |
|---|---|---|
| `five_hour_weight` default | 0.70 | 5-hour is the practical bottleneck; 70/30 reflects its ~34x higher reset frequency |
| weekly weight (derived) | `1.0 - five_hour_weight` | Single knob, always sums to 1.0 |
| five-hour pressure threshold | 15% (hardcoded) | Matches pattern of weekly pressure (20%); slightly lower because avail_score already weights 5-hour more |
| five-hour pressure penalty | -25.0 (hardcoded) | Lighter than weekly's -30.0 since the weighted avail_score already penalizes low 5-hour more |

## Verification

1. `just test-unit` — all existing + new tests pass
2. `just lint` — no clippy/fmt/print-ownership violations
3. `just check` — full gate green
