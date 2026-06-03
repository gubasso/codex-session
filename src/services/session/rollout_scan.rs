use camino::{Utf8Path, Utf8PathBuf};

use crate::services::account::AccountId;

pub(crate) struct RolloutOwner {
    pub(crate) account: AccountId,
    pub(crate) group_id: String,
    pub(crate) session_dir: Utf8PathBuf,
    pub(crate) rollout_path: Utf8PathBuf,
}

const MAX_FALLBACK_FILES: usize = 64;
const MAX_FALLBACK_BYTES: usize = 16 * 1024;

pub(crate) fn find_owner(
    root: &Utf8Path,
    account_ids: &[AccountId],
    thread_id: &str,
) -> Option<RolloutOwner> {
    // The content-fallback budget is shared across the whole walk so a large
    // multi-account/multi-group tree cannot multiply the scan cost.
    let mut fallback_budget = MAX_FALLBACK_FILES;
    for account in account_ids {
        let groups_dir = root.join("accounts").join(account.as_str()).join("groups");
        let entries = match std::fs::read_dir(groups_dir.as_std_path()) {
            Ok(entries) => entries,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => {
                tracing::debug!(
                    op = "rollout_scan.read_groups",
                    account = %account,
                    err = %err
                );
                continue;
            }
        };

        for entry in entries.flatten() {
            let Ok(group_path) = Utf8PathBuf::try_from(entry.path()) else {
                continue;
            };
            let Some(group_id) = group_path.file_name().map(ToOwned::to_owned) else {
                continue;
            };
            if let Some(owner) =
                scan_group(root, account, &group_id, thread_id, &mut fallback_budget)
            {
                return Some(owner);
            }
        }
    }
    None
}

pub(crate) fn owner_has_thread(
    root: &Utf8Path,
    account: &AccountId,
    group_id: &str,
    thread_id: &str,
) -> bool {
    let mut fallback_budget = MAX_FALLBACK_FILES;
    scan_group(root, account, group_id, thread_id, &mut fallback_budget).is_some()
}

fn scan_group(
    root: &Utf8Path,
    account: &AccountId,
    group_id: &str,
    thread_id: &str,
    fallback_budget: &mut usize,
) -> Option<RolloutOwner> {
    let session_dir = root
        .join("accounts")
        .join(account.as_str())
        .join("groups")
        .join(group_id);
    let sessions_root = session_dir.join("sessions");
    if !sessions_root.exists() {
        return None;
    }

    walk_sessions(&sessions_root, &mut |path| {
        let name = path.file_name()?;
        if !name.starts_with("rollout-")
            || !std::path::Path::new(name)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("jsonl"))
        {
            return None;
        }
        if filename_matches_thread(name, thread_id) {
            return Some(RolloutOwner {
                account: account.clone(),
                group_id: group_id.to_owned(),
                session_dir: session_dir.clone(),
                rollout_path: path,
            });
        }
        if *fallback_budget == 0 {
            return None;
        }
        *fallback_budget -= 1;
        file_contains_thread_id(&path, thread_id).then(|| RolloutOwner {
            account: account.clone(),
            group_id: group_id.to_owned(),
            session_dir: session_dir.clone(),
            rollout_path: path,
        })
    })
}

fn walk_sessions(
    sessions_root: &Utf8Path,
    on_file: &mut impl FnMut(Utf8PathBuf) -> Option<RolloutOwner>,
) -> Option<RolloutOwner> {
    let years = match std::fs::read_dir(sessions_root.as_std_path()) {
        Ok(entries) => entries,
        Err(err) => {
            tracing::debug!(
                op = "rollout_scan.read_years",
                path = %sessions_root,
                err = %err
            );
            return None;
        }
    };

    for year in years.flatten() {
        let Ok(year_path) = Utf8PathBuf::try_from(year.path()) else {
            continue;
        };
        let months = match std::fs::read_dir(year_path.as_std_path()) {
            Ok(entries) => entries,
            Err(err) => {
                tracing::debug!(op = "rollout_scan.read_months", path = %year_path, err = %err);
                continue;
            }
        };
        for month in months.flatten() {
            let Ok(month_path) = Utf8PathBuf::try_from(month.path()) else {
                continue;
            };
            let days = match std::fs::read_dir(month_path.as_std_path()) {
                Ok(entries) => entries,
                Err(err) => {
                    tracing::debug!(op = "rollout_scan.read_days", path = %month_path, err = %err);
                    continue;
                }
            };
            for day in days.flatten() {
                let Ok(day_path) = Utf8PathBuf::try_from(day.path()) else {
                    continue;
                };
                let files = match std::fs::read_dir(day_path.as_std_path()) {
                    Ok(entries) => entries,
                    Err(err) => {
                        tracing::debug!(
                            op = "rollout_scan.read_files",
                            path = %day_path,
                            err = %err
                        );
                        continue;
                    }
                };
                for file in files.flatten() {
                    let Ok(file_path) = Utf8PathBuf::try_from(file.path()) else {
                        continue;
                    };
                    if let Some(owner) = on_file(file_path) {
                        return Some(owner);
                    }
                }
            }
        }
    }
    None
}

/// Exact filename match. Codex names rollouts `rollout-<date>-<session-uuid>.jsonl`
/// where `<session-uuid>` is the thread id. The id must be the complete trailing
/// segment (preceded by `-`), so a truncated/mistyped id cannot substring-match a
/// longer, unrelated UUID and recover the wrong owner.
fn filename_matches_thread(name: &str, thread_id: &str) -> bool {
    let Some(stem) = name
        .strip_suffix(".jsonl")
        .or_else(|| name.strip_suffix(".JSONL"))
    else {
        return false;
    };
    stem.strip_suffix(thread_id)
        .is_some_and(|prefix| prefix.ends_with('-'))
}

