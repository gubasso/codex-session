//! Filesystem-backed account registry.

#![allow(clippy::result_large_err)]

use std::time::SystemTime;

use camino::{Utf8Path, Utf8PathBuf};

use super::{AccountError, AccountId};

pub(crate) struct Registry {
    root: Utf8PathBuf,
    last_account_path: Utf8PathBuf,
}

#[derive(Debug, Clone)]
pub(crate) struct AccountEntry {
    pub(crate) id: AccountId,
    pub(crate) dir: Utf8PathBuf,
    pub(crate) has_auth: bool,
    pub(crate) last_used_at: Option<SystemTime>,
}

impl Registry {
    pub(crate) fn from_config(config: &crate::config::Config) -> Self {
        let root = config
            .account
            .registry_dir
            .clone()
            .unwrap_or_else(|| config.paths.state_dir.join("accounts"));
        let last_account_path = config.paths.state_dir.join("state").join("last-account");
        Self {
            root,
            last_account_path,
        }
    }

    pub(crate) fn account_dir(&self, name: &AccountId) -> Utf8PathBuf {
        self.root.join(name.as_str())
    }

    pub(crate) fn group_auth_seed_path(&self, name: &AccountId) -> Utf8PathBuf {
        self.account_dir(name).join("auth.json")
    }

    pub(crate) fn list(&self) -> Result<Vec<AccountEntry>, AccountError> {
        let mut accounts = Vec::new();
        let entries = match std::fs::read_dir(self.root.as_std_path()) {
            Ok(entries) => entries,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(accounts),
            Err(source) => {
                return Err(AccountError::RegistryIo {
                    path: self.root.clone(),
                    source,
                });
            }
        };

        for entry in entries {
            let entry = entry.map_err(|source| AccountError::RegistryIo {
                path: self.root.clone(),
                source,
            })?;
            let path =
                Utf8PathBuf::try_from(entry.path()).map_err(|err| AccountError::RegistryIo {
                    path: self.root.clone(),
                    source: std::io::Error::new(std::io::ErrorKind::InvalidData, err.to_string()),
                })?;
            let metadata = std::fs::symlink_metadata(path.as_std_path()).map_err(|source| {
                AccountError::RegistryIo {
                    path: path.clone(),
                    source,
                }
            })?;
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                continue;
            }
            let Some(name) = path.file_name() else {
                continue;
            };
            let Ok(id) = name.parse::<AccountId>() else {
                continue;
            };
            accounts.push(AccountEntry {
                has_auth: has_auth(&path)?,
                last_used_at: groups_last_used(&path)?,
                id,
                dir: path,
            });
        }

