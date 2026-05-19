//! Typed child invocation.
//!
//! What this is: pure data describing how to invoke the child —
//! binary path, argv, env diff. Has no I/O.
//! What this is not: an executor. Use `Spawner` to actually run it.

use camino::Utf8PathBuf;
use std::ffi::OsString;
use std::fmt::Write as _;

#[derive(Debug, Clone)]
pub(crate) struct ChildInvocation {
    pub(crate) binary: Utf8PathBuf,
    pub(crate) args: Vec<OsString>,
    pub(crate) env: ChildEnv,
}

#[derive(Debug, Clone)]
pub(crate) struct ChildEnv {
    pub(crate) inherit: bool,
    /// Env keys to remove before exec. Stored as `OsString` so that
    /// non-UTF-8 keys (which are legal on Unix) are still scrubbed
    /// byte-for-byte rather than silently dropped on conversion.
    pub(crate) remove: Vec<OsString>,
    pub(crate) set: Vec<(String, OsString)>,
}

impl ChildEnv {
    /// Wrapper-private env keys that must never reach the child, plus
    /// the re-entry marker that the next-layer wrapper checks for.
    ///
    /// The entire `CODEX_SESSION_*` namespace is wrapper-private (see
    /// `src/config/mod.rs::apply_env_layer` — keys consumed include
    /// `CHILD_BIN`, `LOG_FILE`, `LOG_DIR`, `LOG_VERBOSE`,
    /// `LOG_MIRROR_STDERR`, `LOG_FORMAT`, `PATHS_*`, and the re-entry
    /// marker `REENTRY`). Per the cli-design wrapper spec
    /// (`06-cli-wrapper-design/process-and-posix.md`), the wrapper
    /// must "scrub your own namespace from the child's env unless you
    /// intend to expose it". We therefore strip every variable whose
    /// name starts with `CODEX_SESSION_` from the parent env, plus a
    /// fixed baseline of known names (kept for deterministic
    /// `--dry-run` output even when those keys are absent at runtime).
    ///
    /// Order: `into_command` applies `env_remove` before `env`, so a
    /// stale `CODEX_SESSION_REENTRY` from the parent env is cleared
    /// first and the explicit `=1` set wins.
    pub(crate) fn scrubbed_default() -> Self {
        // Deterministic baseline — always reported in `--dry-run`,
        // even when no `CODEX_SESSION_*` var is set in the parent env.
        const BASELINE: &[&str] = &[
            "CODEX_SESSION_CHILD_BIN",
            "CODEX_SESSION_LOG_FILE",
            "CODEX_SESSION_LOG_DIR",
            "CODEX_SESSION_REENTRY",
        ];
        const PREFIX: &[u8] = b"CODEX_SESSION_";

        let mut remove: Vec<OsString> = BASELINE.iter().map(|&s| OsString::from(s)).collect();

        // Union in any other live `CODEX_SESSION_*` keys (e.g.
        // `CODEX_SESSION_LOG_VERBOSE`, `CODEX_SESSION_PATHS_CACHE_DIR`)
        // so future wrapper-private vars don't leak by default.
        //
        // Compare bytewise on Unix: env keys may be arbitrary bytes,
        // and `to_str()` would silently drop a hostile non-UTF-8
        // `CODEX_SESSION_*` key, leaving it to inherit into the child.
        for (key, _) in std::env::vars_os() {
            if !has_prefix(&key, PREFIX) {
                continue;
            }
            if !remove.iter().any(|existing| existing == &key) {
                remove.push(key);
            }
        }
        // Sort to keep the rendered dry-run report stable regardless of
        // env iteration order; the baseline keys still come first
        // because they share the common prefix and sort lexicographically.
        // Non-UTF-8 keys sort by raw byte order, which is also deterministic.
        remove.sort();
        remove.dedup();

        Self {
            inherit: true,
            remove,
            set: vec![("CODEX_SESSION_REENTRY".into(), OsString::from("1"))],
        }
    }
}

/// Bytewise prefix check for `OsStr`. On Unix, env keys are arbitrary
/// bytes; converting through `to_str()` first would silently miss a
/// `CODEX_SESSION_<non-utf8>` key. Falls back to a UTF-8-only check on
/// non-Unix targets (the wrapper currently only targets Unix anyway).
fn has_prefix(value: &std::ffi::OsStr, prefix: &[u8]) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt as _;
        value.as_bytes().starts_with(prefix)
    }
    #[cfg(not(unix))]
    {
        match value.to_str() {
            Some(s) => match std::str::from_utf8(prefix) {
                Ok(p) => s.starts_with(p),
                Err(_) => false,
            },
            None => false,
        }
    }
}

impl ChildInvocation {
    /// Pure projection from typed invocation to `std::process::Command`.
    /// Tests snapshot the result of this function via the dry-run report.
    pub(crate) fn into_command(self) -> std::process::Command {
        let mut cmd = std::process::Command::new(self.binary.as_std_path());
        cmd.args(&self.args);
        if !self.env.inherit {
            cmd.env_clear();
        }
        for key in &self.env.remove {
            cmd.env_remove(key);
        }
        for (key, value) in &self.env.set {
            cmd.env(key, value);
        }
        cmd
    }

    /// Deterministic human-readable report for `--dry-run`. Newline-terminated.
    pub(crate) fn dry_run_report(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "binary: {}", self.binary);
        out.push_str("argv:\n");
        for (i, arg) in self.args.iter().enumerate() {
            let _ = writeln!(out, "  [{i}] {}", arg.to_string_lossy());
        }
        let _ = writeln!(out, "env.inherit: {}", self.env.inherit);
        if !self.env.remove.is_empty() {
            out.push_str("env.remove:\n");
            for key in &self.env.remove {
                let _ = writeln!(out, "  {}", key.to_string_lossy());
            }
        }
        if !self.env.set.is_empty() {
            out.push_str("env.set:\n");
            for (key, value) in &self.env.set {
                let _ = writeln!(out, "  {key}={}", value.to_string_lossy());
            }
        }
        out
    }
}
