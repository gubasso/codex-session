//! Human-facing output.
#![allow(
    clippy::missing_errors_doc,
    clippy::must_use_candidate,
    clippy::new_without_default
)]

use std::io::Write as _;

pub(crate) mod color;

/// UI renderer.
pub(crate) struct Ui;

impl Ui {
    /// Construct the UI.
    pub(crate) const fn new() -> Self {
        Self
    }

    /// Print raw help text to stdout.
    #[allow(clippy::unused_self)]
    pub(crate) fn write_help(&self, text: &str) -> std::io::Result<()> {
        let mut stdout = std::io::stdout().lock();
        if color::stdout_color() {
            for line in text.lines() {
                if is_help_heading(line) {
                    writeln!(stdout, "\u{1b}[1m{line}\u{1b}[0m")?;
                } else {
                    writeln!(stdout, "{line}")?;
                }
            }
            if !text.ends_with('\n') {
                stdout.flush()?;
            }
            return Ok(());
        }

        stdout.write_all(text.as_bytes())?;
        if !text.ends_with('\n') {
            writeln!(stdout)?;
        }
        stdout.flush()
    }

    /// Print the `--dry-run` report verbatim to stdout.
    #[allow(clippy::unused_self)]
    pub(crate) fn write_dry_run(&self, body: &str) -> std::io::Result<()> {
        let mut stdout = std::io::stdout().lock();
        stdout.write_all(body.as_bytes())?;
        stdout.flush()
    }

    /// Print wrapper and child version details.
    #[allow(clippy::unused_self)]
    pub(crate) fn write_version(
        &self,
        view: &crate::commands::version::VersionView,
        fmt: crate::cli::OutputFormat,
    ) -> std::io::Result<()> {
        let mut stdout = std::io::stdout().lock();
        match fmt {
            crate::cli::OutputFormat::Text => {
                writeln!(stdout, "codex-session {}", view.wrapper_version)?;
                match (&view.child_path, &view.child_version) {
                    (Some(path), Some(version)) => writeln!(stdout, "codex {path} {version}"),
                    (Some(path), None) => writeln!(stdout, "codex {path} (unknown)"),
                    (None, _) => writeln!(stdout, "codex (unresolved)"),
                }
            }
            crate::cli::OutputFormat::Json => write_json_line(&mut stdout, view),
        }
    }

    /// Print the merge status.
    #[allow(clippy::unused_self)]
    pub(crate) fn write_config_status(
        &self,
        view: &crate::commands::config_status::ConfigStatusView,
        fmt: crate::cli::OutputFormat,
    ) -> std::io::Result<()> {
        let mut stdout = std::io::stdout().lock();
        match fmt {
            crate::cli::OutputFormat::Text => {
                writeln!(
                    stdout,
                    "base:        {} (exists={})",
                    view.base_path, view.base_exists
                )?;
                writeln!(
                    stdout,
                    "target:      {} (exists={})",
                    view.target_path, view.target_exists
                )?;
                writeln!(
                    stdout,
                    "stamp:       {} (exists={})",
                    view.stamp_path, view.stamp_exists
                )?;
                writeln!(
                    stdout,
                    "child-bin:   {}",
                    view.child_bin.as_deref().unwrap_or("(unavailable)")
                )?;
                writeln!(stdout, "log-file:    {}", view.log_file)?;
                writeln!(stdout, "log-verbose: {}", view.log_verbose)?;
                writeln!(stdout, "log-mirror:  {}", view.log_mirror_stderr)?;
                writeln!(stdout, "log-format:  {}", format_log(view.log_format))?;
                writeln!(
                    stdout,
                    "log-stderr-format: {}",
                    view.log_stderr_format.map_or("auto", format_log)
                )?;
                writeln!(
                    stdout,
                    "needs_merge: {}",
                    if view.needs_merge { "yes" } else { "no" }
                )?;
                writeln!(stdout, "sources:")?;
                writeln!(stdout, "  defaults")?;
                writeln!(
                    stdout,
                    "  user:    {}",
                    view.sources.user.as_deref().unwrap_or("none")
                )?;
                writeln!(
                    stdout,
                    "  project: {}",
                    view.sources.project.as_deref().unwrap_or("none")
                )?;
                writeln!(stdout, "  env:     {}", view.sources.env)?;
                writeln!(stdout, "  cli:     {}", view.sources.cli)
            }
            crate::cli::OutputFormat::Json => write_json_line(&mut stdout, view),
        }
    }

    /// Print local sections verbatim.
    #[allow(clippy::unused_self)]
    pub(crate) fn write_show_local(
        &self,
        view: &crate::commands::config_show_local::ShowLocalView,
        fmt: crate::cli::OutputFormat,
    ) -> std::io::Result<()> {
        let mut stdout = std::io::stdout().lock();
        match fmt {
            crate::cli::OutputFormat::Text => stdout.write_all(view.local_sections.as_bytes()),
            crate::cli::OutputFormat::Json => write_json_line(&mut stdout, view),
        }
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

fn is_help_heading(line: &str) -> bool {
    matches!(
        line,
        "Usage:" | "Wrapper verbs:" | "Wrapper options:" | "Environment:"
    )
}

const fn format_log(format: crate::config::LogFormat) -> &'static str {
    match format {
        crate::config::LogFormat::Json => "json",
        crate::config::LogFormat::Pretty => "pretty",
    }
}

fn write_json_line(
    mut stdout: impl std::io::Write,
    value: &impl serde::Serialize,
) -> std::io::Result<()> {
    serde_json::to_writer_pretty(&mut stdout, value)?;
    writeln!(stdout)
}