        accounts.sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));
        Ok(accounts)
    }

    pub(crate) fn add(&self, name: &AccountId) -> Result<AccountEntry, AccountError> {
        crate::services::auth::ensure_owned_dir_0700(&self.root).map_err(map_auth_error)?;
        let dir = self.account_dir(name);
        if dir.as_std_path().exists() {
            return Err(AccountError::AlreadyExists {
                name: name.clone(),
                path: dir,
            });
        }

        crate::services::auth::ensure_owned_dir_0700(&dir).map_err(map_auth_error)?;
        crate::services::auth::ensure_owned_dir_0700(&dir.join("groups"))
            .map_err(map_auth_error)?;

        Ok(AccountEntry {
            id: name.clone(),
            dir: self.account_dir(name),
            has_auth: self.group_auth_seed_path(name).is_file(),
            last_used_at: groups_last_used(&self.account_dir(name))?,
        })
    }

    pub(crate) fn remove(&self, name: &AccountId) -> Result<(), AccountError> {
        let dir = self.expect_account_dir(name)?;
        std::fs::remove_dir_all(dir.as_std_path()).map_err(|source| AccountError::RegistryIo {
            path: dir.clone(),
            source,
        })?;

        // Propagate `current()` errors instead of swallowing them via
        // `.ok().flatten()`. A malformed `state/last-account` should
        // surface as `AppError::Account` rather than leaving the broken
        // pointer in place while reporting success.
        if self.current()?.as_ref() == Some(name) {
            match std::fs::remove_file(self.last_account_path.as_std_path()) {
                Ok(()) => {}
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                Err(source) => {
                    return Err(AccountError::RegistryIo {
                        path: self.last_account_path.clone(),
                        source,
                    });
                }
            }
        }
        Ok(())
    }

    pub(crate) fn delete_auth_seed(&self, name: &AccountId) -> Result<(), AccountError> {
        let seed = self.group_auth_seed_path(name);
        match std::fs::remove_file(seed.as_std_path()) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(source) => Err(AccountError::RegistryIo { path: seed, source }),
        }
    }

    pub(crate) fn delete_group_auths(&self, name: &AccountId) -> Result<(), AccountError> {
        let account_dir = self.expect_account_dir(name)?;
        let groups = account_dir.join("groups");
        let entries = match std::fs::read_dir(groups.as_std_path()) {
            Ok(entries) => entries,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(source) => {
                return Err(AccountError::RegistryIo {
                    path: groups,
                    source,
                });
            }
        };

        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(source) => {
                    tracing::warn!(
                        op = "account.delete_group_auths",
                        outcome = "entry-read-failed",
                        account = %name,
                        error = %source
                    );
                    continue;
                }
            };
            let Ok(path) = Utf8PathBuf::try_from(entry.path()) else {
                continue;
            };
            let Ok(metadata) = std::fs::symlink_metadata(path.as_std_path()) else {
                continue;
            };
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                continue;
            }

            let auth_path = path.join("auth.json");
            if !auth_path.exists() {
                continue;
            }
            if let Err(source) = std::fs::remove_file(auth_path.as_std_path()) {
                tracing::warn!(
                    op = "account.delete_group_auths",
                    outcome = "delete-failed",
                    account = %name,
                    path = %auth_path,
                    error = %source
                );
            }
        }
        Ok(())
    }

    pub(crate) fn current(&self) -> Result<Option<AccountId>, AccountError> {
        match std::fs::read_to_string(self.last_account_path.as_std_path()) {
            Ok(value) => {
                let trimmed = value.trim();
                if trimmed.is_empty() {
                    return Ok(None);
                }
                trimmed
                    .parse::<AccountId>()
                    .map(Some)
                    .map_err(|reason| AccountError::InvalidName {
                        value: trimmed.to_owned(),
                        reason,
                    })
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(source) => Err(AccountError::RegistryIo {
                path: self.last_account_path.clone(),
                source,
            }),
        }
    }

    pub(crate) fn set_current(&self, name: &AccountId) -> Result<(), AccountError> {
        // Reject symlinked or non-directory registry entries up front:
        // `list()` filters these out of enumeration, and the resolver
        // would later fail in session path creation. Catching it at the
        // mutator keeps `state/last-account` consistent with what `list`
        // is willing to surface.
        let _dir = self.expect_account_dir(name)?;
        if let Some(parent) = self.last_account_path.parent() {
            crate::services::auth::ensure_owned_dir_0700(parent).map_err(map_auth_error)?;
        }
        crate::adapters::fs::atomic_write(&self.last_account_path, name.as_str().as_bytes())
            .map_err(map_fs_error)
    }

    /// Validate that `<root>/<name>` is a real directory (not a symlink, not
    /// a non-directory). Returns `NotFound` for absent entries and
    /// `RegistryIo` for symlinked/non-directory entries so callers don't
    /// rely on a bare `.exists()` check that would accept either.
    pub(crate) fn expect_account_dir(&self, name: &AccountId) -> Result<Utf8PathBuf, AccountError> {
        let dir = self.account_dir(name);
        match std::fs::symlink_metadata(dir.as_std_path()) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() || !metadata.is_dir() {
                    return Err(AccountError::RegistryIo {
                        path: dir,
                        source: std::io::Error::other(
                            "account entry is a symlink or not a directory; refusing",
                        ),
                    });
                }
                Ok(dir)
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Err(AccountError::NotFound {
                name: name.clone(),
                path: dir,
            }),
            Err(source) => Err(AccountError::RegistryIo { path: dir, source }),
        }
    }
}

/// Refuse to follow a symlinked `<account>/groups` directory: that would
/// let a hostile setup redirect registry inspection (used by `account list`
/// and `doctor`) into arbitrary paths outside the registry root.
fn open_groups_dir(account_dir: &Utf8Path) -> Result<Option<Utf8PathBuf>, AccountError> {
    let groups = account_dir.join("groups");
    match std::fs::symlink_metadata(groups.as_std_path()) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(AccountError::RegistryIo {
                    path: groups,
                    source: std::io::Error::other(
                        "groups/ is a symlink or not a directory; refusing to follow",
                    ),
                });
            }
            Ok(Some(groups))
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(AccountError::RegistryIo {
            path: groups,
            source,
        }),
    }
}

fn has_auth(account_dir: &Utf8Path) -> Result<bool, AccountError> {
    if account_dir.join("auth.json").is_file() {
        return Ok(true);
    }
    let Some(groups) = open_groups_dir(account_dir)? else {
        return Ok(false);
    };
    let entries =
        std::fs::read_dir(groups.as_std_path()).map_err(|source| AccountError::RegistryIo {
            path: groups.clone(),
            source,
        })?;
    for entry in entries.flatten() {
        let Ok(path) = Utf8PathBuf::try_from(entry.path()) else {
            continue;
        };
        // Reject symlinked group dirs for the same reason as the parent
        // `groups/` check above.
        let Ok(meta) = std::fs::symlink_metadata(path.as_std_path()) else {
            continue;
        };
        if meta.file_type().is_symlink() || !meta.is_dir() {
            continue;
        }
        if path.join("auth.json").is_file() {
            return Ok(true);
        }
    }
    Ok(false)
}

