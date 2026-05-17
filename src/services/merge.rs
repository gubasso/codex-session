//! Merge orchestration.
#![allow(clippy::missing_errors_doc)]

use crate::adapters::fs::Fs;

/// Mirror bash `__needs_merge` without a BASE existence guard.
pub(crate) fn needs_merge_raw<F: Fs>(
    fs: &F,
    paths: &crate::domain::paths::CodexPaths,
) -> std::io::Result<bool> {
    if !fs.exists(&paths.target) {
        return Ok(true);
    }
    if !fs.exists(&paths.stamp) {
        return Ok(true);
    }

    let base_mtime = fs.modified(&paths.base)?;
    let stamp_mtime = fs.modified(&paths.stamp)?;
    Ok(base_mtime > stamp_mtime)
}

/// Mirror `self config-status` behavior, reporting no merge when BASE is absent.
pub(crate) fn needs_merge_observed<F: Fs>(
    fs: &F,
    paths: &crate::domain::paths::CodexPaths,
) -> std::io::Result<bool> {
    if !fs.exists(&paths.base) {
        return Ok(false);
    }
    needs_merge_raw(fs, paths)
}

/// Perform the merge and update the stamp.
#[tracing::instrument(
    skip(fs),
    fields(base = %paths.base.display(), target = %paths.target.display())
)]
pub(crate) fn perform_merge<F: Fs>(
    fs: &F,
    paths: &crate::domain::paths::CodexPaths,
) -> Result<(), crate::error::AppError> {
    tracing::info!("merging config");
    let base = fs.read_to_string(&paths.base)?;
    let local_sections = if fs.exists(&paths.target) {
        let target = fs.read_to_string(&paths.target)?;
        crate::domain::config_merge::extract_local_sections(&base, &target)
    } else {
        String::new()
    };
    let merged = crate::domain::config_merge::merge_contents(&base, &local_sections);
    fs.write_atomic(&paths.target, &merged)?;
    fs.create_dir_all(&paths.cache_dir)?;
    fs.touch(&paths.stamp)?;
    tracing::debug!(bytes_written = merged.len(), "wrote merged target");
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::collections::{HashMap, HashSet};
    use std::path::{Path, PathBuf};
    use std::time::{Duration, SystemTime};

    use super::{needs_merge_observed, needs_merge_raw};
    use crate::adapters::fs::Fs;
    use crate::domain::paths::CodexPaths;

    #[derive(Default)]
    struct FakeFs {
        exists: HashSet<PathBuf>,
        modified: HashMap<PathBuf, SystemTime>,
    }

    impl FakeFs {
        fn with_exists(mut self, path: &Path) -> Self {
            self.exists.insert(path.to_path_buf());
            self
        }

        fn with_mtime(mut self, path: &Path, time: SystemTime) -> Self {
            self.exists.insert(path.to_path_buf());
            self.modified.insert(path.to_path_buf(), time);
            self
        }
    }

    impl Fs for FakeFs {
        fn exists(&self, path: &Path) -> bool {
            self.exists.contains(path)
        }

        fn read_to_string(&self, _path: &Path) -> std::io::Result<String> {
            unimplemented!()
        }

        fn modified(&self, path: &Path) -> std::io::Result<SystemTime> {
            self.modified
                .get(path)
                .copied()
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "missing mtime"))
        }

        fn create_dir_all(&self, _path: &Path) -> std::io::Result<()> {
            unimplemented!()
        }

        fn write_atomic(&self, _target: &Path, _contents: &str) -> std::io::Result<()> {
            unimplemented!()
        }

        fn touch(&self, _path: &Path) -> std::io::Result<()> {
            unimplemented!()
        }
    }

    fn sample_paths() -> CodexPaths {
        CodexPaths {
            base: PathBuf::from("/tmp/base.toml"),
            target: PathBuf::from("/tmp/target.toml"),
            cache_dir: PathBuf::from("/tmp/cache"),
            stamp: PathBuf::from("/tmp/cache/last-merge"),
        }
    }

    #[test]
    fn observed_merge_is_no_when_base_missing() {
        let paths = sample_paths();
        let fs = FakeFs::default();
        assert!(!needs_merge_observed(&fs, &paths).unwrap());
    }

    #[test]
    fn raw_merge_is_yes_when_target_missing() {
        let paths = sample_paths();
        let fs = FakeFs::default().with_exists(&paths.base);
        assert!(needs_merge_raw(&fs, &paths).unwrap());
    }

    #[test]
    fn raw_merge_is_yes_when_stamp_missing() {
        let paths = sample_paths();
        let fs = FakeFs::default()
            .with_exists(&paths.base)
            .with_exists(&paths.target);
        assert!(needs_merge_raw(&fs, &paths).unwrap());
    }

    #[test]
    fn raw_merge_is_yes_when_base_is_newer() {
        let paths = sample_paths();
        let base_time = SystemTime::UNIX_EPOCH + Duration::from_secs(2);
        let stamp_time = SystemTime::UNIX_EPOCH + Duration::from_secs(1);
        let fs = FakeFs::default()
            .with_mtime(&paths.base, base_time)
            .with_mtime(&paths.target, stamp_time)
            .with_mtime(&paths.stamp, stamp_time);
        assert!(needs_merge_raw(&fs, &paths).unwrap());
    }

    #[test]
    fn raw_merge_is_no_when_base_is_older() {
        let paths = sample_paths();
        let base_time = SystemTime::UNIX_EPOCH + Duration::from_secs(1);
        let stamp_time = SystemTime::UNIX_EPOCH + Duration::from_secs(2);
        let fs = FakeFs::default()
            .with_mtime(&paths.base, base_time)
            .with_mtime(&paths.target, stamp_time)
            .with_mtime(&paths.stamp, stamp_time);
        assert!(!needs_merge_raw(&fs, &paths).unwrap());
    }

    #[test]
    fn raw_merge_is_no_when_times_are_equal() {
        let paths = sample_paths();
        let shared_time = SystemTime::UNIX_EPOCH + Duration::from_secs(2);
        let fs = FakeFs::default()
            .with_mtime(&paths.base, shared_time)
            .with_mtime(&paths.target, shared_time)
            .with_mtime(&paths.stamp, shared_time);
        assert!(!needs_merge_raw(&fs, &paths).unwrap());
    }
}
