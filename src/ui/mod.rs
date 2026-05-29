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
                    "active-config-recipe: {}",
                    view.active_config_recipe
                        .as_deref()
                        .unwrap_or("(stock mode)")
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
    pub(crate) fn write_config_recipe_list(
        &self,
        view: &crate::commands::config_recipe_list::ConfigRecipeListView,
        fmt: crate::cli::OutputFormat,
    ) -> std::io::Result<()> {
        let mut stdout = std::io::stdout().lock();
        match fmt {
            crate::cli::OutputFormat::Text => {
                if view.recipes.is_empty() {
                    writeln!(stdout, "(no config recipes)")
                } else {
                    for config_recipe in &view.recipes {
                        writeln!(
                            stdout,
                            "{}: {} layers={} valid={}",
                            config_recipe.name,
                            config_recipe.manifest_path,
                            config_recipe.layer_count,
                            config_recipe.valid
                        )?;
                        if let Some(error) = config_recipe.error.as_deref() {
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
    pub(crate) fn write_config_recipe_show(
        &self,
        view: &crate::commands::config_recipe_show::ConfigRecipeShowView,
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
                    "config-recipe: {}",
                    view.active_config_recipe.as_deref().unwrap_or("(none)")
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
                    for (config_recipe, env) in &report.env {
                        writeln!(stdout, "\nMerged env for {config_recipe}:")?;
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
                if view.accounts.is_empty() {
                    return writeln!(stdout, "(no accounts)");
                }
                let use_color = color::should_color(color::Stream::Stdout);
                let account_width = view
                    .accounts
                    .iter()
                    .map(|account| account.name.len())
                    .chain(Some("ACCOUNT".len()))
                    .max()
                    .unwrap_or("ACCOUNT".len())
                    .max("ACCOUNT".len());
                let last_used_width = view
                    .accounts
                    .iter()
                    .map(|account| {
                        account
                            .last_used_at_unix
                            .map_or_else(|| "—".len(), |value| human_age(value).len())
                    })
                    .chain(Some("LAST USED".len()))
                    .max()
                    .unwrap_or("LAST USED".len())
                    .max("LAST USED".len());
                writeln!(
                    stdout,
                    "  {}  {}  {}  {}",
                    styled_padded("ACCOUNT", account_width, styles::DIM, use_color),
                    styled_padded("AUTH", 4, styles::DIM, use_color),
                    styled_padded("LAST USED", last_used_width, styles::DIM, use_color),
                    styled_text("STATUS", styles::DIM, use_color),
                )?;
                let mut current_rendered = false;
                for account in &view.accounts {
                    current_rendered |= account.current;
                    let prefix = if account.current {
                        format!(
                            "{}▸{} ",
                            style_open(styles::BOLD_CYAN, use_color),
                            style_close(styles::BOLD_CYAN, use_color)
                        )
                    } else {
                        "  ".to_owned()
                    };
                    let auth = if account.has_auth {
                        styled_padded("✓", 4, styles::GREEN, use_color)
                    } else {
                        styled_padded("✗", 4, styles::RED, use_color)
                    };
                    let (last_used_text, last_used_style) = account.last_used_at_unix.map_or_else(
                        || ("—".to_owned(), styles::DIM),
                        |value| (human_age(value), styles::DIM),
                    );
                    let name = styled_padded(&account.name, account_width, styles::BOLD, use_color);
                    if account.current {
                        let active = view
                            .active
                            .as_ref()
                            .map(|active| format!("active ({})", active.source))
                            .unwrap_or_default();
                        // STATUS follows, so pad LAST USED to align the column.
                        let last_used = styled_padded(
                            &last_used_text,
                            last_used_width,
                            last_used_style,
                            use_color,
                        );
                        writeln!(
                            stdout,
                            "{prefix}{name}  {auth}  {last_used}  {}",
                            styled_text(&active, styles::BOLD_CYAN, use_color),
                        )?;
                    } else {
                        // LAST USED is the final column; leave it unpadded to avoid
                        // trailing whitespace.
                        let last_used = styled_text(&last_used_text, last_used_style, use_color);
                        writeln!(stdout, "{prefix}{name}  {auth}  {last_used}")?;
                    }
                }
                // If the resolved active account is not one of the listed rows
                // (e.g. a pinned/env/LRU name that is not registered), no row
                // carried the `▸` marker. Surface the selection explicitly so
                // `account list` never silently hides which account is active.
                if !current_rendered && let Some(active) = view.active.as_ref() {
                    writeln!(
                        stdout,
                        "{}",
                        styled_text(
                            format!("active: {} ({})", active.name, active.source),
                            styles::BOLD_CYAN,
                            use_color,
                        ),
                    )?;
                }
                Ok(())
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
            crate::cli::OutputFormat::Text => {
                let use_color = color::should_color(color::Stream::Stdout);
                let marker = styled_text("▸", styles::BOLD_CYAN, use_color);
                let name = styled_text(&view.name, styles::BOLD, use_color);
                let source = styled_text(&view.source, styles::DIM, use_color);
                writeln!(stdout, "{marker} {name} ({source})")
            }
            crate::cli::OutputFormat::Json => write_json_line(&mut stdout, view),
        }
    }

    #[allow(clippy::unused_self)]
    pub(crate) fn write_account_mutation(
        &self,
        verb: &'static str,
        view: &crate::commands::account::AccountMutationView,
        format: crate::cli::OutputFormat,
    ) -> std::io::Result<()> {
        let mut stdout = std::io::stdout().lock();
        match format {
            crate::cli::OutputFormat::Text => {
                let use_color = color::should_color(color::Stream::Stdout);
                let marker = styled_text("✓", styles::GREEN, use_color);
                let label = styled_text(format!("account {verb}:"), styles::BOLD_GREEN, use_color);
                let name = styled_text(&view.name, styles::BOLD, use_color);
                writeln!(stdout, "{marker} {label} {name}")?;
                writeln!(
                    stdout,
                    "{}  path: {}{}",
                    style_open(styles::DIM, use_color),
                    view.path,
                    style_close(styles::DIM, use_color),
                )?;
                Ok(())
            }
            crate::cli::OutputFormat::Json => write_json_line(&mut stdout, view),
        }
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
                    style_open(styles::DIM, c),
                    human_age(view.fetched_at_unix),
                    fetched_label(view),
                    view.ttl_secs,
                    style_close(styles::DIM, c),
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
                        style_open(styles::DIM, c),
                        human_age(view.fetched_at_unix),
                        fetched_label(view),
                        style_close(styles::DIM, c),
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
                let use_color = color::should_color(color::Stream::Stdout);
                if verbose {
                    write_health_verbose(&mut stdout, &view.entries, use_color)
                } else {
                    write_health_table(&mut stdout, &view.entries, use_color)
                }
            }
        }
    }

    #[allow(clippy::unused_self)]
    #[allow(clippy::too_many_lines)]
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
                let use_color = color::should_color(color::Stream::Stdout);
                let account_width = view
                    .entries
                    .iter()
                    .map(|entry| entry.account.len())
                    .chain(Some("ACCOUNT".len()))
                    .max()
                    .unwrap_or("ACCOUNT".len())
                    .max("ACCOUNT".len());
                let status_width = view
                    .entries
                    .iter()
                    .map(|entry| {
                        if entry.cooled_down {
                            "cooled-down"
                        } else {
                            "eligible"
                        }
                        .len()
                    })
                    .chain(Some("STATUS".len()))
                    .max()
                    .unwrap_or("STATUS".len())
                    .max("STATUS".len());
                let resets_width = view
                    .entries
                    .iter()
                    .map(|entry| {
                        entry
                            .reset_at_unix
                            .map_or_else(|| "—".len(), |value| human_duration_until(value).len())
                    })
                    .chain(Some("RESETS".len()))
                    .max()
                    .unwrap_or("RESETS".len())
                    .max("RESETS".len());
                writeln!(
                    stdout,
                    "{}  {}  {}  {}",
                    styled_padded("ACCOUNT", account_width, styles::DIM, use_color),
                    styled_padded("STATUS", status_width, styles::DIM, use_color),
                    styled_padded("RESETS", resets_width, styles::DIM, use_color),
                    styled_text("REASON", styles::DIM, use_color),
                )?;
                for entry in &view.entries {
                    let status = if entry.cooled_down {
                        styled_padded("cooled-down", status_width, styles::BOLD_RED, use_color)
                    } else {
                        styled_padded("eligible", status_width, styles::BOLD_GREEN, use_color)
                    };
                    let resets = entry.reset_at_unix.map_or_else(
                        || styled_padded("—", resets_width, styles::DIM, use_color),
                        |value| {
                            styled_padded(
                                human_duration_until(value),
                                resets_width,
                                styles::BOLD_YELLOW,
                                use_color,
                            )
                        },
                    );
                    let reason = entry.reason.as_ref().map_or_else(
                        || styled_text("—", styles::DIM, use_color),
                        |reason| styled_text(reason, anstyle::Style::new(), use_color),
                    );
                    writeln!(
                        stdout,
                        "{}  {}  {}  {}",
                        styled_padded(&entry.account, account_width, styles::BOLD, use_color),
                        status,
                        resets,
                        reason
                    )?;
                }
                Ok(())
            }
            crate::cli::OutputFormat::Json => write_json_line(&mut stdout, &view.entries),
        }
    }

    #[allow(clippy::unused_self)]
    pub(crate) fn write_config_recipe_compose(
        &self,
        view: &crate::commands::config_recipe_compose::ConfigRecipeComposeView,
    ) -> std::io::Result<()> {
        let mut stdout = std::io::stdout().lock();
        writeln!(
            stdout,
            "config-recipe:      {}",
            view.config_recipe.as_deref().unwrap_or("(stock mode)")
        )?;
        writeln!(stdout, "group-id:     {}", view.group_id)?;
        writeln!(stdout, "session-dir:  {}", view.session_dir)?;
        writeln!(stdout, "config:       {}", view.config_path)?;
        writeln!(stdout, "sidecar:      {}", view.sidecar_path)?;
        writeln!(stdout, "session-meta: {}", view.session_meta_path)?;
        // Emitted profile siblings, one per `profiles/<name>.config.toml`
        // input. Suppressed entirely when no profiles were emitted so the
        // output stays minimal in stock mode and for recipes without
        // profile files.
        if !view.profile_paths.is_empty() {
            writeln!(stdout, "profiles:")?;
            for profile in &view.profile_paths {
                writeln!(stdout, "  {}: {}", profile.name, profile.path)?;
            }
        }
        Ok(())
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

#[allow(clippy::too_many_lines)]
fn write_health_verbose(
    stdout: &mut impl std::io::Write,
    entries: &[crate::commands::account::AccountHealthEntryView],
    use_color: bool,
) -> std::io::Result<()> {
    for (idx, entry) in entries.iter().enumerate() {
        if idx > 0 {
            writeln!(stdout)?;
        }
        writeln!(
            stdout,
            "account: {}",
            styled_text(&entry.account, styles::BOLD, use_color)
        )?;
        writeln!(
            stdout,
            "rank: {}",
            entry.rank.map_or_else(
                || styled_text("—", styles::DIM, use_color),
                |r| r.to_string()
            )
        )?;
        writeln!(stdout, "score: {}", entry.score_label)?;
        writeln!(stdout, "plan: {}", entry.plan)?;
        writeln!(
            stdout,
            "token: {}",
            styled_text(&entry.token, health_token_style(&entry.token), use_color)
        )?;
        writeln!(stdout, "token_detail: {}", entry.token_detail)?;
        writeln!(
            stdout,
            "status: {}",
            styled_text(&entry.status, health_status_style(&entry.status), use_color)
        )?;
        writeln!(
            stdout,
            "active: {}",
            if entry.active {
                styled_text("true", styles::BOLD_CYAN, use_color)
            } else {
                "false".to_owned()
            }
        )?;
        writeln!(
            stdout,
            "cooldown: {}",
            if entry.cooldown {
                styled_text("true", styles::BOLD_RED, use_color)
            } else {
                "false".to_owned()
            }
        )?;
        writeln!(
            stdout,
            "last_used: {}",
            entry.last_used.map_or_else(
                || styled_text("—", styles::DIM, use_color),
                |ts| styled_text(human_age(ts), styles::DIM, use_color),
            )
        )?;
        writeln!(
            stdout,
            "fetched: {}",
            styled_fetched_age(entry.fetched_at_unix, use_color)
        )?;
        if let Some(ref scoring) = entry.scoring {
            writeln!(stdout, "scoring_base: {:.2}", scoring.base)?;
            writeln!(stdout, "scoring_plan_bonus: {:.2}", scoring.plan_bonus)?;
            writeln!(stdout, "scoring_recency: {:.2}", scoring.recency)?;
            writeln!(stdout, "scoring_recency_label: {}", scoring.recency_label)?;
            writeln!(stdout, "scoring_avail_score: {:.2}", scoring.avail_score)?;
            if let Some(fh) = scoring.five_hour_pct {
                writeln!(stdout, "scoring_five_hour_pct: {fh:.1}")?;
            }
            if let Some(wk) = scoring.weekly_pct {
                writeln!(stdout, "scoring_weekly_pct: {wk:.1}")?;
            }
            writeln!(
                stdout,
                "scoring_five_hour_weight: {:.2}",
                scoring.five_hour_weight
            )?;
            writeln!(
                stdout,
                "scoring_weekly_pressure: {:.2}",
                scoring.weekly_pressure
            )?;
            writeln!(stdout, "scoring_fh_pressure: {:.2}", scoring.fh_pressure)?;
            writeln!(stdout, "scoring_pressure_label: {}", scoring.pressure_label)?;
            writeln!(stdout, "scoring_total: {:.2}", scoring.total)?;
            writeln!(stdout, "scoring_eligible: {}", scoring.eligible)?;
            if let Some(ref reason) = scoring.ineligible_reason {
                writeln!(stdout, "scoring_ineligible_reason: {reason}")?;
            }
            if let Some(tie) = scoring.tie_five_hour {
                writeln!(stdout, "scoring_tie_five_hour: {tie:.1}")?;
            }
        }
    }
    Ok(())
}

#[allow(clippy::too_many_lines)]
fn write_health_table(
    stdout: &mut impl std::io::Write,
    entries: &[crate::commands::account::AccountHealthEntryView],
    use_color: bool,
) -> std::io::Result<()> {
    let rank_w = entries
        .iter()
        .map(|entry| entry.rank.map_or(1, |rank| rank.to_string().len()))
        .chain(Some("RANK".len()))
        .max()
        .unwrap_or("RANK".len())
        .max("RANK".len());
    let score_w = entries
        .iter()
        .map(|entry| entry.score_label.len())
        .chain(Some("SCORE".len()))
        .max()
        .unwrap_or("SCORE".len())
        .max("SCORE".len());
    let account_w = entries
        .iter()
        .map(|entry| entry.account.len())
        .chain(Some("ACCOUNT".len()))
        .max()
        .unwrap_or("ACCOUNT".len())
        .max("ACCOUNT".len());
    let token_w = entries
        .iter()
        .map(|entry| entry.token.len())
        .chain(Some("TOKEN".len()))
        .max()
        .unwrap_or("TOKEN".len())
        .max("TOKEN".len());
    let plan_w = entries
        .iter()
        .map(|entry| entry.plan.len())
        .chain(Some("PLAN".len()))
        .max()
        .unwrap_or("PLAN".len())
        .max("PLAN".len());
    let status_w = entries
        .iter()
        .map(|entry| entry.status.len())
        .chain(Some("STATUS".len()))
        .max()
        .unwrap_or("STATUS".len())
        .max("STATUS".len());
    let active_w = "ACTIVE".len();
    let cooldown_w = "COOLDOWN".len();
    writeln!(
        stdout,
        "{}  {}  {}  {}  {}  {}  {}  {}  {}",
        styled_padded_right("RANK", rank_w, styles::DIM, use_color),
        styled_padded_right("SCORE", score_w, styles::DIM, use_color),
        styled_padded("ACCOUNT", account_w, styles::DIM, use_color),
        styled_padded("TOKEN", token_w, styles::DIM, use_color),
        styled_padded("PLAN", plan_w, styles::DIM, use_color),
        styled_padded("STATUS", status_w, styles::DIM, use_color),
        styled_padded("ACTIVE", active_w, styles::DIM, use_color),
        styled_padded("COOLDOWN", cooldown_w, styles::DIM, use_color),
        styled_text("FETCHED", styles::DIM, use_color),
    )?;
    for entry in entries {
        writeln!(
            stdout,
            "{}  {}  {}  {}  {}  {}  {}  {}  {}",
            styled_padded_right(
                entry.rank.map_or_else(|| "—".to_owned(), |r| r.to_string()),
                rank_w,
                if entry.rank.is_some() {
                    anstyle::Style::new()
                } else {
                    styles::DIM
                },
                use_color,
            ),
            styled_padded_right(
                &entry.score_label,
                score_w,
                anstyle::Style::new(),
                use_color
            ),
            styled_padded(
                &entry.account,
                account_w,
                if entry.active {
                    styles::BOLD
                } else {
                    anstyle::Style::new()
                },
                use_color,
            ),
            styled_padded(
                &entry.token,
                token_w,
                health_token_style(&entry.token),
                use_color,
            ),
            styled_padded(&entry.plan, plan_w, anstyle::Style::new(), use_color),
            styled_padded(
                &entry.status,
                status_w,
                health_status_style(&entry.status),
                use_color,
            ),
            styled_padded(
                if entry.active { "true" } else { "false" },
                active_w,
                if entry.active {
                    styles::BOLD_CYAN
                } else {
                    anstyle::Style::new()
                },
                use_color,
            ),
            styled_padded(
                if entry.cooldown { "true" } else { "false" },
                cooldown_w,
                if entry.cooldown {
                    styles::BOLD_RED
                } else {
                    anstyle::Style::new()
                },
                use_color,
            ),
            styled_fetched_age(entry.fetched_at_unix, use_color),
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

mod styles {
    use anstyle::{AnsiColor, Effects, Style};

    pub(super) const BOLD: Style = Style::new().effects(Effects::BOLD);
    pub(super) const DIM: Style = Style::new().effects(Effects::DIMMED);
    pub(super) const BOLD_CYAN: Style = Style::new()
        .fg_color(Some(anstyle::Color::Ansi(AnsiColor::Cyan)))
        .effects(Effects::BOLD);
    pub(super) const GREEN: Style =
        Style::new().fg_color(Some(anstyle::Color::Ansi(AnsiColor::Green)));
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

fn styled_text(text: impl AsRef<str>, style: anstyle::Style, use_color: bool) -> String {
    let text = text.as_ref();
    format!(
        "{}{}{}",
        style_open(style, use_color),
        text,
        style_close(style, use_color)
    )
}

fn styled_padded(
    text: impl AsRef<str>,
    width: usize,
    style: anstyle::Style,
    use_color: bool,
) -> String {
    let text = format!("{:<width$}", text.as_ref(), width = width);
    format!(
        "{}{}{}",
        style_open(style, use_color),
        text,
        style_close(style, use_color)
    )
}

/// Right-align a cell to `width`, then wrap in `style`. Use for numeric columns
/// (style guide §8: numeric columns are right-aligned). Padding is computed on
/// the plain text before styling so column alignment is independent of color.
fn styled_padded_right(
    text: impl AsRef<str>,
    width: usize,
    style: anstyle::Style,
    use_color: bool,
) -> String {
    let text = format!("{:>width$}", text.as_ref(), width = width);
    format!(
        "{}{}{}",
        style_open(style, use_color),
        text,
        style_close(style, use_color)
    )
}

fn health_token_style(token: &str) -> anstyle::Style {
    match token {
        "ok" => styles::BOLD_GREEN,
        "invalid" => styles::BOLD_RED,
        _ if token.contains("unknown") => styles::BOLD_YELLOW,
        _ => styles::BOLD,
    }
}

fn health_status_style(status: &str) -> anstyle::Style {
    match status {
        "live" => styles::BOLD_GREEN,
        "cache only" => styles::BOLD_YELLOW,
        "cache missing" | "fetch failed" => styles::BOLD_RED,
        _ => styles::BOLD,
    }
}

/// Render a `fetched_at` unix timestamp for text output, using the `—`
/// missing-data sentinel (in `DIM`) when the timestamp is `0` (no cached data
/// or a failed refresh) instead of `human_age`'s `unknown` token.
fn styled_fetched_age(fetched_at_unix: u64, use_color: bool) -> String {
    if fetched_at_unix == 0 {
        styled_text("—", styles::DIM, use_color)
    } else {
        styled_text(human_age(fetched_at_unix), styles::DIM, use_color)
    }
}

fn percent_style(pct: f64) -> anstyle::Style {
    if pct > 50.0 {
        styles::BOLD_GREEN
    } else if pct > 20.0 {
        styles::BOLD_YELLOW
    } else {
        styles::BOLD_RED
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

fn write_quota_oauth_header(
    stdout: &mut impl std::io::Write,
    view: &crate::commands::account::AccountQuotaEntryView,
    use_color: bool,
) -> std::io::Result<()> {
    if let Some(rank) = view.rank {
        write!(
            stdout,
            "  {}#{} {:.2} {}{}{}{}",
            style_open(styles::DIM, use_color),
            rank,
            view.score.unwrap_or(0.0),
            style_close(styles::DIM, use_color),
            style_open(styles::BOLD, use_color),
            view.account,
            style_close(styles::BOLD, use_color),
        )?;
    } else {
        write!(
            stdout,
            "  {}{:.2} {}{}{}{}",
            style_open(styles::DIM, use_color),
            view.score.unwrap_or(0.0),
            style_close(styles::DIM, use_color),
            style_open(styles::BOLD, use_color),
            view.account,
            style_close(styles::BOLD, use_color),
        )?;
    }
    if view.active {
        write!(
            stdout,
            " {}(active){}",
            style_open(styles::BOLD_CYAN, use_color),
            style_close(styles::BOLD_CYAN, use_color),
        )?;
    }
    writeln!(stdout)
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
            write_quota_oauth_header(stdout, view, use_color)?;
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
                style_open(styles::BOLD, use_color),
                view.account,
                style_close(styles::BOLD, use_color),
                style_open(styles::DIM, use_color),
                style_close(styles::DIM, use_color),
            )?;
            writeln!(stdout, "  Quota not available (API-key auth)")?;
        }
        _ => {
            write!(
                stdout,
                "  {}{}{}",
                style_open(styles::BOLD, use_color),
                view.account,
                style_close(styles::BOLD, use_color),
            )?;
            if view.active {
                write!(
                    stdout,
                    " {}(active){}",
                    style_open(styles::BOLD_CYAN, use_color),
                    style_close(styles::BOLD_CYAN, use_color),
                )?;
            }
            writeln!(stdout)?;
            writeln!(
                stdout,
                "  {}Error: {}{}",
                style_open(styles::RED, use_color),
                view.error.as_deref().unwrap_or("unknown"),
                style_close(styles::RED, use_color),
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
