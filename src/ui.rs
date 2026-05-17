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

    /// Print the wrapper version.
    #[allow(clippy::unused_self)]
    pub(crate) fn print_version(&self, version: &str) -> std::io::Result<()> {
        let mut stdout = std::io::stdout().lock();
        writeln!(stdout, "codex-session {version}")
    }

    /// Print the merge status.
    #[allow(clippy::unused_self, clippy::fn_params_excessive_bools)]
    pub(crate) fn print_config_status(
        &self,
        paths: &crate::domain::paths::CodexPaths,
        base_exists: bool,
        target_exists: bool,
        stamp_exists: bool,
        needs_merge: bool,
    ) -> std::io::Result<()> {
        let mut stdout = std::io::stdout().lock();
        writeln!(
            stdout,
            "base:        {} (exists={base_exists})",
            paths.base.display()
        )?;
        writeln!(
            stdout,
            "target:      {} (exists={target_exists})",
            paths.target.display()
        )?;
        writeln!(
            stdout,
            "stamp:       {} (exists={stamp_exists})",
            paths.stamp.display()
        )?;
        writeln!(
            stdout,
            "needs_merge: {}",
            if needs_merge { "yes" } else { "no" }
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
}
