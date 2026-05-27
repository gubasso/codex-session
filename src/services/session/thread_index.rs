//! Cross-account thread index (JSONL append log).
//!
//! What this is: persistent append-only map from thread-id to originating
//! account / group, enabling `resume <ID>` to resolve the correct
//! `CODEX_HOME` regardless of `--account auto` selection.
//! What this is not: a query-optimized database; O(n) per lookup is
//! acceptable for round 01.

use camino::{Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write as _};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct ThreadEntry {
    pub(crate) thread_id: String,
    pub(crate) account: String,
    pub(crate) group_id: String,
    pub(crate) cwd: Utf8PathBuf,
    pub(crate) created_at: String,
}

pub(crate) fn index_path(state_dir: &Utf8Path) -> Utf8PathBuf {
    state_dir.join("thread-index.jsonl")
}

pub(crate) fn append(state_dir: &Utf8Path, entry: &ThreadEntry) -> std::io::Result<()> {
    let path = index_path(state_dir);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent.as_std_path())?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path.as_std_path())?;
    let json = serde_json::to_string(entry)?;
    writeln!(file, "{json}")?;
    Ok(())
}

pub(crate) fn extract_thread_id(stdout: &[u8]) -> Option<String> {
    for line in stdout.split(|b| *b == b'\n') {
        if line.is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(line) else {
            continue;
        };
        if value.get("type").and_then(|v| v.as_str()) == Some("thread.started")
            && let Some(tid) = value.get("thread_id").and_then(|v| v.as_str())
        {
            return Some(tid.to_owned());
        }
    }
    None
}

pub(crate) fn utc_now_rfc3339() -> String {
    super::time_util::utc_now_rfc3339()
}

fn read_entries(state_dir: &Utf8Path) -> std::io::Result<Vec<ThreadEntry>> {
    let path = index_path(state_dir);
    let file = match std::fs::File::open(path.as_std_path()) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut entries = Vec::new();
    for line in BufReader::new(file).lines() {
        let line = line?;
        if line.is_empty() {
            continue;
        }
        if let Ok(entry) = serde_json::from_str::<ThreadEntry>(&line) {
            entries.push(entry);
        }
    }
    Ok(entries)
}

pub(crate) fn lookup(
    state_dir: &Utf8Path,
    thread_id: &str,
) -> std::io::Result<Option<ThreadEntry>> {
    let entries = read_entries(state_dir)?;
    Ok(entries.into_iter().rfind(|e| e.thread_id == thread_id))
}

pub(crate) fn last_for_group(
    state_dir: &Utf8Path,
    group_id: &str,
) -> std::io::Result<Option<ThreadEntry>> {
    let entries = read_entries(state_dir)?;
    Ok(entries.into_iter().rfind(|e| e.group_id == group_id))
}