fn groups_last_used(account_dir: &Utf8Path) -> Result<Option<SystemTime>, AccountError> {
    let Some(groups) = open_groups_dir(account_dir)? else {
        return Ok(None);
    };
    let metadata = std::fs::symlink_metadata(groups.as_std_path()).map_err(|source| {
        AccountError::RegistryIo {
            path: groups.clone(),
            source,
        }
    })?;
    metadata
        .modified()
        .map(Some)
        .map_err(|source| AccountError::RegistryIo {
            path: groups,
            source,
        })
}

fn map_auth_error(err: crate::services::auth::AuthError) -> AccountError {
    match err {
        crate::services::auth::AuthError::Io { path, source } => {
            AccountError::RegistryIo { path, source }
        }
        crate::services::auth::AuthError::SymlinkRefused { path } => AccountError::RegistryIo {
            path,
            source: std::io::Error::other("symlink refused"),
        },
        crate::services::auth::AuthError::HardlinkRefused { path } => AccountError::RegistryIo {
            path,
            source: std::io::Error::other("hardlink refused"),
        },
        crate::services::auth::AuthError::BadOwnership { path } => AccountError::RegistryIo {
            path,
            source: std::io::Error::other("bad ownership"),
        },
    }
}

fn map_fs_error(err: crate::adapters::fs::FsError) -> AccountError {
    match err {
        crate::adapters::fs::FsError::Io { path, source } => {
            AccountError::RegistryIo { path, source }
        }
        crate::adapters::fs::FsError::SymlinkRefused { path } => AccountError::RegistryIo {
            path,
            source: std::io::Error::other("symlink refused"),
        },
        crate::adapters::fs::FsError::HardlinkRefused { path } => AccountError::RegistryIo {
            path,
            source: std::io::Error::other("hardlink refused"),
        },
        crate::adapters::fs::FsError::BadOwnership { path, .. } => AccountError::RegistryIo {
            path,
            source: std::io::Error::other("bad ownership"),
        },
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::{AccountId, Registry};

    fn test_config() -> crate::config::Config {
        let temp = tempfile::tempdir().unwrap();
        let base = camino::Utf8PathBuf::try_from(temp.path().to_path_buf()).unwrap();
        crate::config::Config {
            child: crate::config::ChildConfig { bin: None },
            profile: crate::config::ProfileConfig {
                active: None,
                default: None,
                config_dir: base.join("config"),
                profiles_dir: base.join("profiles"),
                settings_dir: base.join("settings"),
            },
            paths: crate::config::PathsConfig {
                cache_dir: base.join("cache"),
                state_dir: base.join("state"),
                runtime_dir: Some(base.join("runtime")),
            },
            log: crate::config::LogConfig::default(),
            account: crate::config::AccountConfig::default(),
            sources: crate::config::ConfigSources::default(),
        }
    }

    #[test]
    fn add_creates_dir() {
        let config = test_config();
        let registry = Registry::from_config(&config);
        let id: AccountId = "work".parse().unwrap();
        registry.add(&id).unwrap();
        assert!(registry.account_dir(&id).is_dir());
    }

    #[test]
    fn add_rejects_duplicate() {
        let config = test_config();
        let registry = Registry::from_config(&config);
        let id: AccountId = "work".parse().unwrap();
        registry.add(&id).unwrap();
        assert!(matches!(
            registry.add(&id),
            Err(super::AccountError::AlreadyExists { .. })
        ));
    }

    #[test]
    fn remove_deletes_permanently() {
        let config = test_config();
        let registry = Registry::from_config(&config);
        let id: AccountId = "work".parse().unwrap();
        registry.add(&id).unwrap();
        registry.remove(&id).unwrap();
        assert!(!registry.account_dir(&id).exists());
    }

    #[test]
    fn remove_clears_lru_if_match() {
        let config = test_config();
        let registry = Registry::from_config(&config);
        let id: AccountId = "work".parse().unwrap();
        registry.add(&id).unwrap();
        registry.set_current(&id).unwrap();
        registry.remove(&id).unwrap();
        assert_eq!(registry.current().unwrap(), None);
    }

    #[test]
    fn current_returns_none_when_pointer_missing() {
        let config = test_config();
        let registry = Registry::from_config(&config);
        assert_eq!(registry.current().unwrap(), None);
    }
}
