//! Human-facing output.
//!
//! What this is: text and JSON rendering for wrapper-owned commands.
//! What this is not: color-policy decisions or command dispatch.
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

    /// Print the `--dry-run` report verbatim to stdout.
    #[allow(clippy::unused_self)]
    pub(crate) fn write_dry_run(&self, body: &str) -> std::io::Result<()> {
        let mut stdout = std::io::stdout().lock();
        stdout.write_all(body.as_bytes())?;
        stdout.flush()
    }

    #[allow(clippy::unused_self)]
    pub(crate) fn write_bytes(&self, bytes: &[u8]) -> std::io::Result<()> {
        let mut stdout = std::io::stdout().lock();
        stdout.write_all(bytes)?;
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
                    "active-profile: {}",
                    view.active_profile.as_deref().unwrap_or("(stock mode)")
                )?;
                writeln!(
                    stdout,
                    "manifest-path:  {}",
                    view.manifest_path
                        .as_ref()
                        .map_or_else(|| "(none)".to_owned(), ToString::to_string)
                )?;
                writeln!(stdout, "session-root:   {}", view.session_root)?;
                writeln!(stdout, "session-source: {}", view.session_root_source)?;
                writeln!(
                    stdout,
                    "child-bin:      {}",
                    view.child_bin
                        .as_ref()
                        .map_or_else(|| "(unavailable)".to_owned(), ToString::to_string)
                )?;
                writeln!(stdout, "layers:")?;
                if view.layer_paths.is_empty() {
                    writeln!(stdout, "  (none)")?;
                } else {
                    for layer in &view.layer_paths {
                        writeln!(
                            stdout,
                            "  {} => {} (exists={})",
                            layer.name, layer.path, layer.exists
                        )?;
                        if let Some(error) = layer.error.as_deref() {
                            writeln!(stdout, "    error: {error}")?;
                        }
                    }
                }
                writeln!(stdout, "log.file: {}", view.log.file)?;
                writeln!(stdout, "log.verbose: {}", view.log.verbose)?;
                writeln!(stdout, "log.mirror-stderr: {}", view.log.mirror_stderr)?;
                writeln!(stdout, "log.format: {}", format_log(view.log.format))?;
                writeln!(
                    stdout,
                    "log.stderr-format: {}",
                    view.log.stderr_format.map_or("auto", format_log)
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

    #[allow(clippy::unused_self)]
    pub(crate) fn write_profile_list(
        &self,
        view: &crate::commands::profile_list::ProfileListView,
        fmt: crate::cli::OutputFormat,
    ) -> std::io::Result<()> {
        let mut stdout = std::io::stdout().lock();
        match fmt {
            crate::cli::OutputFormat::Text => {
                if view.profiles.is_empty() {
                    writeln!(stdout, "(no profiles)")
                } else {
                    for profile in &view.profiles {
                        writeln!(
                            stdout,
                            "{}: {} layers={} valid={}",
                            profile.name, profile.manifest_path, profile.layer_count, profile.valid
                        )?;
                        if let Some(error) = profile.error.as_deref() {
                            writeln!(stdout, "  error: {error}")?;
                        }
                    }
                    Ok(())
                }
            }
            crate::cli::OutputFormat::Json => write_json_line(&mut stdout, view),
        }
    }

    #[allow(clippy::unused_self)]
    pub(crate) fn write_profile_show(
        &self,
        view: &crate::commands::profile_show::ProfileShowView,
        fmt: crate::cli::OutputFormat,
    ) -> std::io::Result<()> {
        let mut stdout = std::io::stdout().lock();
        match fmt {
            crate::cli::OutputFormat::Text => {
                if view.stock_mode {
                    return writeln!(stdout, "stock mode");
                }
                writeln!(
                    stdout,
                    "profile: {}",
                    view.active_profile.as_deref().unwrap_or("(none)")
                )?;
                writeln!(
                    stdout,
                    "manifest: {}",
                    view.manifest_path
                        .as_ref()
                        .map_or_else(|| "(none)".to_owned(), ToString::to_string)
                )?;
                writeln!(stdout, "layers:")?;
                for layer in &view.layer_paths {
                    writeln!(
                        stdout,
                        "  {} => {} (exists={})",
                        layer.name, layer.path, layer.exists
                    )?;
                }
                Ok(())
            }
            crate::cli::OutputFormat::Json => write_json_line(&mut stdout, view),
        }
    }

    #[allow(clippy::unused_self)]
    pub(crate) fn write_doctor(
        &self,
        report: &crate::commands::doctor::DoctorReport,
        fmt: crate::cli::OutputFormat,
    ) -> std::io::Result<()> {
        let mut stdout = std::io::stdout().lock();
        match fmt {
            crate::cli::OutputFormat::Text => {
                let name_width = report
                    .checks
                    .iter()
                    .map(|c| c.name.len())
                    .max()
                    .unwrap_or(8)
                    .max(8);
                writeln!(
                    stdout,
                    "status   {:width$}  detail",
                    "check",
                    width = name_width
                )?;
                for check in &report.checks {
                    writeln!(
                        stdout,
                        "{:7}  {:width$}  {}",
                        format_status(check.status),
                        check.name,
                        check.detail,
                        width = name_width
                    )?;
                }
                if !report.next_steps.is_empty() {
                    writeln!(stdout, "\nNext:")?;
                    for step in &report.next_steps {
                        writeln!(stdout, "  - {step}")?;
                    }
                }
                if !report.env.is_empty() {
                    for (profile, env) in &report.env {
                        writeln!(stdout, "\nMerged env for {profile}:")?;
                        for (key, value) in env {
                            writeln!(stdout, "  {key}={value}")?;
                        }
                    }
                }
                writeln!(
                    stdout,
                    "\nsummary: {} OK, {} WARN, {} FAIL",
                    report.summary.ok, report.summary.warn, report.summary.fail
                )
            }
            crate::cli::OutputFormat::Json => write_json_line(&mut stdout, report),
        }
    }

    #[allow(clippy::unused_self)]
    pub(crate) fn write_profile_compose(
        &self,
        view: &crate::commands::profile_compose::ProfileComposeView,
    ) -> std::io::Result<()> {
        let mut stdout = std::io::stdout().lock();
        writeln!(
            stdout,
            "profile:      {}",
            view.profile.as_deref().unwrap_or("(stock mode)")
        )?;
        writeln!(stdout, "terminal-id:  {}", view.terminal_id)?;
        writeln!(stdout, "session-dir:  {}", view.session_dir)?;
        writeln!(stdout, "config:       {}", view.config_path)?;
        writeln!(stdout, "sidecar:      {}", view.sidecar_path)?;
        writeln!(stdout, "session-meta: {}", view.session_meta_path)
    }
}

const fn format_log(format: crate::config::LogFormat) -> &'static str {
    match format {
        crate::config::LogFormat::Json => "json",
        crate::config::LogFormat::Pretty => "pretty",
    }
}

const fn format_status(status: crate::commands::doctor::CheckStatus) -> &'static str {
    match status {
        crate::commands::doctor::CheckStatus::Ok => "OK",
        crate::commands::doctor::CheckStatus::Warn => "WARN",
        crate::commands::doctor::CheckStatus::Fail => "FAIL",
    }
}

fn write_json_line(
    mut stdout: impl std::io::Write,
    value: &impl serde::Serialize,
) -> std::io::Result<()> {
    serde_json::to_writer_pretty(&mut stdout, value)?;
    writeln!(stdout)
}
