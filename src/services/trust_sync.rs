//! Trust-decision sync between session-side codex writes and the
//! machine-local settings layer.
//!
//! What this is: AST-level diff of `[projects]` post-session vs.
//! compose-time baseline, plus a flock-serialized atomic merge into
//! `<cache_dir>/settings.toml`. `cache_dir` is the app-scoped path
//! returned by `directories::ProjectDirs::cache_dir()` — on Linux that
//! resolves to `<XDG_CACHE_HOME>/codex-session/`.
//! What this is not: trust UX, prompting the user, or modifying any
//! stow-managed source file. Codex owns the trust vocabulary
//! (`trust_level = "trusted" | "untrusted"`) and the decision; we relay
//! verbatim.
//!
//! See `docs/upstream-codex.md` for the verified upstream behavior this
//! module mirrors (F1–F8).

#![allow(clippy::result_large_err)]

use std::fs::{File, OpenOptions};
use std::os::unix::fs::OpenOptionsExt as _;

use camino::{Utf8Path, Utf8PathBuf};

use crate::services::auth::{AuthError, ensure_owned_dir_0700, secure_file_read};

/// Result of a single post-flight sync attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TrustSyncOutcome {
    /// No new or changed `[projects]` entries; cache settings untouched.
    Unchanged,
    /// Cache settings updated with `added` brand-new keys and `changed`
    /// updated keys.
    Wrote { added: usize, changed: usize },
}

/// Errors emitted while syncing trust decisions. Kept internal to the
/// service: the caller in `commands::pass_through` log-and-swallows, so we
/// never surface as `AppError`.
#[derive(Debug, thiserror::Error)]
pub(crate) enum TrustSyncError {
    #[error("trust-sync: io error at {path}")]
    Io {
        path: Utf8PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("trust-sync: failed to acquire lock at {path}")]
    LockFailed {
        path: Utf8PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("trust-sync: malformed toml at {path}")]
    TomlParse {
        path: Utf8PathBuf,
        #[source]
        source: toml::de::Error,
    },
    #[error("trust-sync: refused symlink at {path}")]
    SymlinkRefused { path: Utf8PathBuf },
    #[error("trust-sync: refused hardlinked file at {path} (nlink > 1)")]
    HardlinkRefused { path: Utf8PathBuf },
    #[error("trust-sync: bad ownership at {path}")]
    BadOwnership { path: Utf8PathBuf },
    #[error("trust-sync: failed to serialize merged cache settings")]
    TomlSerialize {
        path: Utf8PathBuf,
        #[source]
        source: toml::ser::Error,
    },
}

impl TrustSyncError {
    /// Stable machine-readable error kind. Mirrors `AuthError::kind()` so
    /// structured logs use the same field shape across the wrapper.
    pub(crate) const fn kind(&self) -> &'static str {
        match self {
            Self::Io { .. } => "trust-sync-io",
            Self::LockFailed { .. } => "trust-sync-lock-failed",
            Self::TomlParse { .. } => "trust-sync-toml-parse",
            Self::SymlinkRefused { .. } => "trust-sync-symlink-refused",
            Self::HardlinkRefused { .. } => "trust-sync-hardlink-refused",
            Self::BadOwnership { .. } => "trust-sync-bad-ownership",
            Self::TomlSerialize { .. } => "trust-sync-toml-serialize",
        }
    }

    /// Path the error was reported against (for structured logging).
    pub(crate) fn path(&self) -> &Utf8Path {
        match self {
            Self::Io { path, .. }
            | Self::LockFailed { path, .. }
            | Self::TomlParse { path, .. }
            | Self::SymlinkRefused { path }
            | Self::HardlinkRefused { path }
            | Self::BadOwnership { path }
            | Self::TomlSerialize { path, .. } => path.as_path(),
        }
    }
}

