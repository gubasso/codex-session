#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

mod support;

use support::TestEnv;

#[test]
fn doctor_reports_accounts_section() {
    let env = TestEnv::new();
    env.install_config_recipe(
        "default",
        "config-layers:\n  - base\n",
        &[("base", "[model]\ndefault = \"gpt-5\"\n")],
    );
    env.make_fake_codex_printing_stdout("ignored");
    env.seed_account("work", r#"{"tokens":{"access_token":"test-token"}}"#);

    let output = env
        .cmd()
        .args(["--format", "json", "doctor"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(value["active-account"]["name"], "work");
    assert_eq!(value["active-account"]["source"], "auto");
    assert!(value["accounts"].as_array().is_some());
}
