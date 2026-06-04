//! Spinner helpers for wrapper-owned live progress narration.

use std::borrow::Cow;
use std::io::{IsTerminal as _, Write as _};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

use indicatif::{MultiProgress, ProgressBar, ProgressDrawTarget, ProgressStyle};

const MAX_FINISH_MESSAGE_CHARS: usize = 60;

/// A group of spinner lines that share one stderr draw target.
pub(crate) struct SpinnerGroup {
    multi: MultiProgress,
    visible: bool,
    use_color: bool,
}

impl SpinnerGroup {
    /// Create a spinner group. Hidden groups accept updates but render nothing.
    pub(crate) fn new(visible: bool) -> Self {
        let draw_target = if visible {
            ProgressDrawTarget::term_like_with_hz(Box::new(console::Term::buffered_stderr()), 20)
        } else {
            ProgressDrawTarget::hidden()
        };
        Self {
            multi: MultiProgress::with_draw_target(draw_target),
            visible,
            use_color: visible && super::color::should_color(super::color::Stream::Stderr),
        }
    }

    /// Add a spinner with an initial message.
    pub(crate) fn add(&self, message: &str) -> SpinnerHandle {
        let bar = self.multi.add(ProgressBar::new_spinner());
        bar.set_style(spinner_style(self.use_color));
        bar.set_message(message.to_owned());
        if self.visible {
            bar.enable_steady_tick(Duration::from_millis(80));
        }
        SpinnerHandle {
            bar,
            finished: Arc::new(AtomicBool::new(false)),
            visible: self.visible,
            use_color: self.use_color,
        }
    }

    /// Suspend spinner rendering while a closure writes to stderr.
    #[allow(dead_code)]
    pub(crate) fn suspend<F, R>(&self, f: F) -> R
    where
        F: FnOnce() -> R,
    {
        self.multi.suspend(f)
    }

    /// Clear all visible spinner lines.
    pub(crate) fn clear(&self) -> std::io::Result<()> {
        self.multi.clear()
    }
}

/// Handle for one spinner line.
pub(crate) struct SpinnerHandle {
    bar: ProgressBar,
    finished: Arc<AtomicBool>,
    visible: bool,
    use_color: bool,
}

impl SpinnerHandle {
    /// Update the spinner message.
    #[allow(dead_code)]
    pub(crate) fn set_message(&self, msg: impl Into<Cow<'static, str>>) {
        if !self.finished.load(Ordering::SeqCst) {
            self.bar.set_message(msg);
        }
    }

    /// Finish the spinner with a success marker.
    pub(crate) fn finish_ok(&self, msg: &str) {
        self.finish_with_marker(msg, true);
    }

    /// Finish the spinner with a failure marker.
    pub(crate) fn finish_err(&self, msg: &str) {
        self.finish_with_marker(msg, false);
    }

    /// Finish and clear the spinner line.
    pub(crate) fn finish_and_clear(&self) {
        if !self.finished.swap(true, Ordering::SeqCst) {
            self.bar.finish_and_clear();
        }
    }

    /// Finish and clear before handing the terminal to an interactive child.
    pub(crate) fn finish_and_clear_for_child(&self) {
        let visible = self.visible;
        self.finish_and_clear();
        if visible {
            let _ = std::io::stderr().write_all(b"\n");
            let _ = std::io::stderr().flush();
        }
    }

    /// Clear this spinner line.
    #[allow(dead_code)]
    pub(crate) fn clear(&self) {
        self.finish_and_clear();
    }

    fn finish_with_marker(&self, msg: &str, success: bool) {
        if !self.finished.swap(true, Ordering::SeqCst) {
            let msg = truncate_finish_message(msg);
            let message = if success {
                finish_ok_message(&msg, self.use_color)
            } else {
                finish_err_message(&msg, self.use_color)
            };
            self.bar.set_style(finish_style());
            self.bar.finish_with_message(message);
        }
    }
}

impl Drop for SpinnerHandle {
    fn drop(&mut self) {
        if !self.finished.swap(true, Ordering::SeqCst) {
            self.bar.finish_and_clear();
        }
    }
}

/// Return true when spinners may render visibly for this command invocation.
pub(crate) fn should_show_spinner(
    ctx: &crate::context::AppContext,
    format: crate::cli::OutputFormat,
    suppress: bool,
) -> bool {
    let mirror_off = matches!(
        crate::logging::StderrMirror::from_cli(
            ctx.global.quiet,
            ctx.global.silent,
            ctx.global.log_stderr || ctx.config.log.mirror_stderr,
            ctx.global.log_format.or(ctx.config.log.stderr_format),
            ctx.global.verbose.max(ctx.config.log.verbose),
        ),
        crate::logging::StderrMirror::Off
    );
    spinner_policy(
        format,
        ctx.global.quiet,
        ctx.global.silent,
        suppress,
        std::io::stderr().is_terminal(),
        mirror_off,
    )
}

