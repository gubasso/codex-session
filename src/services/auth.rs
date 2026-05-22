//! Bridge auth.json between native ~/.codex and per-session `CODEX_HOME`.
//!
//! What this is: copy-in/copy-back of upstream codex's auth.json under
//! flock and a top-level `last_refresh` timestamp guard.
//! What this is not: token issuance, refresh, or any cryptography.

pub(crate) mod signal;
pub(crate) mod watcher;

use std::fs::{File, OpenOptions, Permissions};
use std::io::{Read as _, Write as _};
use std::os::unix::fs::{MetadataExt as _, OpenOptionsExt as _, PermissionsExt as _};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use camino::{Utf8Path, Utf8PathBuf};

#[derive(Debug)]
pub(crate) struct AuthBridge {
    #[allow(dead_code)]
    native_dir: Utf8PathBuf,
    native_auth: Utf8PathBuf,
    lockfile: Utf8PathBuf,
    session_auth: Utf8PathBuf,
    seeded_refresh_ts: Option<SystemTime>,
}

impl AuthBridge {
    pub(crate) fn new(home: &Utf8Path, session_dir: &Utf8Path) -> Result<Self, AuthError> {
        let native_dir = home.join(".codex");
        ensure_owned_dir_0700(&native_dir)?;
        Ok(Self {
            native_auth: native_dir.join("auth.json"),
            lockfile: native_dir.join(".auth.json.lock"),
            session_auth: session_dir.join("auth.json"),
            native_dir,
            seeded_refresh_ts: None,
        })
    }

    pub(crate) fn seed_into_session(&mut self) -> Result<(), AuthError> {
        with_lock(&self.lockfile, || {
            if !self.native_auth.exists() {
                self.seeded_refresh_ts = None;
                return Ok(());
            }

            let bytes = secure_file_read(&self.native_auth)?;
            self.seeded_refresh_ts = last_refresh_from_json(&bytes);
            secure_file_write_atomic(&self.session_auth, &bytes)?;
            Ok(())
        })
    }

    pub(crate) fn persist_to_native(&self) -> Result<(), AuthError> {
        if !self.session_auth.exists() {
            tracing::debug!(op = "auth.persist", outcome = "no-session-file");
            return Ok(());
        }

        let bytes = secure_file_read(&self.session_auth)?;
        let session_ts = last_refresh_from_json(&bytes).unwrap_or(UNIX_EPOCH);

        with_lock(&self.lockfile, || {
            let native_ts = if self.native_auth.exists() {
                let native_bytes = secure_file_read(&self.native_auth)?;
                last_refresh_from_json(&native_bytes).unwrap_or(UNIX_EPOCH)
            } else {
                UNIX_EPOCH
            };

            if !self.native_auth.exists() || session_ts > native_ts {
                secure_file_write_atomic(&self.native_auth, &bytes)?;
                tracing::info!(
                    op = "auth.persist",
                    outcome = "wrote",
                    path = %self.native_auth,
                    seeded_refresh_ts = ?self.seeded_refresh_ts
                );
            } else {
                tracing::info!(
                    op = "auth.persist",
                    outcome = "skip-stale",
                    path = %self.native_auth,
                    seeded_refresh_ts = ?self.seeded_refresh_ts
                );
            }
            Ok(())
        })
    }

    fn sync_once(&self) -> Result<SyncOutcome, AuthError> {
        with_lock(&self.lockfile, || {
            let native = read_native_for_sync(&self.native_auth)?;
            let session = read_session_for_sync(&self.session_auth)?;

            match (native, session) {
                (None, SessionRead::Missing | SessionRead::Skip) | (Some(_), SessionRead::Skip) => {
                    Ok(SyncOutcome::Unchanged)
                }
                (None, SessionRead::Parsed { bytes, .. }) => {
                    self.write_native(&bytes)?;
                    Ok(SyncOutcome::WroteNative)
                }
                (Some(native), SessionRead::Missing) => {
                    self.write_session(&native.bytes)?;
                    Ok(SyncOutcome::WroteSession)
                }
                (Some(native), SessionRead::Parsed { bytes, ts }) => match ts.cmp(&native.ts) {
                    std::cmp::Ordering::Greater => {
                        self.write_native(&bytes)?;
                        Ok(SyncOutcome::WroteNative)
                    }
                    std::cmp::Ordering::Less => {
                        self.write_session(&native.bytes)?;
                        Ok(SyncOutcome::WroteSession)
                    }
                    std::cmp::Ordering::Equal => Ok(SyncOutcome::Unchanged),
                },
            }
        })
    }