impl TrustSyncError {
    fn from_auth(err: AuthError, _fallback_path: &Utf8Path) -> Self {
        // Map auth's hardened-IO variants to our equivalents. Preserves the
        // path the auth primitive reported on (the cache settings file or its
        // parent directory).
        let owned = err.path().to_path_buf();
        match err {
            AuthError::SymlinkRefused { .. } => Self::SymlinkRefused { path: owned },
            AuthError::HardlinkRefused { .. } => Self::HardlinkRefused { path: owned },
            AuthError::BadOwnership { .. } => Self::BadOwnership { path: owned },
            AuthError::Io { source, .. } => Self::Io {
                path: owned,
                source,
            },
        }
    }

    fn from_fs(err: crate::adapters::fs::FsError, fallback_path: &Utf8Path) -> Self {
        match err {
            crate::adapters::fs::FsError::SymlinkRefused { path } => Self::SymlinkRefused { path },
            crate::adapters::fs::FsError::HardlinkRefused { path } => {
                Self::HardlinkRefused { path }
            }
            crate::adapters::fs::FsError::BadOwnership { path, .. } => Self::BadOwnership { path },
            crate::adapters::fs::FsError::Io { path, source } => {
                let path = if path.as_str().is_empty() {
                    fallback_path.to_path_buf()
                } else {
                    path
                };
                Self::Io { path, source }
            }
        }
    }
}

/// Sync trust decisions written by codex during a session back into the
/// machine-local cache settings layer.
///
/// On Unchanged, the cache file is not opened and the lock is not acquired.
/// On Wrote, the cache file is created if missing (with mode 0o600 in a
/// 0o700 parent directory) and updated under flock.
pub(crate) fn persist_projects(
    session_config: &Utf8Path,
    cache_settings: &Utf8Path,
    baseline: Option<&toml::Table>,
) -> Result<TrustSyncOutcome, TrustSyncError> {
    // 1. Read & parse the post-session config (where codex wrote trust).
    let Some(post_session) = read_post_session_table(session_config)? else {
        return Ok(TrustSyncOutcome::Unchanged);
    };
    let current = extract_projects_table(&post_session);

    // 2. Diff against baseline; return early if nothing changed. Hot path:
    // no lock, no parent-dir ensure, no cache read.
    let delta = diff_projects(baseline, current.as_ref());
    if delta.is_empty() {
        return Ok(TrustSyncOutcome::Unchanged);
    }

    // 3. Ensure parent dir exists with the same ownership/mode contract the
    // auth bridge uses for `~/.codex/`. The parent is `cache_dir` itself
    // (e.g. `<XDG_CACHE_HOME>/codex-session/`).
    let Some(parent) = cache_settings.parent() else {
        return Err(TrustSyncError::Io {
            path: cache_settings.to_path_buf(),
            source: std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "cache settings path has no parent",
            ),
        });
    };
    ensure_owned_dir_0700(parent).map_err(|e| TrustSyncError::from_auth(e, parent))?;

    // 4. Acquire flock on a sibling lockfile, then read-merge-write under
    // the lock. Concurrent terminals serialize here. The closure returns
    // `Some((added, changed))` on a real write or `None` when the merged
    // cache table is byte-equivalent to the pre-existing cache — that lets
    // us collapse "stock-mode re-confirms an already-cached trust entry"
    // into `Unchanged` instead of a redundant fsync/rename round-trip.
    let lockfile = parent.join(".settings.toml.lock");
    let write_outcome = with_lock(
        &lockfile,
        || -> Result<Option<(usize, usize)>, TrustSyncError> {
            let mut cache_table = read_table_or_empty(cache_settings)?;
            let cache_before = cache_table.clone();
            merge_into_cache(&mut cache_table, &delta);
            if cache_table == cache_before {
                return Ok(None);
            }
            // Recount against the actual cache so the log line reflects what
            // changed on disk, not just what changed relative to compose-time
            // baseline. In stock mode (baseline=None) the baseline-based counts
            // would over-report on every session.
            let counts = count_added_changed_vs_cache(&cache_before, &delta);
            let serialized = toml::to_string_pretty(&cache_table).map_err(|source| {
                TrustSyncError::TomlSerialize {
                    path: cache_settings.to_path_buf(),
                    source,
                }
            })?;
            crate::adapters::fs::atomic_write(cache_settings, serialized.as_bytes())
                .map_err(|e| TrustSyncError::from_fs(e, cache_settings))?;
            Ok(Some(counts))
        },
    )?;

    Ok(match write_outcome {
        Some((added, changed)) => TrustSyncOutcome::Wrote { added, changed },
        None => TrustSyncOutcome::Unchanged,
    })
}

