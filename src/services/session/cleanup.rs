//! Best-effort session directory cleanup.
//!
//! What this is: age-based pruning for stale session directories.
//! What this is not: active-session detection or durable retention policy.

use std::time::{Duration, SystemTime};

use camino::{Utf8Path, Utf8PathBuf};

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

pub(crate) fn prune_legacy_pid_dirs(state_root: &Utf8Path, runtime_root: Option<&Utf8Path>) {
    let marker = legacy_prune_marker(state_root);
    if !should_run_legacy_prune(&marker) {
        return;
    }

    let max_age = Duration::from_secs(24 * 3600);
    for legacy in [
        runtime_root.map(|root| root.join("sessions")),
        Some(state_root.join("sessions")),
    ]
    .into_iter()
    .flatten()
    {
        prune_legacy_pid_dirs_in(&legacy, max_age);
    }

    if let Err(err) = mark_legacy_pruned(&marker) {
        tracing::warn!(
            op = "session.legacy_prune.marker",
            outcome = "error",
            path = %marker,
            err = %err
        );
    }
}

fn prune_legacy_pid_dirs_in(dir: &Utf8Path, max_age: Duration) {
    let Ok(entries) = std::fs::read_dir(dir.as_std_path()) else {
        return;
    };
    let now = SystemTime::now();
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|segment| segment.to_str()) else {
            continue;
        };
        if !name.starts_with("pid-") {
            continue;
        }
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.file_type().is_symlink() || !meta.is_dir() {
            continue;
        }
        let Ok(mtime) = meta.modified() else {
            continue;
        };
        let Ok(age) = now.duration_since(mtime) else {
            continue;
        };
        if age > max_age {
            match std::fs::remove_dir_all(&path) {
                Ok(()) => tracing::warn!(
                    op = "session.legacy_prune",
                    outcome = "removed",
                    path = %path.display()
                ),
                Err(err) => tracing::warn!(
                    op = "session.legacy_prune",
                    outcome = "error",
                    path = %path.display(),
                    err = %err
                ),
            }
        }
    }
}

fn legacy_prune_marker(state_root: &Utf8Path) -> Utf8PathBuf {
    state_root.join("state").join(".legacy-pruned")
}

fn should_run_legacy_prune(marker: &Utf8Path) -> bool {
    !marker.as_std_path().exists()
}

#[allow(clippy::result_large_err)]
fn mark_legacy_pruned(marker: &Utf8Path) -> Result<(), crate::config::ConfigError> {
    if let Some(parent) = marker.parent() {
        std::fs::create_dir_all(parent.as_std_path())?;
    }
    crate::services::profile::composition::write_atomic(marker, "v1\n")
}
