//! Wall-clock helpers.
//!
//! What this is: a single shared reading of the system clock as UNIX seconds,
//! used across account selection, cooldown/quota math, and token-expiry checks.
//! What this is not: monotonic timing, duration formatting (see `ui`), or async
//! scheduling (see `runtime`).

use std::time::{SystemTime, UNIX_EPOCH};

/// Current UNIX time in whole seconds.
///
/// Saturates to `0` if the system clock is set before the UNIX epoch, so callers
/// never have to handle a clock-skew error for a simple "now" reading.
pub(crate) fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}