/// Parse `<session-dir>/config.toml` into a `toml::Table`. Returns `None`
/// when the file does not exist (stock mode + zero-byte file with no
/// projects table) or is empty — there's nothing to sync.
fn read_post_session_table(path: &Utf8Path) -> Result<Option<toml::Table>, TrustSyncError> {
    let bytes = match std::fs::read(path.as_std_path()) {
        Ok(b) => b,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(TrustSyncError::Io {
                path: path.to_path_buf(),
                source,
            });
        }
    };
    if bytes.is_empty() {
        return Ok(None);
    }
    let text = std::str::from_utf8(&bytes).map_err(|err| TrustSyncError::Io {
        path: path.to_path_buf(),
        source: std::io::Error::new(std::io::ErrorKind::InvalidData, err),
    })?;
    toml::from_str::<toml::Table>(text)
        .map(Some)
        .map_err(|source| TrustSyncError::TomlParse {
            path: path.to_path_buf(),
            source,
        })
}

/// Pull the `[projects]` sub-table out, if present and shaped as a table.
fn extract_projects_table(table: &toml::Table) -> Option<toml::Table> {
    table.get("projects").and_then(|v| v.as_table().cloned())
}

/// Return only the project entries that are present in `current` but absent
/// in `baseline` or whose value differs. Removals are intentionally not
/// propagated in v1 — codex never removes entries during normal operation,
/// and persistent removal is a separate user-driven workflow.
fn diff_projects(baseline: Option<&toml::Table>, current: Option<&toml::Table>) -> toml::Table {
    let Some(current) = current else {
        return toml::Table::new();
    };
    let mut delta = toml::Table::new();
    for (key, value) in current {
        let baseline_value = baseline.and_then(|b| b.get(key));
        if baseline_value != Some(value) {
            delta.insert(key.clone(), value.clone());
        }
    }
    delta
}

/// Count the entries in `delta` against the *current cache table* (not the
/// compose-time baseline). An entry is `added` when the key is absent from
/// `cache_before` and `changed` when it is present but holds a different
/// value. Keys whose value is byte-equivalent to the cache contribute to
/// neither bucket — the read-merge-write step skips the file entirely in
/// that case, so they're invisible to operators reading the trace logs.
fn count_added_changed_vs_cache(cache_before: &toml::Table, delta: &toml::Table) -> (usize, usize) {
    let cache_projects = cache_before.get("projects").and_then(|v| v.as_table());
    let mut added = 0usize;
    let mut changed = 0usize;
    for (key, value) in delta {
        match cache_projects.and_then(|p| p.get(key)) {
            None => added += 1,
            Some(existing) if existing != value => changed += 1,
            Some(_) => {}
        }
    }
    (added, changed)
}

/// Read the cache settings file as a `toml::Table`, or return an empty
/// table when the file does not yet exist. Uses the hardened reader from
/// the auth bridge: `O_NOFOLLOW`, ownership/mode checks, no hardlinks.
fn read_table_or_empty(path: &Utf8Path) -> Result<toml::Table, TrustSyncError> {
    if !path.as_std_path().exists() {
        return Ok(toml::Table::new());
    }
    let bytes = secure_file_read(path).map_err(|e| TrustSyncError::from_auth(e, path))?;
    if bytes.is_empty() {
        return Ok(toml::Table::new());
    }
    let text = std::str::from_utf8(&bytes).map_err(|err| TrustSyncError::Io {
        path: path.to_path_buf(),
        source: std::io::Error::new(std::io::ErrorKind::InvalidData, err),
    })?;
    toml::from_str::<toml::Table>(text).map_err(|source| TrustSyncError::TomlParse {
        path: path.to_path_buf(),
        source,
    })
}

