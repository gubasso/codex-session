//! Human-facing output.
#![allow(
    clippy::missing_errors_doc,
    clippy::must_use_candidate,
    clippy::new_without_default
)]

use std::io::Write as _;

/// UI renderer.
pub(crate) struct Ui;

impl Ui {
    /// Construct the UI.
    pub(crate) const fn new() -> Self {
        Self
    }

    /// Print raw help text to stdout.
    #[allow(clippy::unused_self)]
    pub(crate) fn print_help_raw(&self, text: &str) -> std::io::Result<()> {
        let mut stdout = std::io::stdout().lock();
        stdout.write_all(text.as_bytes())
    }

    /// Print wrapper and child version details.
    #[allow(clippy::unused_self)]
    pub(crate) fn print_version_details(
        &self,
        report: &crate::commands::self_version::VersionReport,
    ) -> std::io::Result<()> {
        let mut stdout = std::io::stdout().lock();
        writeln!(stdout, "codex-session {}", report.wrapper_version)?;
        match (&report.child_path, &report.child_version) {
            (Some(path), Some(version)) => writeln!(stdout, "codex {path} ({version})"),
            (Some(path), None) => writeln!(stdout, "codex {path} (unknown)"),
            (None, _) => writeln!(stdout, "codex unavailable"),
        }
    }

    /// Print the merge status.
    #[allow(clippy::unused_self)]
    pub(crate) fn print_config_status(
        &self,
        report: &crate::commands::self_config_status::ConfigStatusReport,
    ) -> std::io::Result<()> {
        let mut stdout = std::io::stdout().lock();
        writeln!(
            stdout,
            "base:        {} (exists={})",
            report.base_path, report.base_exists
        )?;
        writeln!(
            stdout,
            "target:      {} (exists={})",
            report.target_path, report.target_exists
        )?;
        writeln!(
            stdout,
            "stamp:       {} (exists={})",
            report.stamp_path, report.stamp_exists
        )?;
        writeln!(
            stdout,
            "child-bin:   {}",
            report.child_bin.as_deref().unwrap_or("(unavailable)")
        )?;
        writeln!(stdout, "log-file:    {}", report.log_file)?;
        writeln!(
            stdout,
            "needs_merge: {}",
            if report.needs_merge { "yes" } else { "no" }
        )
    }

    /// Print local sections verbatim.
    #[allow(clippy::unused_self)]
    pub(crate) fn print_local_sections(&self, text: &str) -> std::io::Result<()> {
        let mut stdout = std::io::stdout().lock();
        stdout.write_all(text.as_bytes())
    }

    /// Print a successful merge line.
    #[allow(clippy::unused_self)]
    pub(crate) fn print_merge_success(
        &self,
        base: &std::path::Path,
        target: &std::path::Path,
    ) -> std::io::Result<()> {
        let mut stdout = std::io::stdout().lock();
        writeln!(stdout, "merged: {} -> {}", base.display(), target.display())
    }

    /// Print JSON output for a serializable wrapper report.
    #[allow(clippy::unused_self)]
    pub(crate) fn print_json<T: serde::Serialize>(&self, value: &T) -> std::io::Result<()> {
        let mut stdout = std::io::stdout().lock();
        serde_json::to_writer_pretty(&mut stdout, value)?;
        writeln!(stdout)
    }
}
