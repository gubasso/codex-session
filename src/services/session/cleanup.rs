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

pub(crate) fn prune_stale_sessions_all_accounts(accounts_root: &Utf8Path, max_age: Duration) {
    let Ok(entries) = std::fs::read_dir(accounts_root.as_std_path()) else {
        return;
    };
    for entry in entries.flatten() {
        let Some(name) = entry.file_name().to_str().map(ToOwned::to_owned) else {
            continue;
        };
        if name == ".trash" {
            continue;
        }
        // Refuse to descend through a symlinked account directory: that would
        // let a hostile or careless setup redirect `prune_stale_sessions`
        // (which recursively removes age-eligible children) into arbitrary
        // paths outside the wrapper's session tree.
        let account_path = entry.path();
        let Ok(account_meta) = std::fs::symlink_metadata(&account_path) else {
            continue;
        };
        if account_meta.file_type().is_symlink() || !account_meta.is_dir() {
            continue;
        }
        let Ok(groups) = Utf8PathBuf::try_from(account_path.join("groups")) else {
            continue;
        };
        // Same no-follow check on the `groups/` subdirectory itself before
        // pruning beneath it.
        let Ok(groups_meta) = std::fs::symlink_metadata(groups.as_std_path()) else {
            continue;
        };
        if groups_meta.file_type().is_symlink() || !groups_meta.is_dir() {
            continue;
        }
        prune_stale_sessions(&groups, max_age);
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
    crate::services::config_recipe::composition::write_atomic(marker, "v1\n")
}
