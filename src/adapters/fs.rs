//! Filesystem adapter.
//!
//! What this is: the filesystem port used by config loading, merge, and UI
//! support code.
//! What this is not: domain merge logic or config precedence.
#![allow(clippy::missing_errors_doc)]

/// Filesystem-layer failures.
#[derive(Debug, thiserror::Error)]
pub(crate) enum FsError {
    /// Reading a file failed.
    #[error("read failed: {path}: {source}")]
    Read {
        /// Path that was read.
        path: std::path::PathBuf,
        /// Underlying I/O failure.
        #[source]
        source: std::io::Error,
    },

    /// Writing a file failed.
    #[error("write failed: {path}: {source}")]
    Write {
        /// Path that was written.
        path: std::path::PathBuf,
        /// Underlying I/O failure.
        #[source]
        source: std::io::Error,
    },

    /// Creating a directory failed.
    #[error("mkdir failed: {path}: {source}")]
    Mkdir {
        /// Path that was created.
        path: std::path::PathBuf,
        /// Underlying I/O failure.
        #[source]
        source: std::io::Error,
    },

    /// Statting a path failed.
    #[error("stat failed: {path}: {source}")]
    Stat {
        /// Path that was statted.
        path: std::path::PathBuf,
        /// Underlying I/O failure.
        #[source]
        source: std::io::Error,
    },

    /// Touching a file failed.
    #[error("touch failed: {path}: {source}")]
    Touch {
        /// Path that was touched.
        path: std::path::PathBuf,
        /// Underlying I/O failure.
        #[source]
        source: std::io::Error,
    },
}

/// Filesystem operations used by the application.
pub(crate) trait Fs {
    /// Return whether the path exists.
    fn exists(&self, path: &std::path::Path) -> bool;

    /// Read the full file into memory.
    fn read_to_string(&self, path: &std::path::Path) -> Result<String, FsError>;

    /// Return the file modification time.
    fn modified(&self, path: &std::path::Path) -> Result<std::time::SystemTime, FsError>;

    /// Create a directory tree.
    fn create_dir_all(&self, path: &std::path::Path) -> Result<(), FsError>;

    /// Atomically replace `target` with the supplied contents.
    fn write_atomic(&self, target: &std::path::Path, contents: &str) -> Result<(), FsError>;

    /// Mirror bash `touch "$STAMP"` semantics: create if absent, otherwise
    /// only bump atime/mtime to "now" without truncating the existing file.
    fn touch(&self, path: &std::path::Path) -> Result<(), FsError>;
}

/// Real filesystem adapter.
#[derive(Debug, Clone, Copy)]
pub(crate) struct StdFs;

impl Fs for StdFs {
    fn exists(&self, path: &std::path::Path) -> bool {
        // Mirror bash `[[ -f ... ]]`: only regular files count as present.
        // `std::fs::metadata` follows symlinks, so symlinks to regular files
        // still match, but directories, FIFOs, sockets, etc. do not.
        std::fs::metadata(path).is_ok_and(|m| m.is_file())
    }

    fn read_to_string(&self, path: &std::path::Path) -> Result<String, FsError> {
        std::fs::read_to_string(path).map_err(|source| FsError::Read {
            path: path.to_path_buf(),
            source,
        })
    }

    fn modified(&self, path: &std::path::Path) -> Result<std::time::SystemTime, FsError> {
        std::fs::metadata(path)
            .map_err(|source| FsError::Stat {
                path: path.to_path_buf(),
                source,
            })?
            .modified()
            .map_err(|source| FsError::Stat {
                path: path.to_path_buf(),
                source,
            })
    }

    fn create_dir_all(&self, path: &std::path::Path) -> Result<(), FsError> {
        std::fs::create_dir_all(path).map_err(|source| FsError::Mkdir {
            path: path.to_path_buf(),
            source,
        })
    }

    fn write_atomic(&self, target: &std::path::Path, contents: &str) -> Result<(), FsError> {
        use std::io::Write as _;
        use std::os::unix::fs::PermissionsExt as _;

        let Some(dir) = target.parent() else {
            return Err(FsError::Write {
                path: target.to_path_buf(),
                source: std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "target has no parent",
                ),
            });
        };

        self.create_dir_all(dir)?;
        // Match bash parity: `cat "$BASE" >"$tmp"` creates the temp file via
        // shell redirection, which lands at `0o666 & ~umask`. The kernel
        // applies the caller's umask, so a `umask 022` user gets 0o644 and a
        // `umask 077` user gets 0o600. `tempfile::NamedTempFile::new_in`
        // defaults to 0o600 unconditionally, which would tighten
        // `config.toml` for users with the canonical 0o022 umask. Use
        // `Builder::permissions(0o666)` so the kernel applies the user's
        // own umask, preserving bash semantics on every umask setting.
        let mut tmp = tempfile::Builder::new()
            .permissions(std::fs::Permissions::from_mode(0o666))
            .tempfile_in(dir)
            .map_err(|source| FsError::Write {
                path: target.to_path_buf(),
                source,
            })?;
        tmp.write_all(contents.as_bytes())
            .map_err(|source| FsError::Write {
                path: target.to_path_buf(),
                source,
            })?;
        tmp.persist(target).map_err(|err| FsError::Write {
            path: target.to_path_buf(),
            source: err.error,
        })?;
        Ok(())
    }

    fn touch(&self, path: &std::path::Path) -> Result<(), FsError> {
        if let Some(parent) = path.parent() {
            self.create_dir_all(parent)?;
        }
        // Mirror bash `touch "$STAMP"`:
        //   * regular missing path -> create an empty file
        //   * dangling symlink     -> follow the link and create the target
        //                             (bash `touch` follows symlinks and uses
        //                             `open(O_CREAT|O_WRONLY)`, not O_EXCL)
        //   * existing file        -> only bump atime/mtime
        //   * existing dir         -> only bump atime/mtime
        //
        // `Path::exists()` follows symlinks, so it is false for a dangling
        // link. Detect that case via `symlink_metadata` and use plain
        // `create(true)` (no EXCL) so the kernel follows the link and
        // creates the target. Plain `Path::exists()` is the right gate for
        // every other case, because we only want to create when no file
        // resolves at the path.
        if !path.exists() {
            // Plain `create(true)` (no EXCL) so the kernel will follow a
            // dangling symlink and create the target file at the link's
            // destination. We never set `truncate(true)`, so an existing
            // regular file would not be modified — but this branch only
            // runs when no file resolves at `path`.
            std::fs::OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(false)
                .open(path)
                .map_err(|source| FsError::Touch {
                    path: path.to_path_buf(),
                    source,
                })?;
        }
        let now = filetime::FileTime::from_system_time(std::time::SystemTime::now());
        filetime::set_file_times(path, now, now).map_err(|source| FsError::Touch {
            path: path.to_path_buf(),
            source,
        })?;
        Ok(())
    }
}
