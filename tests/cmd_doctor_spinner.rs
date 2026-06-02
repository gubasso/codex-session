#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

mod support;

use support::{FakeCodexBehavior, TestEnv};

fn install_minimal_recipe(env: &TestEnv) {
    env.install_config_recipe(
        "default",
        "config-layers:\n  - base\n",
        &[("base", "[model]\ndefault = \"gpt-5\"\n")],
    );
    env.make_fake_codex_with_version("codex 0.134.0", FakeCodexBehavior::Succeed);
}

fn normalize_doctor_text(env: &TestEnv, stdout: &str) -> String {
    let normalized = env.normalize_text(stdout);
    normalized
        .lines()
        .map(|line| {
            if line.starts_with("group-id:") {
                "group-id:        <GROUP_ID>".to_owned()
            } else if line.starts_with("codex_home:") {
                let prefix = "codex_home:      ";
                let path = line.strip_prefix(prefix).unwrap_or(line);
                let normalized_path = path.split_once("/groups/").map_or_else(
                    || path.to_owned(),
                    |(base, _)| format!("{base}/groups/<GROUP_ID>"),
                );
                format!("{prefix}{normalized_path}")
            } else if line.trim_start().starts_with("default current=true") {
                let prefix = "  default current=true has_auth=✓ last used ";
                line.strip_prefix(prefix).map_or_else(
                    || line.to_owned(),
                    |rest| {
                        let suffix = rest
                            .split_once(" cooldown=")
                            .map_or("", |(_, suffix)| suffix);
                        format!("{prefix}<LAST_USED_AT> cooldown={suffix}")
                    },
                )
            } else {
                line.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

#[test]
fn doctor_text_piped_output_is_clean() {
    let env = TestEnv::new();
    install_minimal_recipe(&env);

    let output = env
        .cmd()
        .args(["doctor", "--format", "text"])
        .assert()
        .success()
        .get_output()
        .clone();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        stdout.contains("OK") || stdout.contains("WARN") || stdout.contains("FAIL"),
        "doctor text output should include check status tokens:\n{stdout}"
    );
    assert!(
        stdout.contains("ENVIRONMENT"),
        "doctor text output should include section titles:\n{stdout}"
    );
    assert!(
        stdout.contains('✓'),
        "doctor text output should include UTF-8 status symbols:\n{stdout}"
    );
    assert!(!stdout.contains('\u{1b}'), "stdout contains ANSI escapes");
    assert!(!stdout.contains('⠋'), "stdout contains spinner frame ⠋");
    assert!(!stdout.contains('⠙'), "stdout contains spinner frame ⠙");
    assert!(!stderr.contains('\u{1b}'), "stderr contains ANSI escapes");
}

#[test]
fn doctor_json_output_is_valid_json() {
    let env = TestEnv::new();
    install_minimal_recipe(&env);

    let output = env
        .cmd()
        .args(["doctor", "--format", "json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(value.is_object(), "doctor json output should be an object");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.contains('\u{1b}'), "stderr contains ANSI escapes");
}

#[test]
fn doctor_text_output_snapshot() {
    let env = TestEnv::new();
    install_minimal_recipe(&env);

    let output = env
        .cmd()
        .args(["doctor", "--format", "text"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(output).unwrap();

    insta::assert_snapshot!(normalize_doctor_text(&env, &stdout));
}
