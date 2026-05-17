//! Filesystem adapter.
#![allow(clippy::missing_errors_doc)]

/// Filesystem operations used by the application.
pub(crate) trait Fs {
    /// Return whether the path exists.
    fn exists(&self, path: &std::path::Path) -> bool;

    /// Read the full file into memory.
    fn read_to_string(&self, path: &std::path::Path) -> std::io::Result<String>;

    /// Return the file modification time.
    fn modified(&self, path: &std::path::Path) -> std::io::Result<std::time::SystemTime>;

    /// Create a directory tree.
    fn create_dir_all(&self, path: &std::path::Path) -> std::io::Result<()>;

    /// Atomically replace `target` with the supplied contents.
    fn write_atomic(&self, target: &std::path::Path, contents: &str) -> std::io::Result<()>;

    /// Mirror bash `: > "$STAMP"` semantics.
    fn touch(&self, path: &std::path::Path) -> std::io::Result<()>;
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

    fn read_to_string(&self, path: &std::path::Path) -> std::io::Result<String> {
        std::fs::read_to_string(path)
    }

    fn modified(&self, path: &std::path::Path) -> std::io::Result<std::time::SystemTime> {
        std::fs::metadata(path)?.modified()
    }

    fn create_dir_all(&self, path: &std::path::Path) -> std::io::Result<()> {
        std::fs::create_dir_all(path)
    }

    fn write_atomic(&self, target: &std::path::Path, contents: &str) -> std::io::Result<()> {
        use std::io::Write as _;

        let Some(dir) = target.parent() else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "target has no parent",
            ));
        };

        self.create_dir_all(dir)?;
        let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
        tmp.write_all(contents.as_bytes())?;
        tmp.persist(target).map_err(|err| err.error)?;
        Ok(())
    }

    fn touch(&self, path: &std::path::Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            self.create_dir_all(parent)?;
        }
        std::fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(path)?;
        Ok(())
    }
}