#[allow(
    clippy::fn_params_excessive_bools,
    reason = "Policy helper intentionally exposes each suppression axis for direct unit tests."
)]
const fn spinner_policy(
    format: crate::cli::OutputFormat,
    quiet: bool,
    silent: bool,
    suppress: bool,
    stderr_is_tty: bool,
    mirror_off: bool,
) -> bool {
    matches!(format, crate::cli::OutputFormat::Text)
        && !quiet
        && !silent
        && !suppress
        && stderr_is_tty
        && mirror_off
}

fn spinner_style(use_color: bool) -> ProgressStyle {
    let template = if use_color {
        "{spinner:.cyan} {msg}"
    } else {
        "{spinner} {msg}"
    };
    ProgressStyle::with_template(template).unwrap_or_else(|_| ProgressStyle::default_spinner())
}

fn finish_style() -> ProgressStyle {
    ProgressStyle::with_template("{msg}").unwrap_or_else(|_| ProgressStyle::default_spinner())
}

fn finish_ok_message(msg: &str, use_color: bool) -> String {
    if use_color {
        styled_marker("✓", green_style(), msg, use_color)
    } else {
        format!("[ok] {msg}")
    }
}

fn finish_err_message(msg: &str, use_color: bool) -> String {
    if use_color {
        styled_marker("✗", red_style(), msg, use_color)
    } else {
        format!("[err] {msg}")
    }
}

fn styled_marker(marker: &str, style: anstyle::Style, msg: &str, use_color: bool) -> String {
    format!(
        "{}{marker} {msg}{}",
        super::style_open(style, use_color),
        super::style_close(style, use_color)
    )
}

const fn green_style() -> anstyle::Style {
    anstyle::Style::new().fg_color(Some(anstyle::Color::Ansi(anstyle::AnsiColor::Green)))
}

const fn red_style() -> anstyle::Style {
    anstyle::Style::new().fg_color(Some(anstyle::Color::Ansi(anstyle::AnsiColor::Red)))
}

fn truncate_finish_message(msg: &str) -> String {
    let char_count = msg.chars().count();
    if char_count <= MAX_FINISH_MESSAGE_CHARS {
        return msg.to_owned();
    }
    msg.chars()
        .take(MAX_FINISH_MESSAGE_CHARS.saturating_sub(3))
        .chain("...".chars())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spinner_policy_hides_json() {
        assert!(!spinner_policy(
            crate::cli::OutputFormat::Json,
            false,
            false,
            false,
            true,
            true,
        ));
    }

    #[test]
    fn spinner_policy_hides_non_tty() {
        assert!(!spinner_policy(
            crate::cli::OutputFormat::Text,
            false,
            false,
            false,
            false,
            true,
        ));
    }

    #[test]
    fn spinner_policy_hides_quiet() {
        assert!(!spinner_policy(
            crate::cli::OutputFormat::Text,
            true,
            false,
            false,
            true,
            true,
        ));
    }

    #[test]
    fn spinner_policy_hides_silent() {
        assert!(!spinner_policy(
            crate::cli::OutputFormat::Text,
            false,
            true,
            false,
            true,
            true,
        ));
    }

    #[test]
    fn spinner_policy_hides_suppressed_command() {
        assert!(!spinner_policy(
            crate::cli::OutputFormat::Text,
            false,
            false,
            true,
            true,
            true,
        ));
    }

    #[test]
    fn spinner_policy_hides_stderr_mirror() {
        assert!(!spinner_policy(
            crate::cli::OutputFormat::Text,
            false,
            false,
            false,
            true,
            false,
        ));
    }

    #[test]
    fn spinner_policy_shows_when_all_clear() {
        assert!(spinner_policy(
            crate::cli::OutputFormat::Text,
            false,
            false,
            false,
            true,
            true,
        ));
    }

    #[test]
    fn finish_ok_message_uses_colored_check() {
        let message = finish_ok_message("done", true);
        assert!(message.contains("✓ done"));
    }

    #[test]
    fn finish_ok_message_uses_ascii_when_plain() {
        assert_eq!(finish_ok_message("done", false), "[ok] done");
    }

    #[test]
    fn finish_err_message_uses_colored_cross() {
        let message = finish_err_message("failed", true);
        assert!(message.contains("✗ failed"));
    }

    #[test]
    fn finish_err_message_uses_ascii_when_plain() {
        assert_eq!(finish_err_message("failed", false), "[err] failed");
    }

    #[test]
    fn finish_message_truncates_to_sixty_chars() {
        let raw = "a".repeat(MAX_FINISH_MESSAGE_CHARS + 10);
        let truncated = truncate_finish_message(&raw);
        assert_eq!(truncated.chars().count(), MAX_FINISH_MESSAGE_CHARS);
        assert!(truncated.ends_with("..."));
    }

    #[test]
    fn dropping_unfinished_hidden_spinner_does_not_panic() {
        let group = SpinnerGroup::new(false);
        let handle = group.add("Hidden spinner");
        drop(handle);
    }
}