pub(crate) fn last_any(state_dir: &Utf8Path) -> std::io::Result<Option<ThreadEntry>> {
    let entries = read_entries(state_dir)?;
    Ok(entries.into_iter().last())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn make_entry(thread_id: &str, account: &str, group_id: &str) -> ThreadEntry {
        ThreadEntry {
            thread_id: thread_id.to_owned(),
            account: account.to_owned(),
            group_id: group_id.to_owned(),
            cwd: Utf8PathBuf::from("/tmp/test"),
            created_at: "2025-01-01T00:00:00Z".to_owned(),
        }
    }

    fn state_dir() -> (tempfile::TempDir, Utf8PathBuf) {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = Utf8PathBuf::try_from(tmp.path().to_path_buf()).expect("utf8");
        (tmp, path)
    }

    #[test]
    fn append_creates_file_and_valid_json_line() {
        let (_tmp, dir) = state_dir();
        let entry = make_entry("t1", "acc1", "g1");

        append(&dir, &entry).expect("append");

        let path = index_path(&dir);
        let text = std::fs::read_to_string(path.as_std_path()).expect("read file");
        let mut lines = text.lines();
        let line = lines.next().expect("first line");
        assert!(lines.next().is_none());

        let parsed: ThreadEntry = serde_json::from_str(line).expect("valid json");
        assert_eq!(parsed.thread_id, "t1");
        assert_eq!(parsed.account, "acc1");
        assert_eq!(parsed.group_id, "g1");
    }

    #[test]
    fn append_appends_without_overwrite() {
        let (_tmp, dir) = state_dir();
        append(&dir, &make_entry("t1", "acc1", "g1")).expect("append 1");
        append(&dir, &make_entry("t2", "acc2", "g2")).expect("append 2");

        let path = index_path(&dir);
        let text = std::fs::read_to_string(path.as_std_path()).expect("read file");
        assert_eq!(text.lines().count(), 2);

        let entries = read_entries(&dir).expect("read entries");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].thread_id, "t1");
        assert_eq!(entries[1].thread_id, "t2");
    }

    #[test]
    fn lookup_finds_by_thread_id() {
        let (_tmp, dir) = state_dir();
        append(&dir, &make_entry("t1", "acc1", "g1")).expect("append 1");
        append(&dir, &make_entry("t2", "acc2", "g2")).expect("append 2");
        append(&dir, &make_entry("t1", "acc3", "g3")).expect("append 3");

        let found = lookup(&dir, "t1").expect("lookup").expect("entry");
        assert_eq!(found.thread_id, "t1");
        assert_eq!(found.account, "acc3");
        assert_eq!(found.group_id, "g3");
    }

    #[test]
    fn lookup_unknown_returns_none() {
        let (_tmp, dir) = state_dir();
        append(&dir, &make_entry("t1", "acc1", "g1")).expect("append");

        let found = lookup(&dir, "missing").expect("lookup");
        assert!(found.is_none());
    }

    #[test]
    fn lookup_missing_file_returns_none() {
        let (_tmp, dir) = state_dir();

        let found = lookup(&dir, "missing").expect("lookup");
        assert!(found.is_none());
    }

    #[test]
    fn last_for_group_returns_most_recent() {
        let (_tmp, dir) = state_dir();
        append(&dir, &make_entry("t1", "acc1", "g1")).expect("append 1");
        append(&dir, &make_entry("t2", "acc2", "g2")).expect("append 2");
        append(&dir, &make_entry("t3", "acc3", "g1")).expect("append 3");

        let found = last_for_group(&dir, "g1")
            .expect("last_for_group")
            .expect("entry");
        assert_eq!(found.thread_id, "t3");
        assert_eq!(found.group_id, "g1");
    }

    #[test]
    fn last_for_group_unknown_returns_none() {
        let (_tmp, dir) = state_dir();
        append(&dir, &make_entry("t1", "acc1", "g1")).expect("append");

        let found = last_for_group(&dir, "missing").expect("last_for_group");
        assert!(found.is_none());
    }

    #[test]
    fn last_any_returns_last_entry() {
        let (_tmp, dir) = state_dir();
        append(&dir, &make_entry("t1", "acc1", "g1")).expect("append 1");
        append(&dir, &make_entry("t2", "acc2", "g2")).expect("append 2");

        let found = last_any(&dir).expect("last_any").expect("entry");
        assert_eq!(found.thread_id, "t2");
        assert_eq!(found.account, "acc2");
    }

    #[test]
    fn malformed_lines_skipped() {
        let (_tmp, dir) = state_dir();
        append(&dir, &make_entry("t1", "acc1", "g1")).expect("append 1");

        let path = index_path(&dir);
        let mut file = OpenOptions::new()
            .append(true)
            .open(path.as_std_path())
            .expect("open append");
        writeln!(file, "NOT JSON").expect("write malformed");

        append(&dir, &make_entry("t2", "acc2", "g2")).expect("append 2");

        let found = lookup(&dir, "t2").expect("lookup").expect("entry");
        assert_eq!(found.thread_id, "t2");

        let last = last_any(&dir).expect("last_any").expect("entry");
        assert_eq!(last.thread_id, "t2");
    }

    #[test]
    fn extract_thread_id_valid_event() {
        let input = b"{\"type\":\"thread.started\",\"thread_id\":\"t-123\"}\n";
        assert_eq!(extract_thread_id(input).as_deref(), Some("t-123"));
    }

    #[test]
    fn extract_thread_id_among_multiple_events() {
        let mut input = Vec::new();
        input.extend_from_slice(b"{\"type\":\"turn.started\"}\n");
        input.extend_from_slice(b"{\"type\":\"thread.started\",\"thread_id\":\"t-456\"}\n");
        input.extend_from_slice(b"{\"type\":\"item.completed\"}\n");
        assert_eq!(extract_thread_id(&input).as_deref(), Some("t-456"));
    }

    #[test]
    fn extract_thread_id_no_thread_event() {
        let input = b"{\"type\":\"turn.started\"}\n{\"type\":\"item.completed\"}\n";
        assert_eq!(extract_thread_id(input), None);
    }

    #[test]
    fn extract_thread_id_empty_input() {
        assert_eq!(extract_thread_id(b""), None);
    }

    #[test]
    fn extract_thread_id_malformed_json() {
        let input = b"NOT JSON\n{garbage\n";
        assert_eq!(extract_thread_id(input), None);
    }

    #[test]
    fn extract_thread_id_mixed_malformed_and_valid() {
        let input = b"NOT JSON\n{\"type\":\"thread.started\",\"thread_id\":\"t-789\"}\n";
        assert_eq!(extract_thread_id(input).as_deref(), Some("t-789"));
    }
}