    fn write_native(&self, bytes: &[u8]) -> Result<(), AuthError> {
        secure_file_write_atomic(&self.native_auth, bytes)?;
        tracing::debug!(
            op = "auth.sync",
            outcome = "wrote-native",
            path = %self.native_auth
        );
        Ok(())
    }

    fn write_session(&self, bytes: &[u8]) -> Result<(), AuthError> {
        secure_file_write_atomic(&self.session_auth, bytes)?;
        tracing::debug!(
            op = "auth.sync",
            outcome = "wrote-session",
            path = %self.session_auth
        );
        Ok(())
    }

    #[allow(dead_code)]
    pub(crate) fn session_auth_path(&self) -> &Utf8Path {
        &self.session_auth
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SyncOutcome {
    WroteNative,
    WroteSession,
    Unchanged,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum AuthError {
    #[error("auth: io error at {path}")]
    Io {
        path: Utf8PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("auth: refused symlink at {path}")]
    SymlinkRefused { path: Utf8PathBuf },
    #[error("auth: refused hardlinked file at {path} (nlink > 1)")]
    HardlinkRefused { path: Utf8PathBuf },
    #[error("auth: bad ownership at {path}")]
    BadOwnership { path: Utf8PathBuf },
    #[error("auth: failed to acquire lock at {path}")]
    LockFailed {
        path: Utf8PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[allow(dead_code)]
    #[error("auth: malformed json at {path}")]
    JsonParse {
        path: Utf8PathBuf,
        #[source]
        source: serde_json::Error,
    },
}

impl AuthError {
    /// Stable machine-readable error kind.
    pub(crate) const fn kind(&self) -> &'static str {
        match self {
            Self::Io { .. } => "auth-io",
            Self::SymlinkRefused { .. } => "auth-symlink-refused",
            Self::HardlinkRefused { .. } => "auth-hardlink-refused",
            Self::BadOwnership { .. } => "auth-bad-ownership",
            Self::LockFailed { .. } => "auth-lock-failed",
            Self::JsonParse { .. } => "auth-json-parse",
        }
    }

    /// Path the error was reported against.
    pub(crate) fn path(&self) -> &Utf8Path {
        match self {
            Self::Io { path, .. }
            | Self::SymlinkRefused { path }
            | Self::HardlinkRefused { path }
            | Self::BadOwnership { path }
            | Self::LockFailed { path, .. }
            | Self::JsonParse { path, .. } => path.as_path(),
        }
    }
}

fn ensure_owned_dir_0700(path: &Utf8Path) -> Result<(), AuthError> {
    if std::fs::symlink_metadata(path.as_std_path()).is_ok_and(|meta| meta.file_type().is_symlink())
    {
        return Err(AuthError::SymlinkRefused {
            path: path.to_path_buf(),
        });
    }

    std::fs::create_dir_all(path.as_std_path()).map_err(|source| AuthError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let metadata =
        std::fs::symlink_metadata(path.as_std_path()).map_err(|source| AuthError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(AuthError::SymlinkRefused {
            path: path.to_path_buf(),
        });
    }
    if metadata.uid() != current_uid() {
        return Err(AuthError::BadOwnership {
            path: path.to_path_buf(),
        });
    }
    std::fs::set_permissions(path.as_std_path(), Permissions::from_mode(0o700)).map_err(
        |source| AuthError::Io {
            path: path.to_path_buf(),
            source,
        },
    )?;
    Ok(())
}

fn secure_file_read(path: &Utf8Path) -> Result<Vec<u8>, AuthError> {
    let mut file = open_nofollow_read(path)?;
    let metadata = file.metadata().map_err(|source| AuthError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    if metadata.nlink() != 1 {
        return Err(AuthError::HardlinkRefused {
            path: path.to_path_buf(),
        });
    }
    if metadata.uid() != current_uid() || metadata.permissions().mode() & 0o077 != 0 {
        return Err(AuthError::BadOwnership {
            path: path.to_path_buf(),
        });
    }

    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|source| AuthError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    Ok(bytes)
}

fn secure_file_write_atomic(target: &Utf8Path, contents: &[u8]) -> Result<(), AuthError> {
    let Some(parent) = target.parent() else {
        return Err(AuthError::Io {
            path: target.to_path_buf(),
            source: std::io::Error::new(std::io::ErrorKind::InvalidInput, "target has no parent"),
        });
    };

    let mut temp =
        tempfile::NamedTempFile::new_in(parent.as_std_path()).map_err(|source| AuthError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    temp.write_all(contents).map_err(|source| AuthError::Io {
        path: target.to_path_buf(),
        source,
    })?;
    temp.as_file_mut()
        .sync_all()
        .map_err(|source| AuthError::Io {
            path: target.to_path_buf(),
            source,
        })?;
    temp.as_file_mut()
        .set_permissions(Permissions::from_mode(0o600))
        .map_err(|source| AuthError::Io {
            path: target.to_path_buf(),
            source,
        })?;

    // Use symlink_metadata (which does NOT follow symlinks) rather than
    // `target.exists()`. The latter returns false for dangling symlinks, so
    // a dangling symlink at `target` would skip validation and silently be
    // renamed over by `persist`. Only treat NotFound as "no destination".
    match std::fs::symlink_metadata(target.as_std_path()) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() {
                return Err(AuthError::SymlinkRefused {
                    path: target.to_path_buf(),
                });
            }
            if metadata.nlink() > 1 {
                return Err(AuthError::HardlinkRefused {
                    path: target.to_path_buf(),
                });
            }
            if metadata.uid() != current_uid() {
                return Err(AuthError::BadOwnership {
                    path: target.to_path_buf(),
                });
            }
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(AuthError::Io {
                path: target.to_path_buf(),
                source,
            });
        }
    }

    // `persist` is an atomic rename into place. A concurrent attacker would
    // need to swap the destination after the validation above and before the
    // rename; that residual race is accepted for this wrapper threat model.
    temp.persist(target.as_std_path())
        .map_err(|err| AuthError::Io {
            path: target.to_path_buf(),
            source: err.error,
        })?;
    Ok(())
}

#[derive(Debug)]
struct NativeAuth {
    bytes: Vec<u8>,
    ts: SystemTime,
}

#[derive(Debug)]
enum SessionRead {
    Missing,
    Parsed {
        bytes: Vec<u8>,
        ts: SystemTime,
    },
    /// Session file exists but its JSON is malformed. The bridge treats
    /// this as "in flight" and skips the tick rather than overwriting
    /// native with the partially-written contents.
    Skip,
}

/// Native is the source of truth: parse errors propagate. A malformed
/// native file is a real problem the user needs to know about, not a
/// transient in-flight write (codex writes native via `tempfile`+rename,
/// which is atomic).
fn read_native_for_sync(path: &Utf8Path) -> Result<Option<NativeAuth>, AuthError> {
    match std::fs::symlink_metadata(path.as_std_path()) {
        Ok(_) => {
            let bytes = secure_file_read(path)?;
            let ts = sync_timestamp_from_json(path, &bytes)?;
            Ok(Some(NativeAuth { bytes, ts }))
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(AuthError::Io {
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn read_session_for_sync(path: &Utf8Path) -> Result<SessionRead, AuthError> {
    match std::fs::symlink_metadata(path.as_std_path()) {
        Ok(_) => {
            let bytes = secure_file_read(path)?;
            match sync_timestamp_from_json(path, &bytes) {
                Ok(ts) => Ok(SessionRead::Parsed { bytes, ts }),
                Err(AuthError::JsonParse { .. }) => Ok(SessionRead::Skip),
                Err(err) => Err(err),
            }
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(SessionRead::Missing),
        Err(source) => Err(AuthError::Io {
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn sync_timestamp_from_json(path: &Utf8Path, bytes: &[u8]) -> Result<SystemTime, AuthError> {
    let value = serde_json::from_slice::<serde_json::Value>(bytes).map_err(|source| {
        AuthError::JsonParse {
            path: path.to_path_buf(),
            source,
        }
    })?;
    Ok(last_refresh_from_value(&value).unwrap_or(UNIX_EPOCH))
}

fn with_lock<R>(
    lockfile: &Utf8Path,
    f: impl FnOnce() -> Result<R, AuthError>,
) -> Result<R, AuthError> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .custom_flags(libc::O_CLOEXEC)
        .open(lockfile.as_std_path())
        .map_err(|source| AuthError::LockFailed {
            path: lockfile.to_path_buf(),
            source,
        })?;

    rustix::fs::flock(&file, rustix::fs::FlockOperation::LockExclusive).map_err(|errno| {
        AuthError::LockFailed {
            path: lockfile.to_path_buf(),
            source: std::io::Error::from_raw_os_error(errno.raw_os_error()),
        }
    })?;

    let result = f();
    let _ = rustix::fs::flock(&file, rustix::fs::FlockOperation::Unlock);
    result
}

// Missing/malformed top-level `last_refresh` returns `None`, which callers
// treat as `UNIX_EPOCH`. This is deliberate: the rollback-protection contract
// is "the newer copy wins, ties skip." Treating a parse failure as epoch lets
// a well-formed session file replace a corrupt native file, instead of
// quarantining the user behind a stale token they can't refresh. The
// `AuthError::JsonParse` variant is reserved for a future strict-mode
// validator that wants to surface parse failures explicitly.
fn last_refresh_from_json(bytes: &[u8]) -> Option<SystemTime> {
    let value = serde_json::from_slice::<serde_json::Value>(bytes).ok()?;
    last_refresh_from_value(&value)
}

fn last_refresh_from_value(value: &serde_json::Value) -> Option<SystemTime> {
    let ts = value.get("last_refresh")?.as_str()?;
    let parsed =
        time::OffsetDateTime::parse(ts, &time::format_description::well_known::Rfc3339).ok()?;
    let nanos = parsed.unix_timestamp_nanos();
    if nanos <= 0 {
        return Some(UNIX_EPOCH);
    }
    Some(UNIX_EPOCH + Duration::from_nanos(u64::try_from(nanos).ok()?))
}

fn open_nofollow_read(path: &Utf8Path) -> Result<File, AuthError> {
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path.as_std_path())
        .map_err(|source| {
            if source.raw_os_error() == Some(libc::ELOOP) {
                AuthError::SymlinkRefused {
                    path: path.to_path_buf(),
                }
            } else {
                AuthError::Io {
                    path: path.to_path_buf(),
                    source,
                }
            }
        })
}

fn current_uid() -> u32 {
    rustix::process::getuid().as_raw()
}

/// Standard native paths derived from `$HOME`. Shared with `doctor` so
/// the wrapper and its health check never disagree on what they're
/// looking at.
pub(crate) fn native_paths(home: &Utf8Path) -> NativePaths {
    let dir = home.join(".codex");
    let auth = dir.join("auth.json");
    NativePaths { dir, auth }
}

#[derive(Debug)]
pub(crate) struct NativePaths {
    pub(crate) dir: Utf8PathBuf,
    pub(crate) auth: Utf8PathBuf,
}

/// Read-only inspection of `~/.codex/{,auth.json}`. The bridge uses this
/// same set of predicates as part of its enforcement; `doctor` consumes
/// the result to render OK/WARN/FAIL without re-implementing the OS
/// checks. Keep the predicates here as the single source of truth.
pub(crate) fn inspect_native_health(home: &Utf8Path) -> NativeHealth {
    let paths = native_paths(home);
    let self_uid = current_uid();

    let dir = match std::fs::symlink_metadata(paths.dir.as_std_path()) {
        Ok(meta) => {
            if meta.file_type().is_symlink() {
                DirHealth::Symlink
            } else if !meta.is_dir() {
                DirHealth::NotDirectory
            } else if meta.uid() != self_uid {
                DirHealth::WrongOwner {
                    uid: meta.uid(),
                    expected: self_uid,
                }
            } else {
                let mode = meta.permissions().mode() & 0o777;
                if mode == 0o700 {
                    DirHealth::OkAt0700
                } else {
                    DirHealth::OkNeedsChmod { mode }
                }
            }
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return NativeHealth {
                paths,
                dir: DirHealth::Missing,
                auth: AuthFileHealth::DirAbsent,
            };
        }
        Err(err) => DirHealth::InspectError(err),
    };

    let dir_usable = matches!(dir, DirHealth::OkAt0700 | DirHealth::OkNeedsChmod { .. });
    let auth = if dir_usable {
        inspect_native_auth_file(&paths.auth, self_uid)
    } else {
        AuthFileHealth::DirAbsent
    };

    NativeHealth { paths, dir, auth }
}

fn inspect_native_auth_file(path: &Utf8Path, self_uid: u32) -> AuthFileHealth {
    let meta = match std::fs::symlink_metadata(path.as_std_path()) {
        Ok(meta) => meta,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return AuthFileHealth::Missing,
        Err(err) => return AuthFileHealth::InspectError(err),
    };
    if meta.file_type().is_symlink() {
        return AuthFileHealth::Symlink;
    }
    if meta.nlink() > 1 {
        return AuthFileHealth::Hardlinked;
    }
    if meta.uid() != self_uid {
        return AuthFileHealth::WrongOwner {
            uid: meta.uid(),
            expected: self_uid,
        };
    }
    let mode = meta.permissions().mode() & 0o777;
    if mode & 0o077 != 0 {
        return AuthFileHealth::BadMode { mode };
    }

    let last_refresh = std::fs::read(path.as_std_path()).ok().and_then(|bytes| {
        serde_json::from_slice::<serde_json::Value>(&bytes)
            .ok()
            .and_then(|value| {
                value
                    .get("last_refresh")
                    .and_then(serde_json::Value::as_str)
                    .map(ToOwned::to_owned)
            })
    });
    AuthFileHealth::Readable { last_refresh }
}

#[derive(Debug)]
pub(crate) struct NativeHealth {
    pub(crate) paths: NativePaths,
    pub(crate) dir: DirHealth,
    pub(crate) auth: AuthFileHealth,
}

#[derive(Debug)]
pub(crate) enum DirHealth {
    Missing,
    Symlink,
    NotDirectory,
    WrongOwner {
        uid: u32,
        expected: u32,
    },
    /// Dir exists and is owned by self; mode is 0o700. The bridge will
    /// accept this as-is.
    OkAt0700,
    /// Dir exists and is owned by self; mode is something else. The
    /// bridge auto-corrects this on first login, so doctor treats it as
    /// a WARN, not a FAIL.
    OkNeedsChmod {
        mode: u32,
    },
    InspectError(std::io::Error),
}

#[derive(Debug)]
pub(crate) enum AuthFileHealth {
    /// Dir was missing or unusable; no point inspecting the file.
    DirAbsent,
    Missing,
    Symlink,
    Hardlinked,
    WrongOwner {
        uid: u32,
        expected: u32,
    },
    BadMode {
        mode: u32,
    },
    Readable {
        last_refresh: Option<String>,
    },
    InspectError(std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    use anyhow::Result;
    use filetime::FileTime;

    fn bridge_fixture() -> Result<(tempfile::TempDir, AuthBridge)> {
        let td = tempfile::tempdir()?;
        let home = Utf8PathBuf::from_path_buf(td.path().join("home"))
            .map_err(|_| anyhow::anyhow!("home path must be utf-8"))?;
        let session = Utf8PathBuf::from_path_buf(td.path().join("session"))
            .map_err(|_| anyhow::anyhow!("session path must be utf-8"))?;
        std::fs::create_dir_all(session.as_std_path())?;
        let bridge = AuthBridge::new(&home, &session)?;
        Ok((td, bridge))
    }

    fn write_auth(path: &Utf8Path, payload: &str) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent.as_std_path())?;
            std::fs::set_permissions(parent.as_std_path(), Permissions::from_mode(0o700))?;
        }
        std::fs::write(path.as_std_path(), payload)?;
        std::fs::set_permissions(path.as_std_path(), Permissions::from_mode(0o600))?;
        Ok(())
    }

    fn payload(ts: &str, token: &str) -> String {
        format!(r#"{{"last_refresh":"{ts}","tokens":{{"access_token":"{token}"}}}}"#)
    }

    #[test]
    fn sync_once_skips_unparseable_session() -> Result<()> {
        let (_td, bridge) = bridge_fixture()?;
        let native = payload("2026-06-01T00:00:00Z", "native");
        write_auth(&bridge.native_auth, &native)?;
        write_auth(&bridge.session_auth, r#"{"tokens":{"#)?;

        let outcome = bridge.sync_once()?;

        assert_eq!(outcome, SyncOutcome::Unchanged);
        assert_eq!(
            std::fs::read_to_string(bridge.native_auth.as_std_path())?,
            native
        );
        Ok(())
    }

    #[test]
    fn sync_once_prefers_newer_native_when_both_parse() -> Result<()> {
        let (_td, bridge) = bridge_fixture()?;
        let native = payload("2026-06-02T00:00:00Z", "native-newer");
        let session = payload("2026-06-01T00:00:00Z", "session-older");
        write_auth(&bridge.native_auth, &native)?;
        write_auth(&bridge.session_auth, &session)?;

        let outcome = bridge.sync_once()?;

        assert_eq!(outcome, SyncOutcome::WroteSession);
        assert_eq!(
            std::fs::read_to_string(bridge.session_auth.as_std_path())?,
            native
        );
        Ok(())
    }

    #[test]
    fn sync_once_prefers_newer_session_when_both_parse() -> Result<()> {
        let (_td, bridge) = bridge_fixture()?;
        let native = payload("2026-06-01T00:00:00Z", "native-older");
        let session = payload("2026-06-02T00:00:00Z", "session-newer");
        write_auth(&bridge.native_auth, &native)?;
        write_auth(&bridge.session_auth, &session)?;

        let outcome = bridge.sync_once()?;

        assert_eq!(outcome, SyncOutcome::WroteNative);
        assert_eq!(
            std::fs::read_to_string(bridge.native_auth.as_std_path())?,
            session
        );
        Ok(())
    }

    #[test]
    fn sync_once_writes_native_when_only_session_present() -> Result<()> {
        let (_td, bridge) = bridge_fixture()?;
        let session = payload("2026-06-01T00:00:00Z", "session-only");
        write_auth(&bridge.session_auth, &session)?;

        let outcome = bridge.sync_once()?;

        assert_eq!(outcome, SyncOutcome::WroteNative);
        assert_eq!(
            std::fs::read_to_string(bridge.native_auth.as_std_path())?,
            session
        );
        Ok(())
    }

    #[test]
    fn sync_once_writes_session_when_only_native_present() -> Result<()> {
        let (_td, bridge) = bridge_fixture()?;
        let native = payload("2026-06-01T00:00:00Z", "native-only");
        write_auth(&bridge.native_auth, &native)?;

        let outcome = bridge.sync_once()?;

        assert_eq!(outcome, SyncOutcome::WroteSession);
        assert_eq!(
            std::fs::read_to_string(bridge.session_auth.as_std_path())?,
            native
        );
        Ok(())
    }

    #[test]
    fn sync_once_unchanged_when_both_missing() -> Result<()> {
        let (_td, bridge) = bridge_fixture()?;

        let outcome = bridge.sync_once()?;

        assert_eq!(outcome, SyncOutcome::Unchanged);
        assert!(!bridge.native_auth.as_std_path().exists());
        assert!(!bridge.session_auth.as_std_path().exists());
        Ok(())
    }

    #[test]
    fn sync_once_unchanged_on_tie() -> Result<()> {
        let (_td, bridge) = bridge_fixture()?;
        let native = payload("2026-06-01T00:00:00Z", "native");
        let session = payload("2026-06-01T00:00:00Z", "session");
        write_auth(&bridge.native_auth, &native)?;
        write_auth(&bridge.session_auth, &session)?;
        let before = FileTime::from_unix_time(1_700_000_000, 0);
        filetime::set_file_mtime(bridge.native_auth.as_std_path(), before)?;

        let outcome = bridge.sync_once()?;
        let after = FileTime::from_last_modification_time(&std::fs::metadata(
            bridge.native_auth.as_std_path(),
        )?);

        assert_eq!(outcome, SyncOutcome::Unchanged);
        assert_eq!(after, before);
        assert_eq!(
            std::fs::read_to_string(bridge.native_auth.as_std_path())?,
            native
        );
        assert_eq!(
            std::fs::read_to_string(bridge.session_auth.as_std_path())?,
            session
        );
        Ok(())
    }
}