fn file_contains_thread_id(path: &Utf8Path, thread_id: &str) -> bool {
    let Ok(metadata) = std::fs::metadata(path.as_std_path()) else {
        tracing::debug!(op = "rollout_scan.metadata", path = %path, "metadata failed");
        return false;
    };
    let max = u64::try_from(MAX_FALLBACK_BYTES).unwrap_or(u64::MAX);
    if metadata.len() > max {
        return false;
    }
    match std::fs::read_to_string(path.as_std_path()) {
        // Match the id only as a complete JSON string value (`"<id>"`), not as a
        // raw substring, so a partial id cannot match inside a longer field.
        Ok(text) => text.contains(&format!("\"{thread_id}\"")),
        Err(err) => {
            tracing::warn!(op = "rollout_scan.read_file", path = %path, err = %err);
            false
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn utf8(path: std::path::PathBuf) -> Utf8PathBuf {
        Utf8PathBuf::try_from(path).unwrap()
    }

    fn plant_rollout(
        root: &Utf8Path,
        account: &str,
        group: &str,
        file: &str,
        body: &str,
    ) -> Utf8PathBuf {
        let path = root
            .join("accounts")
            .join(account)
            .join("groups")
            .join(group)
            .join("sessions/2026/06/03")
            .join(file);
        std::fs::create_dir_all(path.parent().unwrap().as_std_path()).unwrap();
        std::fs::write(path.as_std_path(), body).unwrap();
        path
    }

    #[test]
    fn finds_owner_by_filename_match() {
        let tmp = tempfile::tempdir().unwrap();
        let root = utf8(tmp.path().to_path_buf());
        let thread_id = "thread-hit";
        let path = plant_rollout(
            &root,
            "work",
            "stable",
            &format!("rollout-2026-06-03-{thread_id}.jsonl"),
            "{}\n",
        );
        plant_rollout(&root, "personal", "stable", "rollout-else.jsonl", "{}\n");

        let owner = find_owner(
            &root,
            &["personal".parse().unwrap(), "work".parse().unwrap()],
            thread_id,
        )
        .unwrap();
        assert_eq!(owner.account.as_str(), "work");
        assert_eq!(owner.group_id, "stable");
        assert_eq!(owner.rollout_path, path);
    }

    #[test]
    fn returns_none_on_miss() {
        let tmp = tempfile::tempdir().unwrap();
        let root = utf8(tmp.path().to_path_buf());
        plant_rollout(&root, "work", "stable", "rollout-other.jsonl", "{}\n");
        assert!(find_owner(&root, &["work".parse().unwrap()], "missing").is_none());
    }

    #[test]
    fn content_fallback_finds_thread() {
        let tmp = tempfile::tempdir().unwrap();
        let root = utf8(tmp.path().to_path_buf());
        plant_rollout(
            &root,
            "work",
            "stable",
            "rollout-other.jsonl",
            "{\"thread_id\":\"content-thread\"}\n",
        );
        let owner = find_owner(&root, &["work".parse().unwrap()], "content-thread").unwrap();
        assert_eq!(owner.account.as_str(), "work");
    }

    #[test]
    fn unreadable_or_malformed_dirs_are_skipped() {
        let tmp = tempfile::tempdir().unwrap();
        let root = utf8(tmp.path().to_path_buf());
        let bad = root.join("accounts/work/groups/stable/sessions/2026/06");
        std::fs::create_dir_all(bad.as_std_path()).unwrap();
        std::fs::write(bad.join("03").as_std_path(), "not a dir").unwrap();
        assert!(find_owner(&root, &["work".parse().unwrap()], "missing").is_none());
    }

    #[test]
    fn truncated_id_does_not_substring_match_filename() {
        // A mistyped/truncated id that is a substring of a real UUID must NOT
        // recover the unrelated owner.
        let tmp = tempfile::tempdir().unwrap();
        let root = utf8(tmp.path().to_path_buf());
        let full = "019e8e4f-8428-70d3-8f18-d263b73f739d";
        plant_rollout(
            &root,
            "work",
            "stable",
            &format!("rollout-2026-06-03-{full}.jsonl"),
            "{}\n",
        );
        let accounts = ["work".parse().unwrap()];
        // Truncated prefix of the real UUID: rejected.
        assert!(find_owner(&root, &accounts, "019e8e4f-8428").is_none());
        // A bare interior fragment: rejected.
        assert!(find_owner(&root, &accounts, "70d3").is_none());
        // The exact id: still recovered.
        assert!(find_owner(&root, &accounts, full).is_some());
    }

    #[test]
    fn content_fallback_rejects_partial_id() {
        // The fallback must match a complete quoted JSON value, not a substring.
        let tmp = tempfile::tempdir().unwrap();
        let root = utf8(tmp.path().to_path_buf());
        plant_rollout(
            &root,
            "work",
            "stable",
            "rollout-other.jsonl",
            "{\"thread_id\":\"019e8e4f-full-uuid-value\"}\n",
        );
        let accounts = ["work".parse().unwrap()];
        assert!(find_owner(&root, &accounts, "019e8e4f").is_none());
        assert!(find_owner(&root, &accounts, "019e8e4f-full-uuid-value").is_some());
    }

    #[test]
    fn owner_has_thread_checks_single_group() {
        let tmp = tempfile::tempdir().unwrap();
        let root = utf8(tmp.path().to_path_buf());
        plant_rollout(
            &root,
            "work",
            "stable",
            "rollout-2026-06-03-owned.jsonl",
            "{}\n",
        );
        let account: AccountId = "work".parse().unwrap();
        assert!(owner_has_thread(&root, &account, "stable", "owned"));
        assert!(!owner_has_thread(&root, &account, "other", "owned"));
    }
}