/// Merge `delta` (a projects-table fragment) into the cache root's
/// `[projects]` table. The merge is shallow at the per-project level —
/// each `[projects."<path>"]` entry from `delta` REPLACES the cache entry
/// for the same path verbatim. Unrelated root keys and pre-existing
/// project entries for other paths are preserved.
///
/// Per-entry replace is the intended semantics for v1: codex owns the
/// project-trust vocabulary (`trust_level`, today; possibly more keys in
/// future codex releases). Trusting codex's session-config output as the
/// canonical shape for each project entry guarantees we never strand stale
/// fields from an older codex schema in the cache layer. The cost is that
/// any unrelated key a human added to `[projects."<path>"]` in the cache
/// file would be dropped on the next session — but the cache layer is
/// wrapper-owned (see `docs/upstream-codex.md` "Cache layer is the trust
/// persistence home") and hand-editing it is not supported.
fn merge_into_cache(cache_root: &mut toml::Table, delta: &toml::Table) {
    let projects = cache_root
        .entry("projects".to_owned())
        .or_insert_with(|| toml::Value::Table(toml::Table::new()));
    let toml::Value::Table(projects_table) = projects else {
        // The cache file's `[projects]` was authored as a non-table (very
        // unusual). Replace it with a fresh table containing the delta;
        // refusing to write would strand the user with no way to recover.
        *projects = toml::Value::Table(delta.clone());
        return;
    };
    for (key, value) in delta {
        projects_table.insert(key.clone(), value.clone());
    }
}

/// Trust-sync flavor of the auth bridge's `with_lock`. Re-uses flock
/// semantics (`LOCK_EX`, `O_CLOEXEC`) but yields `TrustSyncError` so the
/// closure body can return our own error type without `from_auth` ceremony
/// on every internal call.
fn with_lock<R, F>(lockfile: &Utf8Path, f: F) -> Result<R, TrustSyncError>
where
    F: FnOnce() -> Result<R, TrustSyncError>,
{
    let file: File = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .custom_flags(libc::O_CLOEXEC)
        .open(lockfile.as_std_path())
        .map_err(|source| TrustSyncError::LockFailed {
            path: lockfile.to_path_buf(),
            source,
        })?;

    rustix::fs::flock(&file, rustix::fs::FlockOperation::LockExclusive).map_err(|errno| {
        TrustSyncError::LockFailed {
            path: lockfile.to_path_buf(),
            source: std::io::Error::from_raw_os_error(errno.raw_os_error()),
        }
    })?;

    let result = f();
    let _ = rustix::fs::flock(&file, rustix::fs::FlockOperation::Unlock);
    result
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::too_many_lines
)]
mod tests {
    use super::*;

    use std::os::unix::fs::PermissionsExt as _;

    fn projects_one(path: &str, level: &str) -> toml::Table {
        let mut inner = toml::Table::new();
        inner.insert(
            "trust_level".to_owned(),
            toml::Value::String(level.to_owned()),
        );
        let mut root = toml::Table::new();
        root.insert(path.to_owned(), toml::Value::Table(inner));
        root
    }

    fn fixture_paths(td: &tempfile::TempDir) -> (Utf8PathBuf, Utf8PathBuf) {
        let base = Utf8PathBuf::from_path_buf(td.path().to_path_buf()).unwrap();
        let session_config = base.join("session/config.toml");
        let cache_settings = base.join("cache/codex-session/settings.toml");
        std::fs::create_dir_all(session_config.parent().unwrap().as_std_path()).unwrap();
        std::fs::create_dir_all(cache_settings.parent().unwrap().as_std_path()).unwrap();
        // The auth-style 0o700 parent contract.
        std::fs::set_permissions(
            cache_settings.parent().unwrap().as_std_path(),
            std::fs::Permissions::from_mode(0o700),
        )
        .unwrap();
        (session_config, cache_settings)
    }

