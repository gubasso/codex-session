#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

pub mod support;

use support::TestEnv;

fn has_ansi(bytes: &[u8]) -> bool {
    String::from_utf8_lossy(bytes).contains('\u{1b}')
}

#[test]
fn no_color_disables_help_ansi() {
    let env = TestEnv::new();
    let output = env
        .cmd()
        .env("NO_COLOR", "1")
        .arg("--help")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert!(!has_ansi(&output));
}

#[test]
fn force_color_enables_help_ansi_when_piped() {
    let env = TestEnv::new();
    let output = env
        .cmd()
        .env("FORCE_COLOR", "1")
        .arg("--help")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert!(has_ansi(&output));
}

#[test]
fn clicolor_zero_disables_help_ansi() {
    let env = TestEnv::new();
    let output = env
        .cmd()
        .env("CLICOLOR", "0")
        .arg("--help")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert!(!has_ansi(&output));
}

#[test]
fn no_color_wins_over_clicolor_force() {
    let env = TestEnv::new();
    let output = env
        .cmd()
        .env("NO_COLOR", "1")
        .env("CLICOLOR_FORCE", "1")
        .arg("--help")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert!(!has_ansi(&output));
}

#[test]
fn force_color_zero_does_not_force_help_ansi() {
    let env = TestEnv::new();
    let output = env
        .cmd()
        .env("FORCE_COLOR", "0")
        .arg("--help")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert!(!has_ansi(&output));
}

#[test]
fn clicolor_force_enables_help_ansi_when_piped() {
    let env = TestEnv::new();
    let output = env
        .cmd()
        .env("CLICOLOR_FORCE", "1")
        .arg("--help")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert!(has_ansi(&output));
}

#[test]
fn stderr_color_policy_matches_help_policy() {
    // Cover the full documented precedence matrix on the stderr error
    // renderer (NO_COLOR > FORCE_COLOR/CLICOLOR_FORCE > isatty > CLICOLOR).
    // `exec` with no child resolved exits non-zero and emits the renderer
    // output we want to inspect.
    let env = TestEnv::new();

    // FORCE_COLOR=1 → ANSI on stderr.
    let forced = env
        .cmd()
        .env("FORCE_COLOR", "1")
        .arg("exec")
        .assert()
        .failure()
        .get_output()
        .stderr
        .clone();
    assert!(has_ansi(&forced));

    // CLICOLOR_FORCE=1 → ANSI on stderr.
    let cli_forced = env
        .cmd()
        .env("CLICOLOR_FORCE", "1")
        .arg("exec")
        .assert()
        .failure()
        .get_output()
        .stderr
        .clone();
    assert!(has_ansi(&cli_forced));

    // NO_COLOR=1 wins even over CLICOLOR_FORCE=1.
    let no_color_wins = env
        .cmd()
        .env("NO_COLOR", "1")
        .env("CLICOLOR_FORCE", "1")
        .arg("exec")
        .assert()
        .failure()
        .get_output()
        .stderr
        .clone();
    assert!(!has_ansi(&no_color_wins));

    // NO_COLOR=1 alone → no ANSI on stderr.
    let no_color = env
        .cmd()
        .env("NO_COLOR", "1")
        .arg("exec")
        .assert()
        .failure()
        .get_output()
        .stderr
        .clone();
    assert!(!has_ansi(&no_color));

    // FORCE_COLOR=0 falls through to the (piped → non-TTY) branch → no ANSI.
    let force_color_zero = env
        .cmd()
        .env("FORCE_COLOR", "0")
        .arg("exec")
        .assert()
        .failure()
        .get_output()
        .stderr
        .clone();
    assert!(!has_ansi(&force_color_zero));

    // CLICOLOR=0 with no overrides → no ANSI (piped → non-TTY already
    // suppresses; this asserts CLICOLOR is not accidentally re-enabling).
    let clicolor_zero = env
        .cmd()
        .env("CLICOLOR", "0")
        .arg("exec")
        .assert()
        .failure()
        .get_output()
        .stderr
        .clone();
    assert!(!has_ansi(&clicolor_zero));
}
