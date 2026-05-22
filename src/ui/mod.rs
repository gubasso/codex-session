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

    #[allow(clippy::unused_self)]
    pub(crate) fn write_warning(&self, body: &str) -> std::io::Result<()> {
        let mut stderr = std::io::stderr().lock();
        stderr.write_all(body.as_bytes())?;
        if !body.ends_with('\n') {
            stderr.write_all(b"\n")?;
        }
        stderr.flush()
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
                writeln!(stdout, "account:        {}", view.account)?;
                writeln!(stdout, "account-source: {}", view.account_source)?;
                writeln!(stdout, "group-id:       {}", view.group_id)?;
                writeln!(stdout, "group-id-source: {}", view.group_id_source)?;
                writeln!(stdout, "codex_home:     {}", view.codex_home)?;
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
                writeln!(stdout, "account:         {}", report.account)?;
                writeln!(stdout, "account-source:  {}", report.account_source)?;
                writeln!(stdout, "group-id:        {}", report.group_id)?;
                writeln!(stdout, "group-id-source: {}", report.group_id_source)?;
                writeln!(stdout, "codex_home:      {}", report.codex_home)?;
                writeln!(
                    stdout,
                    "active account:  {} ({})",
                    report.active_account.name, report.active_account.source
                )?;
                writeln!(stdout, "accounts:")?;
                if report.accounts.is_empty() {
                    writeln!(stdout, "  (none)")?;
                } else {
                    for account in &report.accounts {
                        writeln!(
                            stdout,
                            "  {} current={} has_auth={} last_used_at_unix={}",
                            account.name,
                            account.current,
                            account.has_auth,
                            account
                                .last_used_at_unix
                                .map_or_else(|| "(none)".to_owned(), |value| value.to_string())
                        )?;
                    }
                }
                writeln!(stdout)?;
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
    pub(crate) fn write_account_list(
        &self,
        view: &crate::commands::account::AccountListView,
        format: crate::cli::OutputFormat,
    ) -> std::io::Result<()> {
        let mut stdout = std::io::stdout().lock();
        match format {
            crate::cli::OutputFormat::Text => {
                if let Some(active) = view.active.as_ref() {
                    writeln!(stdout, "active: {} ({})", active.name, active.source)?;
                }
                if view.accounts.is_empty() {
                    writeln!(stdout, "(no accounts)")
                } else {
                    for account in &view.accounts {
                        writeln!(
                            stdout,
                            "{}: {} has_auth={} current={} last_used_at_unix={}",
                            account.name,
                            account.dir,
                            account.has_auth,
                            account.current,
                            account
                                .last_used_at_unix
                                .map_or_else(|| "(none)".to_owned(), |value| value.to_string())
                        )?;
                    }
                    Ok(())
                }
            }
            crate::cli::OutputFormat::Json => write_json_line(&mut stdout, view),
        }
    }

    #[allow(clippy::unused_self)]
    pub(crate) fn write_account_current(
        &self,
        view: &crate::commands::account::AccountCurrentView,
        format: crate::cli::OutputFormat,
    ) -> std::io::Result<()> {
        let mut stdout = std::io::stdout().lock();
        match format {
            crate::cli::OutputFormat::Text => writeln!(stdout, "{} ({})", view.name, view.source),
            crate::cli::OutputFormat::Json => write_json_line(&mut stdout, view),
        }
    }

    #[allow(clippy::unused_self)]
    pub(crate) fn write_account_mutation(
        &self,
        verb: &'static str,
        view: &crate::commands::account::AccountMutationView,
    ) -> std::io::Result<()> {
        let mut stdout = std::io::stdout().lock();
        writeln!(stdout, "account {verb}: {}", view.name)?;
        writeln!(stdout, "path: {}", view.path)?;
        if let Some(archived_to) = view.archived_to.as_ref() {
            writeln!(stdout, "archived-to: {archived_to}")?;
        }
        Ok(())
    }

    #[allow(clippy::unused_self)]
    pub(crate) fn write_account_quota(
        &self,
        view: &crate::commands::account::AccountQuotaEntryView,
        format: crate::cli::OutputFormat,
    ) -> std::io::Result<()> {
        let mut stdout = std::io::stdout().lock();
        match format {
            crate::cli::OutputFormat::Text => {
                let suffix = if view.active { " (active)" } else { "" };
                match view.mode.as_str() {
                    "oauth" => {
                        writeln!(stdout, "account: {}{suffix}", view.account)?;
                        if let Some(five_hour) = view.five_hour.as_ref() {
                            writeln!(
                                stdout,
                                "five-hour:  {:.1}% left, resets in {}",
                                five_hour.percent_left,
                                human_duration_until(five_hour.reset_at_unix)
                            )?;
                        }
                        if let Some(weekly) = view.weekly.as_ref() {
                            writeln!(
                                stdout,
                                "weekly:     {:.1}% left, resets in {}",
                                weekly.percent_left,
                                human_duration_until(weekly.reset_at_unix)
                            )?;
                        }
                    }
                    "api-key" => {
                        writeln!(stdout, "account: {} (api-key-mode)", view.account)?;
                        writeln!(stdout, "quota:   not available (API-key auth)")?;
                    }
                    _ => {
                        writeln!(stdout, "account: {}{}", view.account, suffix)?;
                        writeln!(
                            stdout,
                            "quota:   error ({})",
                            view.error.as_deref().unwrap_or("unknown")
                        )?;
                    }
                }
                writeln!(
                    stdout,
                    "fetched:    {} ({}, TTL {}s)",
                    human_age(view.fetched_at_unix),
                    fetched_label(view),
                    view.ttl_secs
                )
            }
            crate::cli::OutputFormat::Json => write_json_line(&mut stdout, view),
        }
    }

    #[allow(clippy::unused_self)]
    pub(crate) fn write_account_quota_many(
        &self,
        views: &[crate::commands::account::AccountQuotaEntryView],
        format: crate::cli::OutputFormat,
    ) -> std::io::Result<()> {
        let mut stdout = std::io::stdout().lock();
        match format {
            crate::cli::OutputFormat::Text => {
                if views.is_empty() {
                    return writeln!(stdout, "(no accounts)");
                }
                for view in views {
                    match view.mode.as_str() {
                        "oauth" => {
                            let percent = view.five_hour.as_ref().map_or_else(
                                || "n/a".to_owned(),
                                |window| format!("{:.1}%", window.percent_left),
                            );
                            writeln!(
                                stdout,
                                "{}: mode={} five-hour={} fetched={}{}",
                                view.account,
                                view.mode,
                                percent,
                                human_age(view.fetched_at_unix),
                                if view.active { " active" } else { "" }
                            )?;
                        }
                        _ => {
                            writeln!(
                                stdout,
                                "{}: mode={} fetched={}{}",
                                view.account,
                                view.mode,
                                human_age(view.fetched_at_unix),
                                if view.active { " active" } else { "" }
                            )?;
                        }
                    }
                }
                Ok(())
            }
            crate::cli::OutputFormat::Json => write_json_line(&mut stdout, views),
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
        writeln!(stdout, "group-id:     {}", view.group_id)?;
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

fn write_json_line<T: serde::Serialize + ?Sized>(
    mut stdout: impl std::io::Write,
    value: &T,
) -> std::io::Result<()> {
    serde_json::to_writer_pretty(&mut stdout, value)?;
    writeln!(stdout)
}

const fn fetched_label(view: &crate::commands::account::AccountQuotaEntryView) -> &'static str {
    if view.stale {
        "stale"
    } else if view.live {
        "live"
    } else {
        "cached"
    }
}

fn human_duration_until(reset_at_unix: u64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs());
    let secs = reset_at_unix.saturating_sub(now);
    human_duration_secs(secs)
}

fn human_age(fetched_at_unix: u64) -> String {
    if fetched_at_unix == 0 {
        return "unknown".to_owned();
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs());
    let secs = now.saturating_sub(fetched_at_unix);
    format!("{} ago", human_duration_secs(secs))
}

fn human_duration_secs(secs: u64) -> String {
    let days = secs / 86_400;
    let hours = (secs % 86_400) / 3_600;
    let minutes = (secs % 3_600) / 60;
    let seconds = secs % 60;

    if days > 0 {
        format!("{days}d {hours}h")
    } else if hours > 0 {
        format!("{hours}h {minutes}m")
    } else if minutes > 0 {
        format!("{minutes}m {seconds}s")
    } else {
        format!("{seconds} s")
    }
}