    #[test]
    fn diff_returns_added_changed_unchanged() {
        let baseline = projects_one("/a", "trusted");
        let mut current = projects_one("/a", "trusted");
        current.insert(
            "/b".to_owned(),
            toml::Value::Table(
                projects_one("/b", "untrusted")["/b"]
                    .as_table()
                    .unwrap()
                    .clone(),
            ),
        );
        // Change /a to untrusted.
        current.insert(
            "/a".to_owned(),
            toml::Value::Table({
                let mut t = toml::Table::new();
                t.insert(
                    "trust_level".to_owned(),
                    toml::Value::String("untrusted".to_owned()),
                );
                t
            }),
        );

        let delta = diff_projects(Some(&baseline), Some(&current));
        assert_eq!(
            delta.len(),
            2,
            "delta should include /a (changed) and /b (added)"
        );
        assert!(delta.contains_key("/a"));
        assert!(delta.contains_key("/b"));
    }

    #[test]
    fn diff_empty_when_current_equals_baseline() {
        let baseline = projects_one("/a", "trusted");
        let current = projects_one("/a", "trusted");
        let delta = diff_projects(Some(&baseline), Some(&current));
        assert!(delta.is_empty());
    }

    #[test]
    fn diff_empty_when_current_missing() {
        let baseline = projects_one("/a", "trusted");
        let delta = diff_projects(Some(&baseline), None);
        assert!(delta.is_empty(), "removals are not propagated in v1");
    }

    #[test]
    fn merge_preserves_unrelated_root_keys_and_other_projects() {
        let mut cache = toml::Table::new();
        cache.insert("model".to_owned(), toml::Value::String("gpt-5".to_owned()));
        let mut projects = projects_one("/keep", "trusted");
        projects.insert("/replace".to_owned(), {
            let mut t = toml::Table::new();
            t.insert(
                "trust_level".to_owned(),
                toml::Value::String("trusted".to_owned()),
            );
            toml::Value::Table(t)
        });
        cache.insert("projects".to_owned(), toml::Value::Table(projects));

        let delta = projects_one("/replace", "untrusted");
        merge_into_cache(&mut cache, &delta);

        let projects_after = cache
            .get("projects")
            .and_then(toml::Value::as_table)
            .unwrap();
        assert_eq!(projects_after.len(), 2, "/keep + /replace");
        assert!(projects_after.contains_key("/keep"));
        let replace = projects_after
            .get("/replace")
            .and_then(toml::Value::as_table)
            .unwrap();
        assert_eq!(
            replace.get("trust_level").and_then(toml::Value::as_str),
            Some("untrusted"),
            "delta value overwrites pre-existing key"
        );
        assert_eq!(
            cache.get("model").and_then(toml::Value::as_str),
            Some("gpt-5"),
            "unrelated root keys preserved"
        );
    }

    #[test]
    fn persist_is_noop_when_diff_is_empty() {
        let td = tempfile::tempdir().unwrap();
        let (session, cache) = fixture_paths(&td);
        std::fs::write(
            session.as_std_path(),
            "[projects.\"/a\"]\ntrust_level = \"trusted\"\n",
        )
        .unwrap();
        let baseline = projects_one("/a", "trusted");

        let outcome = persist_projects(&session, &cache, Some(&baseline)).unwrap();
        assert_eq!(outcome, TrustSyncOutcome::Unchanged);
        assert!(
            !cache.as_std_path().exists(),
            "noop must not create the cache file"
        );
        // Lockfile also must not have been created.
        assert!(
            !cache
                .parent()
                .unwrap()
                .join(".settings.toml.lock")
                .as_std_path()
                .exists()
        );
    }

