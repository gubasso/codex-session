//! Read-only inspection of native auth state for `doctor`.

use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};

use camino::{Utf8Path, Utf8PathBuf};

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
    WrongOwner { uid: u32, expected: u32 },
    OkAt0700,
    OkNeedsChmod { mode: u32 },
    InspectError(std::io::Error),
}

#[derive(Debug)]
pub(crate) enum AuthFileHealth {
    DirAbsent,
    Missing,
    Symlink,
    Hardlinked,
    WrongOwner { uid: u32, expected: u32 },
    BadMode { mode: u32 },
    Readable { last_refresh: Option<String> },
    InspectError(std::io::Error),
}

fn current_uid() -> u32 {
    rustix::process::getuid().as_raw()
}
