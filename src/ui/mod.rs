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
pub(crate) mod raw_passthrough;

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
    pub(crate) fn write_prompt(&self, body: &str) -> std::io::Result<()> {
        let mut stderr = std::io::stderr().lock();
        stderr.write_all(body.as_bytes())?;
        stderr.flush()
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
                }?;
                if let Some(ref account) = view.account {
                    let source = view.account_source.as_deref().unwrap_or("unknown");
                    writeln!(stdout, "account:         {account} (source: {source})")?;
                }
                Ok(())
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
                writeln!(
                    stdout,
                    "accounts:       {} ({} in cooldown)",
                    view.accounts_count, view.accounts_in_cooldown
                )?;
                writeln!(stdout, "active-auth:    {}", view.active_account_has_auth)?;
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
                        let cooldown_str = if account.cooldown_active {
                            format!(
                                "cooldown=active(reset_at={})",
                                account
                                    .cooldown_reset_at_unix
                                    .map_or_else(|| "?".to_owned(), |v| v.to_string())
                            )
                        } else {
                            "cooldown=none".to_owned()
                        };
                        writeln!(
                            stdout,
                            "  {} current={} has_auth={} last_used_at_unix={} {}",
                            account.name,
                            account.current,
                            account.has_auth,
                            account
                                .last_used_at_unix
                                .map_or_else(|| "(none)".to_owned(), |value| value.to_string()),
                            cooldown_str,
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
        Ok(())
    }

    #[allow(clippy::unused_self)]
    pub(crate) fn write_account_quota(
        &self,
        view: &crate::commands::account::AccountQuotaEntryView,
        format: crate::cli::OutputFormat,
        verbose: bool,
    ) -> std::io::Result<()> {
        let mut stdout = std::io::stdout().lock();
        match format {
            crate::cli::OutputFormat::Text => {
                let c = color::should_color(color::Stream::Stdout);
                write_quota_entry_text(&mut stdout, view, c, verbose)?;
                writeln!(
                    stdout,
                    "  {}Fetched: {} ({}, TTL {}s){}",
                    style_open(quota_styles::DIM, c),
                    human_age(view.fetched_at_unix),
                    fetched_label(view),
                    view.ttl_secs,
                    style_close(quota_styles::DIM, c),
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
        verbose: bool,
    ) -> std::io::Result<()> {
        let mut stdout = std::io::stdout().lock();
        match format {
            crate::cli::OutputFormat::Text => {
                if views.is_empty() {
                    return writeln!(stdout, "(no accounts)");
                }
                let c = color::should_color(color::Stream::Stdout);
                for (i, view) in views.iter().enumerate() {
                    if i > 0 {
                        writeln!(stdout)?;
                    }
                    write_quota_entry_text(&mut stdout, view, c, verbose)?;
                    writeln!(
                        stdout,
                        "  {}{}, {}{}",
                        style_open(quota_styles::DIM, c),
                        human_age(view.fetched_at_unix),
                        fetched_label(view),
                        style_close(quota_styles::DIM, c),
                    )?;
                }
                Ok(())
            }
            crate::cli::OutputFormat::Json => write_json_line(&mut stdout, views),
        }
    }

    #[allow(clippy::unused_self)]
    pub(crate) fn write_account_health(
        &self,
        view: &crate::commands::account::AccountHealthView,
        format: crate::cli::OutputFormat,
        verbose: bool,
    ) -> std::io::Result<()> {
        let mut stdout = std::io::stdout().lock();
        match format {
            crate::cli::OutputFormat::Json => write_json_line(&mut stdout, &view.entries),
            crate::cli::OutputFormat::Text => {
                if verbose {
                    write_health_verbose(&mut stdout, &view.entries)
                } else {
                    write_health_table(&mut stdout, &view.entries)
                }
            }
        }
    }

    #[allow(clippy::unused_self)]
    pub(crate) fn write_account_cooldowns(
        &self,
        view: &crate::commands::account::AccountCooldownView,
        format: crate::cli::OutputFormat,
    ) -> std::io::Result<()> {
        let mut stdout = std::io::stdout().lock();
        match format {
            crate::cli::OutputFormat::Text => {
                if view.entries.is_empty() {
                    return writeln!(stdout, "(no accounts)");
                }
                writeln!(stdout, "ACCOUNT     STATUS       RESETS         REASON")?;
                for entry in &view.entries {
                    let status = if entry.cooled_down {
                        "cooled-down"
                    } else {
                        "eligible"
                    };
                    let resets = entry
                        .reset_at_unix
                        .map_or_else(|| "—".to_owned(), human_duration_until);
                    let reason = entry
                        .reason
                        .as_ref()
                        .map_or_else(|| "—".to_owned(), |reason| format!("{reason:?}"));
                    writeln!(
                        stdout,
                        "{:<11} {:<12} {:<14} {}",
                        entry.account, status, resets, reason
                    )?;
                }
                Ok(())
            }
            crate::cli::OutputFormat::Json => write_json_line(&mut stdout, &view.entries),
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

fn write_health_verbose(
    stdout: &mut impl std::io::Write,
    entries: &[crate::commands::account::AccountHealthEntryView],
) -> std::io::Result<()> {
    for (idx, entry) in entries.iter().enumerate() {
        if idx > 0 {
            writeln!(stdout)?;
        }
        writeln!(stdout, "account: {}", entry.account)?;
        writeln!(
            stdout,
            "rank: {}",
            entry.rank.map_or_else(|| "—".to_owned(), |r| r.to_string())
        )?;
        writeln!(stdout, "score: {}", entry.score_label)?;
        writeln!(stdout, "plan: {}", entry.plan)?;
        writeln!(stdout, "token: {}", entry.token)?;
        writeln!(stdout, "token_detail: {}", entry.token_detail)?;
        writeln!(stdout, "status: {}", entry.status)?;
        writeln!(stdout, "active: {}", entry.active)?;
        writeln!(stdout, "cooldown: {}", entry.cooldown)?;
        writeln!(
            stdout,
            "last_used: {}",
            entry
                .last_used
                .map_or_else(|| "—".to_owned(), |ts| ts.to_string())
        )?;
        writeln!(stdout, "fetched_at_unix: {}", entry.fetched_at_unix)?;
    }
    Ok(())
}

fn write_health_table(
    stdout: &mut impl std::io::Write,
    entries: &[crate::commands::account::AccountHealthEntryView],
) -> std::io::Result<()> {
    let tok_w = entries
        .iter()
        .map(|e| e.token.len())
        .max()
        .unwrap_or(5)
        .max(5);
    writeln!(
        stdout,
        "{:<5} {:<7} {:<12} {:<tw$} {:<15} {:<12} {:<7} {:<9} FETCHED",
        "RANK",
        "SCORE",
        "ACCOUNT",
        "TOKEN",
        "PLAN",
        "STATUS",
        "ACTIVE",
        "COOLDOWN",
        tw = tok_w + 1,
    )?;
    for entry in entries {
        writeln!(
            stdout,
            "{:<5} {:<7} {:<12} {:<tw$} {:<15} {:<12} {:<7} {:<9} {}",
            entry.rank.map_or_else(|| "—".to_owned(), |r| r.to_string()),
            entry.score_label,
            entry.account,
            entry.token,
            entry.plan,
            entry.status,
            entry.active,
            entry.cooldown,
            human_age(entry.fetched_at_unix),
            tw = tok_w + 1,
        )?;
    }
    Ok(())
}

fn write_json_line<T: serde::Serialize + ?Sized>(
    mut stdout: impl std::io::Write,
    value: &T,
) -> std::io::Result<()> {
    serde_json::to_writer_pretty(&mut stdout, value)?;
    writeln!(stdout)
}

const fn fetched_label(_view: &crate::commands::account::AccountQuotaEntryView) -> &'static str {
    "live"
}

mod quota_styles {
    use anstyle::{AnsiColor, Effects, Style};

    pub(super) const BOLD: Style = Style::new().effects(Effects::BOLD);
    pub(super) const DIM: Style = Style::new().effects(Effects::DIMMED);
    pub(super) const BOLD_CYAN: Style = Style::new()
        .fg_color(Some(anstyle::Color::Ansi(AnsiColor::Cyan)))
        .effects(Effects::BOLD);
    pub(super) const RED: Style = Style::new().fg_color(Some(anstyle::Color::Ansi(AnsiColor::Red)));
    pub(super) const BOLD_GREEN: Style = Style::new()
        .fg_color(Some(anstyle::Color::Ansi(AnsiColor::Green)))
        .effects(Effects::BOLD);
    pub(super) const BOLD_YELLOW: Style = Style::new()
        .fg_color(Some(anstyle::Color::Ansi(AnsiColor::Yellow)))
        .effects(Effects::BOLD);
    pub(super) const BOLD_RED: Style = Style::new()
        .fg_color(Some(anstyle::Color::Ansi(AnsiColor::Red)))
        .effects(Effects::BOLD);
}

fn style_open(style: anstyle::Style, use_color: bool) -> impl std::fmt::Display {
    if use_color {
        style.render()
    } else {
        anstyle::Style::new().render()
    }
}

fn style_close(style: anstyle::Style, use_color: bool) -> impl std::fmt::Display {
    if use_color {
        style.render_reset()
    } else {
        anstyle::Style::new().render_reset()
    }
}

fn percent_style(pct: f64) -> anstyle::Style {
    if pct > 50.0 {
        quota_styles::BOLD_GREEN
    } else if pct > 20.0 {
        quota_styles::BOLD_YELLOW
    } else {
        quota_styles::BOLD_RED
    }
}

fn quota_bar(pct: f64, width: usize, use_color: bool) -> String {
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    let filled = (pct / 100.0 * width as f64).round() as usize;
    let filled = filled.min(width);
    let empty = width - filled;
    let style = percent_style(pct);
    format!(
        "{}{}{}{}",
        style_open(style, use_color),
        "█".repeat(filled),
        style_close(style, use_color),
        "░".repeat(empty),
    )
}

fn write_quota_entry_verbose(
    stdout: &mut impl std::io::Write,
    view: &crate::commands::account::AccountQuotaEntryView,
) -> std::io::Result<()> {
    writeln!(stdout, "account: {}", view.account)?;
    writeln!(
        stdout,
        "rank: {}",
        view.rank
            .map_or_else(|| "—".to_owned(), |rank| rank.to_string())
    )?;
    writeln!(
        stdout,
        "score: {}",
        view.score
            .map_or_else(|| "—".to_owned(), |score| format!("{score:.2}"))
    )?;
    writeln!(stdout, "mode: {}", view.mode)?;
    writeln!(stdout, "status: {}", view.status_label)?;
    writeln!(stdout, "active: {}", view.active)?;
    writeln!(stdout, "fetched_at_unix: {}", view.fetched_at_unix)?;
    writeln!(stdout, "ttl_secs: {}", view.ttl_secs)?;
    if let Some(ref fh) = view.five_hour {
        writeln!(stdout, "five_hour_pct: {:.1}", fh.percent_left)?;
        writeln!(stdout, "five_hour_reset_at_unix: {}", fh.reset_at_unix)?;
    }
    if let Some(ref wk) = view.weekly {
        writeln!(stdout, "weekly_pct: {:.1}", wk.percent_left)?;
        writeln!(stdout, "weekly_reset_at_unix: {}", wk.reset_at_unix)?;
    }
    if let Some(ref scoring) = view.scoring {
        writeln!(stdout, "scoring_total: {:.2}", scoring.total)?;
        writeln!(stdout, "scoring_recency_label: {}", scoring.recency_label)?;
        writeln!(stdout, "scoring_pressure_label: {}", scoring.pressure_label)?;
    }
    Ok(())
}

fn write_quota_entry_text(
    stdout: &mut impl std::io::Write,
    view: &crate::commands::account::AccountQuotaEntryView,
    use_color: bool,
    verbose: bool,
) -> std::io::Result<()> {
    if verbose {
        return write_quota_entry_verbose(stdout, view);
    }
    match view.mode.as_str() {
        "oauth" => {
            write!(
                stdout,
                "  {}#{} {:.2} {}{}{}{}",
                style_open(quota_styles::DIM, use_color),
                view.rank.unwrap_or(0),
                view.score.unwrap_or(0.0),
                style_close(quota_styles::DIM, use_color),
                style_open(quota_styles::BOLD, use_color),
                view.account,
                style_close(quota_styles::BOLD, use_color),
            )?;
            if view.active {
                write!(
                    stdout,
                    " {}(active){}",
                    style_open(quota_styles::BOLD_CYAN, use_color),
                    style_close(quota_styles::BOLD_CYAN, use_color),
                )?;
            }
            writeln!(stdout)?;
            if let Some(ref fh) = view.five_hour {
                let ps = percent_style(fh.percent_left);
                writeln!(
                    stdout,
                    "  Five-hour   {}  {}{:.1}%{} left   resets in {}",
                    quota_bar(fh.percent_left, 20, use_color),
                    style_open(ps, use_color),
                    fh.percent_left,
                    style_close(ps, use_color),
                    human_duration_until(fh.reset_at_unix),
                )?;
            }
            if let Some(ref wk) = view.weekly {
                let ps = percent_style(wk.percent_left);
                writeln!(
                    stdout,
                    "  Weekly      {}  {}{:.1}%{} left   resets in {}",
                    quota_bar(wk.percent_left, 20, use_color),
                    style_open(ps, use_color),
                    wk.percent_left,
                    style_close(ps, use_color),
                    human_duration_until(wk.reset_at_unix),
                )?;
            }
        }
        "api-key" => {
            writeln!(
                stdout,
                "  {}{}{} {}(api-key){}",
                style_open(quota_styles::BOLD, use_color),
                view.account,
                style_close(quota_styles::BOLD, use_color),
                style_open(quota_styles::DIM, use_color),
                style_close(quota_styles::DIM, use_color),
            )?;
            writeln!(stdout, "  Quota not available (API-key auth)")?;
        }
        _ => {
            write!(
                stdout,
                "  {}{}{}",
                style_open(quota_styles::BOLD, use_color),
                view.account,
                style_close(quota_styles::BOLD, use_color),
            )?;
            if view.active {
                write!(
                    stdout,
                    " {}(active){}",
                    style_open(quota_styles::BOLD_CYAN, use_color),
                    style_close(quota_styles::BOLD_CYAN, use_color),
                )?;
            }
            writeln!(stdout)?;
            writeln!(
                stdout,
                "  {}Error: {}{}",
                style_open(quota_styles::RED, use_color),
                view.error.as_deref().unwrap_or("unknown"),
                style_close(quota_styles::RED, use_color),
            )?;
        }
    }
    Ok(())
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