    #[test]
    fn persist_creates_cache_file_when_absent_and_writes_added_entry() {
        let td = tempfile::tempdir().unwrap();
        let (session, cache) = fixture_paths(&td);
        std::fs::write(
            session.as_std_path(),
            "[projects.\"/a\"]\ntrust_level = \"trusted\"\n",
        )
        .unwrap();

        let outcome = persist_projects(&session, &cache, None).unwrap();
        assert_eq!(
            outcome,
            TrustSyncOutcome::Wrote {
                added: 1,
                changed: 0
            }
        );

        let written = std::fs::read_to_string(cache.as_std_path()).unwrap();
        let parsed: toml::Table = toml::from_str(&written).unwrap();
        let project = parsed
            .get("projects")
            .and_then(toml::Value::as_table)
            .unwrap();
        assert_eq!(
            project
                .get("/a")
                .and_then(toml::Value::as_table)
                .and_then(|t| t.get("trust_level"))
                .and_then(toml::Value::as_str),
            Some("trusted")
        );
        let mode = std::fs::metadata(cache.as_std_path())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600, "secure_file_write_atomic chmods to 0o600");
    }

    #[test]
    fn persist_persists_untrusted_too() {
        let td = tempfile::tempdir().unwrap();
        let (session, cache) = fixture_paths(&td);
        std::fs::write(
            session.as_std_path(),
            "[projects.\"/decline\"]\ntrust_level = \"untrusted\"\n",
        )
        .unwrap();

        let outcome = persist_projects(&session, &cache, None).unwrap();
        assert_eq!(
            outcome,
            TrustSyncOutcome::Wrote {
                added: 1,
                changed: 0
            }
        );
        let written = std::fs::read_to_string(cache.as_std_path()).unwrap();
        assert!(written.contains("trust_level = \"untrusted\""));
    }

    #[test]
    fn persist_updates_changed_entry_and_counts_changed() {
        let td = tempfile::tempdir().unwrap();
        let (session, cache) = fixture_paths(&td);
        // Baseline (compose-time) AND the on-disk cache already have
        // /a as trusted. Counts are measured against the cache, so we
        // need to seed both for the assertion to be meaningful.
        let baseline = projects_one("/a", "trusted");
        std::fs::write(
            cache.as_std_path(),
            "[projects.\"/a\"]\ntrust_level = \"trusted\"\n",
        )
        .unwrap();
        std::fs::set_permissions(cache.as_std_path(), std::fs::Permissions::from_mode(0o600))
            .unwrap();
        // Post-session: /a flipped to untrusted (changed), /b added.
        let session_body = concat!(
            "[projects.\"/a\"]\ntrust_level = \"untrusted\"\n",
            "[projects.\"/b\"]\ntrust_level = \"trusted\"\n",
        );
        std::fs::write(session.as_std_path(), session_body).unwrap();

        let outcome = persist_projects(&session, &cache, Some(&baseline)).unwrap();
        assert_eq!(
            outcome,
            TrustSyncOutcome::Wrote {
                added: 1,
                changed: 1
            }
        );
    }

    #[test]
    fn persist_noop_when_cache_already_has_same_trust() {
        // Stock-mode regression: baseline is None, but the cache already
        // contains the entry codex wrote. The merge produces a table
        // identical to what's on disk, so we must skip the rewrite
        // entirely.
        let td = tempfile::tempdir().unwrap();
        let (session, cache) = fixture_paths(&td);
        let body = "[projects.\"/a\"]\ntrust_level = \"trusted\"\n";
        std::fs::write(cache.as_std_path(), body).unwrap();
        std::fs::set_permissions(cache.as_std_path(), std::fs::Permissions::from_mode(0o600))
            .unwrap();
        std::fs::write(session.as_std_path(), body).unwrap();
        let mtime_before = std::fs::metadata(cache.as_std_path())
            .unwrap()
            .modified()
            .unwrap();

        let outcome = persist_projects(&session, &cache, None).unwrap();
        assert_eq!(outcome, TrustSyncOutcome::Unchanged);
        // Cache file mtime must not advance — the merge equality check
        // prevents the atomic-rename round-trip.
        let mtime_after = std::fs::metadata(cache.as_std_path())
            .unwrap()
            .modified()
            .unwrap();
        assert_eq!(mtime_before, mtime_after);
    }

    #[test]
    fn persist_preserves_existing_cache_entries() {
        let td = tempfile::tempdir().unwrap();
        let (session, cache) = fixture_paths(&td);
        // Existing cache content the user must not lose.
        let existing = "model = \"gpt-5\"\n\
                        [projects.\"/keep\"]\ntrust_level = \"trusted\"\n";
        std::fs::write(cache.as_std_path(), existing).unwrap();
        std::fs::set_permissions(cache.as_std_path(), std::fs::Permissions::from_mode(0o600))
            .unwrap();

        std::fs::write(
            session.as_std_path(),
            "[projects.\"/new\"]\ntrust_level = \"trusted\"\n",
        )
        .unwrap();

        let outcome = persist_projects(&session, &cache, None).unwrap();
        assert_eq!(
            outcome,
            TrustSyncOutcome::Wrote {
                added: 1,
                changed: 0
            }
        );
        let parsed: toml::Table =
            toml::from_str(&std::fs::read_to_string(cache.as_std_path()).unwrap()).unwrap();
        assert_eq!(
            parsed.get("model").and_then(toml::Value::as_str),
            Some("gpt-5")
        );
        let projects = parsed
            .get("projects")
            .and_then(toml::Value::as_table)
            .unwrap();
        assert!(projects.contains_key("/keep"));
        assert!(projects.contains_key("/new"));
    }

    #[test]
    fn persist_refuses_symlinked_cache_settings() {
        let td = tempfile::tempdir().unwrap();
        let (session, cache) = fixture_paths(&td);
        // Pre-create a symlink at the cache settings path.
        let target = td.path().join("decoy");
        std::fs::write(&target, "model = \"decoy\"\n").unwrap();
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o600)).unwrap();
        std::os::unix::fs::symlink(&target, cache.as_std_path()).unwrap();
        std::fs::write(
            session.as_std_path(),
            "[projects.\"/a\"]\ntrust_level = \"trusted\"\n",
        )
        .unwrap();

        let err = persist_projects(&session, &cache, None).expect_err("must refuse symlink");
        assert!(matches!(err, TrustSyncError::SymlinkRefused { .. }));
        assert_eq!(err.kind(), "trust-sync-symlink-refused");
        // Decoy target unchanged.
        assert_eq!(
            std::fs::read_to_string(&target).unwrap(),
            "model = \"decoy\"\n"
        );
    }

    #[test]
    fn persist_propagates_malformed_session_toml() {
        let td = tempfile::tempdir().unwrap();
        let (session, cache) = fixture_paths(&td);
        std::fs::write(session.as_std_path(), "[unterminated\n").unwrap();
        let err = persist_projects(&session, &cache, None).expect_err("must propagate parse");
        assert!(matches!(err, TrustSyncError::TomlParse { .. }));
        assert_eq!(err.kind(), "trust-sync-toml-parse");
    }

    #[test]
    fn persist_noop_when_session_config_missing() {
        let td = tempfile::tempdir().unwrap();
        let (session, cache) = fixture_paths(&td);
        // No session config file at all.
        std::fs::remove_file(session.as_std_path()).ok();
        let outcome = persist_projects(&session, &cache, None).unwrap();
        assert_eq!(outcome, TrustSyncOutcome::Unchanged);
    }

    #[test]
    fn persist_noop_when_session_config_empty() {
        let td = tempfile::tempdir().unwrap();
        let (session, cache) = fixture_paths(&td);
        std::fs::write(session.as_std_path(), "").unwrap();
        let outcome = persist_projects(&session, &cache, None).unwrap();
        assert_eq!(outcome, TrustSyncOutcome::Unchanged);
    }

    #[test]
    fn persist_handles_toml_edit_style_session_output() {
        // F7: upstream codex uses `toml_edit` to preserve user formatting.
        // The wrapper's parser must accept comments, blank lines, mixed
        // table styles, and arbitrary spacing without losing trust entries.
        let td = tempfile::tempdir().unwrap();
        let (session, cache) = fixture_paths(&td);
        let body = r#"
# user's pre-existing config
model = "gpt-5"

[ui]
theme = "auto"   # inline comment

# codex appended the trust entry below via toml_edit
[projects."/work/edge case"]
trust_level = "trusted"

[projects."/other"]
trust_level = "untrusted"
"#;
        std::fs::write(session.as_std_path(), body).unwrap();
        let outcome = persist_projects(&session, &cache, None).unwrap();
        assert_eq!(
            outcome,
            TrustSyncOutcome::Wrote {
                added: 2,
                changed: 0
            }
        );
        let parsed: toml::Table =
            toml::from_str(&std::fs::read_to_string(cache.as_std_path()).unwrap()).unwrap();
        let projects = parsed
            .get("projects")
            .and_then(toml::Value::as_table)
            .unwrap();
        assert!(projects.contains_key("/work/edge case"));
        assert!(projects.contains_key("/other"));
    }

    #[test]
    fn error_kinds_are_stable() {
        // Exhaustive matrix lock — kind() strings are part of the log
        // contract for operators grepping $XDG_STATE_HOME logs.
        let p = Utf8PathBuf::from("/tmp/x");
        let cases: &[(TrustSyncError, &str)] = &[
            (
                TrustSyncError::Io {
                    path: p.clone(),
                    source: std::io::Error::from(std::io::ErrorKind::Other),
                },
                "trust-sync-io",
            ),
            (
                TrustSyncError::LockFailed {
                    path: p.clone(),
                    source: std::io::Error::from(std::io::ErrorKind::Other),
                },
                "trust-sync-lock-failed",
            ),
            (
                TrustSyncError::SymlinkRefused { path: p.clone() },
                "trust-sync-symlink-refused",
            ),
            (
                TrustSyncError::HardlinkRefused { path: p.clone() },
                "trust-sync-hardlink-refused",
            ),
            (
                TrustSyncError::BadOwnership { path: p.clone() },
                "trust-sync-bad-ownership",
            ),
        ];
        for (err, expected) in cases {
            assert_eq!(err.kind(), *expected, "kind mismatch for {err:?}");
            assert_eq!(err.path(), p.as_path());
        }
    }

    #[test]
    fn persist_serializes_concurrent_writes() {
        // Spawn N threads, each writes a distinct project key. All keys
        // must be present after every thread exits. Without flock, parallel
        // read-modify-writes would race and lose entries.
        let td = tempfile::tempdir().unwrap();
        let (_session_unused, cache) = fixture_paths(&td);
        let cache_arc = std::sync::Arc::new(cache);

        let n = 8usize;
        let handles: Vec<_> = (0..n)
            .map(|i| {
                let cache = std::sync::Arc::clone(&cache_arc);
                let tmp = td.path().to_path_buf();
                std::thread::spawn(move || {
                    let session_path =
                        Utf8PathBuf::from_path_buf(tmp.join(format!("session-{i}.toml"))).unwrap();
                    std::fs::write(
                        session_path.as_std_path(),
                        format!("[projects.\"/p{i}\"]\ntrust_level = \"trusted\"\n"),
                    )
                    .unwrap();
                    persist_projects(&session_path, &cache, None).unwrap();
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        let parsed: toml::Table =
            toml::from_str(&std::fs::read_to_string(cache_arc.as_std_path()).unwrap()).unwrap();
        let projects = parsed
            .get("projects")
            .and_then(toml::Value::as_table)
            .unwrap();
        for i in 0..n {
            assert!(
                projects.contains_key(&format!("/p{i}")),
                "missing /p{i} after concurrent persist"
            );
        }
    }
}
