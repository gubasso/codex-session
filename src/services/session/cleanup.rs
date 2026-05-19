//! Best-effort session directory cleanup.
//!
//! What this is: age-based pruning for stale session directories.
//! What this is not: active-session detection or durable retention policy.

use std::time::{Duration, SystemTime};

use camino::Utf8Path;

pub(crate) fn prune_stale_sessions(sessions_root: &Utf8Path, max_age: Duration) {
    let Ok(entries) = std::fs::read_dir(sessions_root.as_std_path()) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let metadata = match std::fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(err) => {
                tracing::warn!(
                    op = "session.prune",
                    outcome = "error",
                    path = %path.display(),
                    err = %err
                );
                continue;
            }
        };
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            continue;
        }

        let modified = match metadata.modified() {
            Ok(modified) => modified,
            Err(err) => {
                tracing::warn!(
                    op = "session.prune",
                    outcome = "error",
                    path = %path.display(),
                    err = %err
                );
                continue;
            }
        };
        let Ok(age) = SystemTime::now().duration_since(modified) else {
            continue;
        };

        if age > max_age {
            match std::fs::remove_dir_all(&path) {
                Ok(()) => tracing::warn!(
                    op = "session.prune",
                    outcome = "removed",
                    path = %path.display()
                ),
                Err(err) => tracing::warn!(
                    op = "session.prune",
                    outcome = "error",
                    path = %path.display(),
                    err = %err
                ),
            }
        }
    }
}
