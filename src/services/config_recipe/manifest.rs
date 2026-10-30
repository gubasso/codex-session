//! Config-recipe manifest parsing.
//!
//! What this is: YAML parsing and schema validation for `config-recipes/*.yaml`.
//! What this is not: TOML layer parsing or merge logic.

#![allow(clippy::result_large_err)]

use camino::Utf8PathBuf;
use serde::Deserialize;

#[derive(Debug, Clone)]
pub(crate) struct Manifest {
    pub(crate) path: Utf8PathBuf,
    pub(crate) config_layers: Vec<String>,
    pub(crate) profile_files: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawManifest {
    #[serde(rename = "config-layers")]
    config_layers: serde_yaml_ng::Value,
    #[serde(rename = "profile-files", default)]
    profile_files: Option<serde_yaml_ng::Value>,
}

impl Manifest {
    pub(crate) fn parse(path: Utf8PathBuf) -> Result<Self, crate::config::ConfigError> {
        let contents = std::fs::read_to_string(path.as_std_path())?;
        let raw: RawManifest = serde_yaml_ng::from_str(&contents).map_err(|source| {
            crate::config::ConfigError::ManifestParse {
                path: path.clone(),
                source,
            }
        })?;

        let layers_value = raw.config_layers;
        let serde_yaml_ng::Value::Sequence(layers) = layers_value else {
            return Err(crate::config::ConfigError::ManifestSchema {
                path,
                reason: "`config-layers` must be a non-empty array of strings".to_owned(),
            });
        };

        if layers.is_empty() {
            return Err(crate::config::ConfigError::ManifestSchema {
                path,
                reason: "`config-layers` must not be empty".to_owned(),
            });
        }

        let mut config_layers = Vec::with_capacity(layers.len());
        for item in layers {
            let serde_yaml_ng::Value::String(name) = item else {
                return Err(crate::config::ConfigError::ManifestSchema {
                    path,
                    reason: "`config-layers` entries must be strings".to_owned(),
                });
            };
            if !is_valid_layer_name(&name) {
                return Err(crate::config::ConfigError::ManifestSchema {
                    path,
                    reason: format!("invalid config layer name `{name}`"),
                });
            }
            config_layers.push(name);
        }

        let profile_files = match raw.profile_files {
            None => None,
            Some(serde_yaml_ng::Value::Sequence(items)) => {
                if items.is_empty() {
                    return Err(crate::config::ConfigError::ManifestSchema {
                        path,
                        reason: "`profile-files` must not be empty when present".to_owned(),
                    });
                }
                let mut names = Vec::with_capacity(items.len());
                let mut seen = std::collections::BTreeSet::new();
                for item in items {
                    let serde_yaml_ng::Value::String(name) = item else {
                        return Err(crate::config::ConfigError::ManifestSchema {
                            path,
                            reason: "`profile-files` entries must be strings".to_owned(),
                        });
                    };
                    if !is_valid_layer_name(&name) {
                        return Err(crate::config::ConfigError::ManifestSchema {
                            path,
                            reason: format!("invalid profile-file name `{name}`"),
                        });
                    }
                    if !seen.insert(name.clone()) {
                        return Err(crate::config::ConfigError::ManifestSchema {
                            path,
                            reason: format!("duplicate profile-file name `{name}`"),
                        });
                    }
                    names.push(name);
                }
                Some(names)
            }
            Some(_) => {
                return Err(crate::config::ConfigError::ManifestSchema {
                    path,
                    reason: "`profile-files` must be a non-empty array of strings".to_owned(),
                });
            }
        };

        Ok(Self {
            path,
            config_layers,
            profile_files,
        })
    }
}

/// Validate a layer / profile-file stem against the `[a-z0-9._-]+` regex
/// the round-02 contract pins for both `config-layers:` and
/// `profile-files:` entries. ASCII uppercase is intentionally rejected so
/// that the on-disk filename, the YAML reference, and the emitted sibling
/// name agree case-for-case on every platform (some filesystems are
/// case-insensitive, which would silently alias `Deep.config.toml` and
/// `deep.config.toml` and let codex see two profiles where the operator
/// declared one).
pub(crate) fn is_valid_layer_name(name: &str) -> bool {
    !name.is_empty()
        && name.chars().all(|ch| {
            ch.is_ascii_lowercase() || ch.is_ascii_digit() || matches!(ch, '.' | '_' | '-')
        })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::{Manifest, is_valid_layer_name};
    use crate::config::ConfigError;

    /// What `Manifest::parse` made of one manifest. A parse error is one
    /// outcome whatever its wording, because the wording belongs to the YAML
    /// crate and is not pinned here.
    #[derive(Debug, PartialEq, Eq)]
    enum Outcome {
        Parsed {
            layers: Vec<String>,
            profile_files: Option<Vec<String>>,
        },
        Schema(String),
        Parse,
        /// An error of another kind, which no case here expects.
        Other(String),
    }

    fn outcome(yaml: &str) -> Outcome {
        let dir = tempfile::tempdir().unwrap();
        let path = camino::Utf8PathBuf::from_path_buf(dir.path().join("recipe.yaml")).unwrap();
        std::fs::write(&path, yaml).unwrap();
        match Manifest::parse(path) {
            Ok(manifest) => Outcome::Parsed {
                layers: manifest.config_layers,
                profile_files: manifest.profile_files,
            },
            Err(ConfigError::ManifestSchema { reason, .. }) => Outcome::Schema(reason),
            Err(ConfigError::ManifestParse { .. }) => Outcome::Parse,
            Err(other) => Outcome::Other(other.to_string()),
        }
    }

    fn layers(names: &[&str]) -> Outcome {
        Outcome::Parsed {
            layers: names.iter().map(|name| (*name).to_owned()).collect(),
            profile_files: None,
        }
    }

    fn base_with_profile_files(names: &[&str]) -> Outcome {
        Outcome::Parsed {
            layers: vec!["base".to_owned()],
            profile_files: Some(names.iter().map(|name| (*name).to_owned()).collect()),
        }
    }

    fn schema(reason: &str) -> Outcome {
        Outcome::Schema(reason.to_owned())
    }

    /// One entry of `config-layers`.
    fn layer_entry(entry: &str) -> String {
        format!("config-layers:\n  - {entry}\n")
    }

    /// One entry of `profile-files`, beside a valid `config-layers`.
    fn profile_entry(entry: &str) -> String {
        format!("config-layers: [base]\nprofile-files:\n  - {entry}\n")
    }

    /// The whole value of `config-layers`.
    fn layers_value(value: &str) -> String {
        format!("config-layers: {value}\n")
    }

    /// The whole value of `profile-files`, beside a valid `config-layers`.
    fn profile_value(value: &str) -> String {
        format!("config-layers: [base]\nprofile-files: {value}\n")
    }

    const LAYER_NOT_STRING: &str = "`config-layers` entries must be strings";
    const PROFILE_NOT_STRING: &str = "`profile-files` entries must be strings";
    const LAYERS_NOT_LIST: &str = "`config-layers` must be a non-empty array of strings";
    const PROFILES_NOT_LIST: &str = "`profile-files` must be a non-empty array of strings";

    /// Every schema reason keeps its exact text: an operator reads it, and a
    /// test of the CLI matches on it.
    #[test]
    fn each_schema_reason_keeps_its_text() {
        let cases = [
            (layers_value("base"), LAYERS_NOT_LIST),
            (layers_value("[]"), "`config-layers` must not be empty"),
            (layer_entry("[base]"), LAYER_NOT_STRING),
            (
                layers_value("[base, Base]"),
                "invalid config layer name `Base`",
            ),
            (profile_value("base"), PROFILES_NOT_LIST),
            (
                profile_value("[]"),
                "`profile-files` must not be empty when present",
            ),
            (profile_entry("[base]"), PROFILE_NOT_STRING),
            (profile_value("[A]"), "invalid profile-file name `A`"),
            (profile_value("[a, a]"), "duplicate profile-file name `a`"),
        ];
        for (yaml, reason) in cases {
            assert_eq!(outcome(&yaml), schema(reason), "{yaml:?}");
        }
    }

    /// A plain scalar that YAML 1.2 reads as text is a name. One that it
    /// reads as a boolean, a number, or null is not a string. A quoted
    /// scalar is always text.
    #[test]
    fn a_scalar_spelling_keeps_its_result() {
        for name in ["on", "yes", "no"] {
            assert_eq!(outcome(&layer_entry(name)), layers(&[name]), "{name}");
            assert_eq!(
                outcome(&profile_entry(name)),
                base_with_profile_files(&[name]),
                "{name}"
            );
        }
        for spelling in ["true", "12", "0xb", "null", "~"] {
            assert_eq!(
                outcome(&layer_entry(spelling)),
                schema(LAYER_NOT_STRING),
                "{spelling}"
            );
            assert_eq!(
                outcome(&profile_entry(spelling)),
                schema(PROFILE_NOT_STRING),
                "{spelling}"
            );
        }
        for (quoted, name) in [("\"null\"", "null"), ("\"12\"", "12"), ("\"007\"", "007")] {
            assert_eq!(outcome(&layer_entry(quoted)), layers(&[name]), "{quoted}");
            assert_eq!(
                outcome(&profile_entry(quoted)),
                base_with_profile_files(&[name]),
                "{quoted}"
            );
        }
    }

    /// Infinity and not-a-number are floats, so they are not names.
    #[test]
    fn a_non_finite_float_is_not_a_string() {
        for spelling in [".inf", "-.inf", ".nan", ".NaN"] {
            assert_eq!(
                outcome(&layer_entry(spelling)),
                schema(LAYER_NOT_STRING),
                "{spelling}"
            );
            assert_eq!(
                outcome(&profile_entry(spelling)),
                schema(PROFILE_NOT_STRING),
                "{spelling}"
            );
        }
    }

    /// `!!str` leaves a name as written. `!!binary` is not decoded: the
    /// entry keeps its text, and the name rule then judges that text.
    #[test]
    fn a_string_tag_and_a_binary_tag_keep_the_text() {
        assert_eq!(outcome(&layer_entry("!!str base")), layers(&["base"]));
        assert_eq!(
            outcome(&profile_entry("!!str base")),
            base_with_profile_files(&["base"])
        );
        assert_eq!(outcome(&layer_entry("!!binary base")), layers(&["base"]));
        assert_eq!(
            outcome(&profile_entry("!!binary base")),
            base_with_profile_files(&["base"])
        );
        assert_eq!(
            outcome(&layer_entry("!!binary aGVsbG8=")),
            schema("invalid config layer name `aGVsbG8=`")
        );
        assert_eq!(
            outcome(&profile_entry("!!binary aGVsbG8=")),
            schema("invalid profile-file name `aGVsbG8=`")
        );
    }

    /// An alias repeats its anchor. `config-layers` may name a layer twice,
    /// and `profile-files` refuses the repeat as a duplicate.
    #[test]
    fn an_alias_repeats_its_anchor() {
        assert_eq!(
            outcome(&layers_value("[&shared base, work, *shared]")),
            layers(&["base", "work", "base"])
        );
        assert_eq!(
            outcome(&profile_value("[&shared a, *shared]")),
            schema("duplicate profile-file name `a`")
        );
        let many = format!("config-layers:\n  - &a base\n{}", "  - *a\n".repeat(99));
        assert_eq!(outcome(&many), layers(&["base"; 100]));
    }

    /// An absent `profile-files` and a null one both mean no list.
    #[test]
    fn absent_or_null_profile_files_is_no_list() {
        for yaml in [
            "config-layers: [base]\n".to_owned(),
            profile_value("null"),
            profile_value("~"),
        ] {
            assert_eq!(outcome(&yaml), layers(&["base"]), "{yaml:?}");
        }
    }

    /// The root is a closed mapping. A repeated key, an unknown key, and a
    /// merge key are refused, so `<<` never pulls fields in from elsewhere.
    #[test]
    fn the_root_mapping_is_closed() {
        for yaml in [
            "config-layers: [base]\nconfig-layers: [base]\n",
            "config-layers: [base]\nnope: 1\n",
            "<<: {config-layers: [base]}\n",
            "config-layers: [base]\n<<: {profile-files: [a]}\n",
        ] {
            assert_eq!(outcome(yaml), Outcome::Parse, "{yaml:?}");
        }
    }

    /// A merge key inside an entry makes the entry a mapping, which is not a
    /// name.
    #[test]
    fn a_merge_key_in_an_entry_is_not_a_string() {
        assert_eq!(
            outcome(&layer_entry("<<: {a: b}")),
            schema(LAYER_NOT_STRING)
        );
        assert_eq!(
            outcome(&profile_entry("<<: {a: b}")),
            schema(PROFILE_NOT_STRING)
        );
    }

    /// A manifest is one mapping in one document, and it names its layers.
    #[test]
    fn a_document_that_is_not_one_manifest_is_refused() {
        for yaml in [
            "",
            "# only a comment\n",
            "null\n",
            "~\n",
            "config-layers: [base]\n---\nconfig-layers: [base]\n",
            "{}\n",
            "profile-files: [a]\n",
            "- base\n",
        ] {
            assert_eq!(outcome(yaml), Outcome::Parse, "{yaml:?}");
        }
    }

    /// YAML forbids a tab in indentation. A tab as the first indentation
    /// character is refused.
    #[test]
    fn a_tab_as_the_first_indentation_character_is_refused() {
        assert_eq!(outcome("config-layers:\n\t- base\n"), Outcome::Parse);
    }

    // The tests below pin inputs whose result a change of the YAML crate is
    // expected to move. Each states the result of today.

    /// A plain scalar with a leading zero, an underscore, or an exponent too
    /// large for a float is a name as written.
    #[test]
    fn a_number_like_plain_scalar_is_a_name() {
        for spelling in ["007", "01", "00", "1_000", "1e999"] {
            assert_eq!(
                outcome(&layer_entry(spelling)),
                layers(&[spelling]),
                "{spelling}"
            );
            assert_eq!(
                outcome(&profile_entry(spelling)),
                base_with_profile_files(&[spelling]),
                "{spelling}"
            );
        }
    }

    /// An application tag on an entry or a field value makes it a value that
    /// is neither a name nor a list, which the schema refuses.
    #[test]
    fn an_application_tag_is_a_schema_error() {
        for entry in [
            "!foo base",
            "!foo [base]",
            "!foo {a: base}",
            "!foo []",
            "!foo null",
            "!foo ~",
        ] {
            assert_eq!(
                outcome(&layer_entry(entry)),
                schema(LAYER_NOT_STRING),
                "{entry}"
            );
            assert_eq!(
                outcome(&profile_entry(entry)),
                schema(PROFILE_NOT_STRING),
                "{entry}"
            );
        }
        for value in ["!foo [base]", "!foo []", "!foo null", "!foo ~", "!foo base"] {
            assert_eq!(
                outcome(&layers_value(value)),
                schema(LAYERS_NOT_LIST),
                "{value}"
            );
            assert_eq!(
                outcome(&profile_value(value)),
                schema(PROFILES_NOT_LIST),
                "{value}"
            );
        }
        assert_eq!(
            outcome(&layers_value("[&a !foo base, *a]")),
            schema(LAYER_NOT_STRING)
        );
    }

    /// An application tag on the root mapping is ignored.
    #[test]
    fn an_application_tag_on_the_root_is_ignored() {
        assert_eq!(outcome("!foo {config-layers: [base]}\n"), layers(&["base"]));
    }

    /// `!!seq` on a scalar entry and `!!str` on a list value are ignored.
    #[test]
    fn a_mismatched_seq_or_str_tag_is_ignored() {
        assert_eq!(outcome(&layer_entry("!!seq base")), layers(&["base"]));
        assert_eq!(
            outcome(&profile_entry("!!seq base")),
            base_with_profile_files(&["base"])
        );
        assert_eq!(outcome(&layers_value("!!str [base]")), layers(&["base"]));
        assert_eq!(
            outcome(&profile_value("!!str [base]")),
            base_with_profile_files(&["base"])
        );
    }

    /// `!!null` on text that is not null is a parse error, as an entry and as
    /// the value of `config-layers`.
    #[test]
    fn a_null_tag_on_text_is_a_parse_error() {
        assert_eq!(outcome(&layer_entry("!!null base")), Outcome::Parse);
        assert_eq!(outcome(&profile_entry("!!null base")), Outcome::Parse);
        assert_eq!(outcome(&layers_value("!!null base")), Outcome::Parse);
    }

    /// `!!null` on text as the value of `profile-files` is a parse error.
    #[test]
    fn a_null_tag_on_profile_files_is_a_parse_error() {
        assert_eq!(outcome(&profile_value("!!null base")), Outcome::Parse);
    }

    /// One anchor may be repeated by 100 aliases.
    #[test]
    fn one_hundred_aliases_of_one_anchor_parse() {
        let many = format!("config-layers:\n  - &a base\n{}", "  - *a\n".repeat(100));
        assert_eq!(outcome(&many), layers(&["base"; 101]));
    }

    /// A tab after the indentation spaces, a tab after `-`, an unknown
    /// directive, and a `%YAML 1.3` directive are parse errors.
    #[test]
    fn yaml_1_2_spacing_and_directives_are_parse_errors() {
        for yaml in [
            "config-layers:\n  \t- base\n",
            "config-layers:\n  -\tbase\n",
            "%FOO bar\n---\nconfig-layers: [base]\n",
            "%FOO\n---\nconfig-layers: [base]\n",
            "%YAML 1.3\n---\nconfig-layers: [base]\n",
        ] {
            assert_eq!(outcome(yaml), Outcome::Parse, "{yaml:?}");
        }
    }

    #[test]
    fn accepts_lowercase_digits_and_punctuation() {
        assert!(is_valid_layer_name("base"));
        assert!(is_valid_layer_name("base-work"));
        assert!(is_valid_layer_name("v1.2"));
        assert!(is_valid_layer_name("a_b"));
        assert!(is_valid_layer_name("0"));
    }

    #[test]
    fn rejects_empty() {
        assert!(!is_valid_layer_name(""));
    }

    #[test]
    fn rejects_uppercase() {
        // The round-02 contract is `[a-z0-9._-]+`. Uppercase must be
        // rejected so case-insensitive filesystems cannot silently alias
        // `Deep.config.toml` and `deep.config.toml` into two profiles
        // where the operator declared one.
        assert!(!is_valid_layer_name("Deep"));
        assert!(!is_valid_layer_name("DEEP"));
        assert!(!is_valid_layer_name("baseWork"));
    }

    #[test]
    fn rejects_whitespace_and_other_punctuation() {
        assert!(!is_valid_layer_name("base work"));
        assert!(!is_valid_layer_name("base/work"));
        assert!(!is_valid_layer_name("base:work"));
    }
}
