//! Spinner helpers for wrapper-owned live progress narration.

use std::borrow::Cow;
use std::io::IsTerminal as _;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

use indicatif::{MultiProgress, ProgressBar, ProgressDrawTarget, ProgressStyle};

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
    #[allow(dead_code)]
    pub(crate) fn clear(&self) -> std::io::Result<()> {
        self.multi.clear()
    }
}

/// Handle for one spinner line.
pub(crate) struct SpinnerHandle {
    bar: ProgressBar,
    finished: Arc<AtomicBool>,
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
        self.finish_with_marker(finish_ok_message(msg, self.use_color));
    }

    /// Finish the spinner with a failure marker.
    pub(crate) fn finish_err(&self, msg: &str) {
        self.finish_with_marker(finish_err_message(msg, self.use_color));
    }

    /// Finish and clear the spinner line.
    pub(crate) fn finish_and_clear(&self) {
        if !self.finished.swap(true, Ordering::SeqCst) {
            self.bar.finish_and_clear();
        }
    }

    /// Clear this spinner line.
    #[allow(dead_code)]
    pub(crate) fn clear(&self) {
        self.finish_and_clear();
    }

    fn finish_with_marker(&self, message: String) {
        if !self.finished.swap(true, Ordering::SeqCst) {
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
    matches!(format, crate::cli::OutputFormat::Text)
        && !suppress
        && !ctx.global.quiet
        && !ctx.global.silent
        && std::io::stderr().is_terminal()
        && matches!(
            crate::logging::StderrMirror::from_cli(
                ctx.global.quiet,
                ctx.global.silent,
                ctx.global.log_stderr || ctx.config.log.mirror_stderr,
                ctx.global.log_format.or(ctx.config.log.stderr_format),
                ctx.global.verbose.max(ctx.config.log.verbose),
            ),
            crate::logging::StderrMirror::Off
        )
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
