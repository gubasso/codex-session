#![allow(clippy::result_large_err)]

use camino::{Utf8Path, Utf8PathBuf};

use super::{AccountError, registry::Registry};

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
pub(crate) struct Cooldown {
    pub reset_at_unix: u64,
    pub reason: String,
    pub last_429_at_unix: u64,
    pub snippet_truncated: String,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum CooldownError {
    #[error(transparent)]
    Fs(#[from] crate::adapters::fs::FsError),
    #[error("cooldown decode failed at {path}: {source}")]
    Decode {
        path: Utf8PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("cooldown encode failed at {path}: {source}")]
    Encode {
        path: Utf8PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("cooldown io failed at {path}: {source}")]
    Io {
        path: Utf8PathBuf,
        #[source]
        source: std::io::Error,
    },
}

fn path(account_root: &Utf8Path) -> Utf8PathBuf {
    account_root.join("cooldown.json")
}

pub(crate) fn read(account_root: &Utf8Path) -> Result<Option<Cooldown>, CooldownError> {
    let path = path(account_root);
    // Use the hardened reader so a symlinked or hardlinked `cooldown.json`
    // cannot redirect the wrapper into attacker-controlled bytes outside
    // the registry root. Matches the protection `atomic_write` already
    // applies on the write side.
    crate::adapters::fs::secure_read(&path)?.map_or(Ok(None), |bytes| {
        serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|source| CooldownError::Decode { path, source })
    })
}

pub(crate) fn write(account_root: &Utf8Path, cooldown: &Cooldown) -> Result<(), CooldownError> {
    let path = path(account_root);
    let bytes = serde_json::to_vec_pretty(cooldown).map_err(|source| CooldownError::Encode {
        path: path.clone(),
        source,
    })?;
    crate::adapters::fs::atomic_write(&path, &bytes)?;
    Ok(())
}

pub(crate) fn clear(account_root: &Utf8Path) -> Result<(), CooldownError> {
    let path = path(account_root);
    match std::fs::remove_file(path.as_std_path()) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(CooldownError::Io { path, source }),
    }
}

pub(crate) fn clear_all(registry: &Registry) -> Result<usize, CooldownError> {
    let entries = registry.list().map_err(|err| match err {
        AccountError::RegistryIo { path, source } => CooldownError::Io { path, source },
        other => CooldownError::Io {
            path: Utf8PathBuf::from("."),
            source: std::io::Error::other(other.to_string()),
        },
    })?;
    let mut removed = 0;
    for entry in entries {
        let cooldown_path = entry.dir.join("cooldown.json");
        match std::fs::remove_file(cooldown_path.as_std_path()) {
            Ok(()) => removed += 1,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(CooldownError::Io {
                    path: cooldown_path,
                    source,
                });
            }
        }
    }
    Ok(removed)
}

pub(crate) const fn is_active(cd: &Cooldown, now_unix: u64) -> bool {
    cd.reset_at_unix > now_unix
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::{Cooldown, clear, clear_all, is_active, read, write};

    fn state_root() -> (tempfile::TempDir, camino::Utf8PathBuf) {
        let temp = tempfile::tempdir().unwrap();
        let root = camino::Utf8PathBuf::try_from(temp.path().to_path_buf()).unwrap();
        std::fs::create_dir_all(root.join("accounts").as_std_path()).unwrap();
        (temp, root)
    }

    fn sample() -> Cooldown {
        Cooldown {
            reset_at_unix: 4_102_444_800,
            reason: "429 detected".to_owned(),
            last_429_at_unix: 4_102_444_500,
            snippet_truncated: "HTTP 429 Too Many Requests".to_owned(),
        }
    }

    fn ensure_account(root: &camino::Utf8Path, name: &str) -> camino::Utf8PathBuf {
        let account_root = root.join("accounts").join(name);
        std::fs::create_dir_all(account_root.as_std_path()).unwrap();
        account_root
    }

    fn test_config(root: &camino::Utf8Path) -> crate::config::Config {
        crate::config::Config {
            child: crate::config::ChildConfig { bin: None },
            config_recipe: crate::config::ConfigRecipeConfig {
                active: None,
                default: None,
                config_dir: root.join("config"),
                recipes_dir: root.join("config-recipes"),
                settings_dir: root.join("settings"),
            },
            paths: crate::config::PathsConfig {
                cache_dir: root.join("cache"),
                state_dir: root.to_path_buf(),
                runtime_dir: Some(root.join("runtime")),
            },
            log: crate::config::LogConfig::default(),
            account: crate::config::AccountConfig::default(),
            sources: crate::config::ConfigSources::default(),
        }
    }

    #[test]
    fn write_then_read_round_trips() {
        let (_temp, root) = state_root();
        let account = ensure_account(&root, "work");
        let cooldown = sample();
        write(&account, &cooldown).unwrap();
        assert_eq!(read(&account).unwrap(), Some(cooldown));
    }

    #[test]
    fn read_missing_returns_none() {
        let (_temp, root) = state_root();
        let account = ensure_account(&root, "work");
        assert_eq!(read(&account).unwrap(), None);
    }

    #[test]
    fn is_active_flips_across_reset_at() {
        let cooldown = sample();
        assert!(is_active(&cooldown, cooldown.reset_at_unix - 1));
        assert!(!is_active(&cooldown, cooldown.reset_at_unix));
        assert!(!is_active(&cooldown, cooldown.reset_at_unix + 1));
    }

    #[test]
    fn clear_removes_one_file_only() {
        let (_temp, root) = state_root();
        let keep_a = ensure_account(&root, "a");
        let remove_b = ensure_account(&root, "b");
        let keep_c = ensure_account(&root, "c");
        let cooldown = sample();
        write(&keep_a, &cooldown).unwrap();
        write(&remove_b, &cooldown).unwrap();
        write(&keep_c, &cooldown).unwrap();

        clear(&remove_b).unwrap();

        assert!(read(&keep_a).unwrap().is_some());
        assert!(read(&remove_b).unwrap().is_none());
        assert!(read(&keep_c).unwrap().is_some());
    }

    #[test]
    fn clear_all_returns_count_removed() {
        let (_temp, root) = state_root();
        let config = test_config(&root);
        let registry = crate::services::account::registry::Registry::from_config(&config);
        let a = ensure_account(&root, "a");
        let b = ensure_account(&root, "b");
        let c = ensure_account(&root, "c");
        let cooldown = sample();
        write(&a, &cooldown).unwrap();
        write(&b, &cooldown).unwrap();
        write(&c, &cooldown).unwrap();

        assert_eq!(clear_all(&registry).unwrap(), 3);
        assert_eq!(clear_all(&registry).unwrap(), 0);
    }
}
