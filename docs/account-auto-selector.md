# Account auto-selector algorithm

When `--account auto` is used, codex-session automatically picks the best
account from all registered accounts. The algorithm lives in
`src/services/account/selector.rs` and is adapted from
[caam](https://github.com/Dicklesworthstone/coding_agent_account_manager) ADR
D7.

## Why auto-selection exists

If you have multiple API accounts (A, B, C), each with its own rate limits and
quotas, always using the same one will burn through its quota while the others
sit idle. If that account gets rate-limited (HTTP 429), your session stops. The
auto-selector spreads the load, avoids exhausted accounts, and fails over
automatically.

## Inspecting scores

The scoring algorithm is used internally by `--account auto`, but you can
inspect the scores directly:

```bash
codex-session account quota          # shows rank + total score per account
codex-session account quota --detail # shows every scoring component
codex-session account health         # shows score alongside auth/cooldown status
```

## High-level flow

The algorithm has two phases:

1. **Filter** — disqualify accounts that shouldn't be used right now
2. **Score and rank** — compute a composite score for each remaining account and
  pick the highest

## Phase 1: filtering

### Cooldown check

If an account recently received a 429 "Too Many Requests" response, it enters a
5-minute cooldown. During cooldown the account is skipped entirely. Hammering an
API that just told you to slow down wastes time and may make things worse.

### Quota thresholds

Two minimum bars an account must clear:

| Threshold | Default | Purpose |
|---|---|---|
| 5-hour quota | > 50% | Prevents picking an account that is burning too fast right now |
| Weekly quota | > 10% | Prevents picking an account that is nearly dry for the rest of the week |

Why two? They protect against different failure modes. The 5-hour window catches
"I'm using this too fast right now." The weekly window catches "I've been
draining this all week." An account could pass one but fail the other.

Accounts using API-key mode (no quota tracking) or accounts where the quota
fetch failed are treated as eligible — they skip both gates.

## Phase 2: scoring

Every account that passes filtering gets a composite score:

```
total = health_bonus
      + penalty
      + plan_bonus
      + recency
      + avail_score
      + weekly_pressure_penalty
      + five_hour_pressure_penalty
```

### health_bonus = +100.0

Every account starts with 100 points as a baseline. This keeps all scores
positive and makes the math cleaner. The interesting part is how the other terms
push accounts above or below their peers.

### penalty = 0.0

Currently always zero. This is a reserved slot for future punishment logic (e.g.
accounts with recurring errors, or accounts the user manually deprioritized). It
is in the formula so the scoring structure does not need to change later.

### plan_bonus (0, 20, or 30)

Higher-tier subscription plans have higher rate limits. An account on a Team
plan (30 tokens/min) can sustain more work than a free-tier account (0). By
adding the plan bonus to the score, the algorithm naturally prefers accounts that
can handle heavier loads. The bonus is modest — it won't override everything
else — but when two accounts are otherwise similar, the higher-tier plan wins.

### recency (-30, 0, or +20)

This is the rotation engine. It has three states:

| Situation | Value | Rationale |
|---|---|---|
| Account was the **last one used** (LRU) | -30.0 | Spread the load; don't keep hammering the same account |
| Account hasn't been used in **> 7 days** | +20.0 | It has been resting; quotas are fully reset; reward it |
| Neither | 0.0 | Neutral |

**Why the LRU penalty?** Without it, the same account would win every time
(highest quota stays highest if you keep picking it). The -30 penalty pushes the
selector to rotate. It is like round-robin scheduling with a soft touch — it
doesn't force strict alternation, but it tilts the balance away from the account
you just used.

**Why the idle bonus?** If an account hasn't been touched in a week, its quotas
are fully replenished. It is the freshest option available. The +20 reward says
"this one is well-rested, give it a chance."

**Magnitude choice:** -30 is strong enough to overcome small quota differences
(so rotation actually happens), but not so strong that it forces you onto a
nearly-empty account. +20 is a nice nudge but won't override a genuinely
low-quota account.

### avail_score = five_hour_weight × five_hour% + (1 - five_hour_weight) × weekly% - 50.0

This computes a weighted average of the two quota percentages and subtracts 50.
The default weight is **0.70** for the 5-hour window and **0.30** for weekly.

**Why weighted instead of a simple midpoint?** The 5-hour window is the
practical bottleneck — it drains fast and resets every 5 hours, while the weekly
quota rarely runs out. Giving both windows equal weight masks this asymmetry and
can cause the selector to pick an account with low immediate headroom just
because its weekly looks good. The 70/30 split reflects the ~34× higher reset
frequency of the 5-hour window.

**Why subtract 50?** To center the score around zero. An account with average
health contributes ~0.0 — neither a boost nor a drag. Accounts above average go
positive, below average go negative.

Example values (at default 0.70 weight):

| 5-hour | Weekly | Weighted avg | avail_score |
|---|---|---|---|
| 90% | 80% | 87% | +37.0 |
| 60% | 50% | 57% | +7.0 |
| 45% | 95% | 60% | +10.0 |
| 75% | 65% | 72% | +22.0 |

Note: with equal-weight midpoint, 45/95 and 75/65 would both score +20.0 — a
tie. The weighted formula correctly prefers 75/65 (+22.0 vs +10.0) because its
5-hour headroom is much higher.

Accounts in API-key mode or with unknown quota get 0.0 (neutral).

### weekly_pressure_penalty (0 or -30)

If the weekly quota drops below 20%, an extra -30 is applied.

**Why isn't avail_score enough?** Because avail_score is gradual — it smoothly
decreases as quota drops. But below 20% weekly you are in danger territory. You
might not have enough quota to finish the week. This extra -30 is a hard shove
away from the account, like a "low fuel" warning light. It stacks on top of the
already-negative avail_score, making it very unlikely this account gets picked
unless all alternatives are worse.

**Why 20%?** Below 20% weekly means 80%+ of the week's allowance is consumed
with potentially days remaining. That is a strong signal to preserve what's left.

### five_hour_pressure_penalty (0 or -25)

If the 5-hour quota drops below 15%, an extra -25 is applied.

This is the 5-hour analogue of the weekly pressure penalty. When the 5-hour
window is nearly exhausted, avail_score's weighted formula already penalizes
it more than before (70% weight), but a hard penalty adds urgency at the
critical threshold.

**Why -25 instead of -30?** The weighted avail_score already amplifies the
impact of a low 5-hour percentage. Stacking an equally aggressive penalty would
be excessive. -25 is enough to decisively steer away from the account without
over-penalizing.

**Why 15%?** Slightly lower than the weekly threshold (20%) because avail_score
already weights 5-hour more heavily. By the time 5-hour drops to 15%, the
weighted avail_score has already been dragging the account down; the pressure
penalty is the final push.

## Tie-breaking

When two accounts end up with the same total score, ties are broken in order:

1. **Higher 5-hour quota** — prefer the one with more immediate headroom
2. **Alphabetically first account name** — deterministic and reproducible

## Worked example

Three accounts, all eligible after filtering:

| Component | A (LRU, Pro) | B (Free) | C (Team, idle 10d) |
|---|---|---|---|
| health_bonus | +100 | +100 | +100 |
| plan_bonus | +20 | +0 | +30 |
| recency | -30 (just used) | 0 | +20 (idle > 7d) |
| avail_score | 0.7×80+0.3×60-50 = +24 | 0.7×70+0.3×50-50 = +14 | 0.7×90+0.3×85-50 = +38.5 |
| weekly_pressure | 0 | 0 | 0 |
| five_hour_pressure | 0 | 0 | 0 |
| **Total** | **114** | **114** | **188.5** |

Account C wins by a wide margin: it has been resting (recency +20), it has the
best plan (plan +30), and it has the most quota available (avail +38.5).

A and B tie at 114. Tie-breaker: A has 80% five-hour quota vs B's 70%, so A
takes second place.

Final ranking: **C > A > B**.

## The design philosophy

The scoring system asks five questions about each account:

1. **"Are you alive?"** — health_bonus: yes, you're in the running.
2. **"How capable are you?"** — plan_bonus: higher tier means more capacity.
3. **"Were you just used?"** — recency: spread the load, don't hammer one
  account.
4. **"How full is your tank?"** — avail_score: prefer accounts with more
  remaining quota, weighted 70/30 toward 5-hour headroom.
5. **"Are you dangerously low on weekly?"** — weekly_pressure: emergency weekly
  avoidance.
6. **"Are you dangerously low on 5-hour?"** — five_hour_pressure: emergency
  5-hour avoidance.

Each question addresses a different failure mode: capacity limits, quota
exhaustion, rate-limiting, weekly burnout, and imminent 5-hour exhaustion.
Together they produce a balanced rotation that keeps all accounts healthy over
time.

## Cooldown and retry failover

When `--max-retries > 0` and `--account auto` are both set:

1. The child process runs and hits a 429 error
2. A cooldown file is written with a 5-minute expiry
3. On the next retry, `pick()` skips the cooled-down account
4. The selector picks the next-best eligible account
5. The cooldown auto-expires after 5 minutes

This makes multi-account setups resilient: a 429 on account A transparently
fails over to account B without user intervention.

## Configuration

Defaults in `src/config/mod.rs` (`AccountConfig`):

| Setting | Default | Purpose |
|---|---|---|
| `quota_ttl_secs` | 30 | Cache quota lookups for 30 seconds |
| `weekly_floor` | 10.0 | Minimum weekly quota % to be eligible |
| `five_hour_threshold` | 50.0 | Minimum 5-hour quota % to be eligible |
| `five_hour_weight` | 0.70 | Weight for 5-hour window in avail_score (weekly = 1 - this) |
