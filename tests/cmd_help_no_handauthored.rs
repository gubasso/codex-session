#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]
#![allow(missing_docs)]

pub mod support;

use predicates::prelude::*;
use support::{TestEnv, color};

#[test]
fn help_text_is_clap_generated() {
    let env = TestEnv::new();
    let mut cmd = env.cmd();
    let assertion = color::with_no_color(&mut cmd)
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage:"))
        .stdout(predicate::str::contains("version"))
        .stdout(predicate::str::contains("completion"))
        .stdout(predicate::str::contains("config"))
        .stdout(predicate::str::contains("help"))
        .stdout(predicate::str::contains("CODEX_SESSION_CHILD_BIN"))
        .stdout(predicate::str::contains("Wrapper verbs:").not())
        .stdout(predicate::str::contains("Wrapper options:").not());
    let _ = assertion;
}

/// The `help_extras.txt` addendum must not re-introduce hand-authored
/// content that clap already renders. Per the plan, the addendum is
/// strictly post-clap narrative (passthrough, env vars, examples) — it
/// must not duplicate the `Usage:` block, the auto-generated `--help`
/// / `--version` rows, or a second `Commands:` / `Options:` table.
#[test]
fn help_extras_does_not_duplicate_clap_sections() {
    let env = TestEnv::new();
    let mut cmd = env.cmd();
    let output = color::with_no_color(&mut cmd)
        .arg("--help")
        .output()
        .unwrap();
    assert!(output.status.success(), "--help should succeed");
    let stdout = String::from_utf8(output.stdout).unwrap();

    // Only one clap-rendered `Usage:` section in the long help output.
    let usage_count = stdout.matches("Usage:").count();
    assert_eq!(
        usage_count, 1,
        "expected exactly one `Usage:` block in long help, got {usage_count}:\n{stdout}"
    );

    // The addendum begins after clap's last section. Slice off everything
    // through the first occurrence of the addendum's lead-in
    // (`WRAPPER OVERVIEW`) and assert the remainder does not re-introduce
    // section headers or flag rows clap already owns.
    let addendum_start = stdout
        .find("WRAPPER OVERVIEW")
        .expect("help_extras.txt addendum should appear in long help");
    let addendum = &stdout[addendum_start..];
    let needles = ["Usage:", "Commands:", "Options:"];
    for needle in needles {
        let header = format!("addendum must not re-introduce `{needle}` (clap renders it):");
        let msg = format!("{header}\n--- addendum ---\n{addendum}");
        assert!(!addendum.contains(needle), "{msg}");
    }
}
