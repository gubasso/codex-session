//! Config-recipe manifest parsing.
//!
//! What this is: YAML parsing and schema validation for `config-recipes/*.yaml`.
//! What this is not: TOML layer parsing or merge logic.

#![allow(clippy::result_large_err)]

use std::fmt;

use camino::Utf8PathBuf;
use serde::{
    Deserialize, Deserializer,
    de::{self, IgnoredAny, MapAccess, SeqAccess, Visitor},
};

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
    config_layers: Listed,
    #[serde(rename = "profile-files", default)]
    profile_files: Option<Listed>,
}

/// A field value as the manifest wrote it: a list, or any other value.
///
/// The YAML crate has no untyped value, so the shape is read here and judged
/// by `Manifest::parse`, which owns the schema reasons.
#[derive(Debug)]
enum Listed {
    Sequence(Vec<Entry>),
    NotASequence,
}

/// One list entry as the manifest wrote it: text, or any other value.
#[derive(Debug)]
enum Entry {
    Text(String),
    NotAString,
}

impl<'de> Deserialize<'de> for Listed {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(ListedVisitor)
    }
}

struct ListedVisitor;

/// Every value that is not a sequence becomes `NotASequence`, so a wrong
/// shape reaches the schema reason instead of a parse error.
impl<'de> Visitor<'de> for ListedVisitor {
    type Value = Listed;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("any YAML value")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Listed, A::Error> {
        let mut entries = Vec::new();
        while let Some(entry) = seq.next_element::<Entry>()? {
            entries.push(entry);
        }
        Ok(Listed::Sequence(entries))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Listed, A::Error> {
        while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
        Ok(Listed::NotASequence)
    }

    fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<Listed, D::Error> {
        Listed::deserialize(deserializer)
    }

    fn visit_newtype_struct<D: Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> Result<Listed, D::Error> {
        Listed::deserialize(deserializer)
    }

    fn visit_bool<E: de::Error>(self, _: bool) -> Result<Listed, E> {
        Ok(Listed::NotASequence)
    }

    fn visit_i64<E: de::Error>(self, _: i64) -> Result<Listed, E> {
        Ok(Listed::NotASequence)
    }

    fn visit_i128<E: de::Error>(self, _: i128) -> Result<Listed, E> {
        Ok(Listed::NotASequence)
    }

    fn visit_u64<E: de::Error>(self, _: u64) -> Result<Listed, E> {
        Ok(Listed::NotASequence)
    }

    fn visit_u128<E: de::Error>(self, _: u128) -> Result<Listed, E> {
        Ok(Listed::NotASequence)
    }

    fn visit_f64<E: de::Error>(self, _: f64) -> Result<Listed, E> {
        Ok(Listed::NotASequence)
    }

    fn visit_str<E: de::Error>(self, _: &str) -> Result<Listed, E> {
        Ok(Listed::NotASequence)
    }

    fn visit_bytes<E: de::Error>(self, _: &[u8]) -> Result<Listed, E> {
        Ok(Listed::NotASequence)
    }

    fn visit_none<E: de::Error>(self) -> Result<Listed, E> {
        Ok(Listed::NotASequence)
    }

    fn visit_unit<E: de::Error>(self) -> Result<Listed, E> {
        Ok(Listed::NotASequence)
    }
}

impl<'de> Deserialize<'de> for Entry {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(EntryVisitor)
    }
}

struct EntryVisitor;

/// Text becomes `Text`. Every other value, a float included, becomes
/// `NotAString`, and a nested list or mapping is consumed whole.
impl<'de> Visitor<'de> for EntryVisitor {
    type Value = Entry;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("any YAML value")
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<Entry, E> {
        Ok(Entry::Text(value.to_owned()))
    }

    fn visit_borrowed_str<E: de::Error>(self, value: &'de str) -> Result<Entry, E> {
        Ok(Entry::Text(value.to_owned()))
    }

    fn visit_string<E: de::Error>(self, value: String) -> Result<Entry, E> {
        Ok(Entry::Text(value))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Entry, A::Error> {
        while seq.next_element::<IgnoredAny>()?.is_some() {}
        Ok(Entry::NotAString)
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Entry, A::Error> {
        while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
        Ok(Entry::NotAString)
    }

    fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<Entry, D::Error> {
        Entry::deserialize(deserializer)
    }

    fn visit_newtype_struct<D: Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> Result<Entry, D::Error> {
        Entry::deserialize(deserializer)
    }

    fn visit_bool<E: de::Error>(self, _: bool) -> Result<Entry, E> {
        Ok(Entry::NotAString)
    }

    fn visit_i64<E: de::Error>(self, _: i64) -> Result<Entry, E> {
        Ok(Entry::NotAString)
    }

    fn visit_i128<E: de::Error>(self, _: i128) -> Result<Entry, E> {
        Ok(Entry::NotAString)
    }

    fn visit_u64<E: de::Error>(self, _: u64) -> Result<Entry, E> {
        Ok(Entry::NotAString)
    }

    fn visit_u128<E: de::Error>(self, _: u128) -> Result<Entry, E> {
        Ok(Entry::NotAString)
    }

    fn visit_f64<E: de::Error>(self, _: f64) -> Result<Entry, E> {
        Ok(Entry::NotAString)
    }

    fn visit_bytes<E: de::Error>(self, _: &[u8]) -> Result<Entry, E> {
        Ok(Entry::NotAString)
    }

    fn visit_none<E: de::Error>(self) -> Result<Entry, E> {
        Ok(Entry::NotAString)
    }

    fn visit_unit<E: de::Error>(self) -> Result<Entry, E> {
        Ok(Entry::NotAString)
    }
}

/// The parser options of every manifest parse, each set by name so that a
/// changed default of the YAML crate cannot move a result unnoticed.
fn yaml_options() -> serde_saphyr::Options {
    serde_saphyr::options! {
        // Only `true` and `false` are booleans; `yes` and `on` stay text.
        strict_booleans: true,
        duplicate_keys: serde_saphyr::DuplicateKeyPolicy::Error,
        // `<<` is an ordinary key, which the closed root mapping refuses.
        merge_keys: serde_saphyr::MergeKeyPolicy::AsOrdinary,
        // A `!!binary` entry keeps its text rather than being decoded.
        ignore_binary_tag_for_string: true,
        // An application tag such as `!foo` is refused, as it always was.
        reject_unsupported_tags: true,
        legacy_octal_numbers: false,
        // `.inf` and `.nan` reach the visitors as floats, which are not names.
        non_finite_float_policy: serde_saphyr::NonFiniteFloatPolicy::PassThrough,
        budget: Some(serde_saphyr::Budget::default()),
        // The `caused by:` line holds one message. A source excerpt would add
        // lines, name its file `<input>` beside the `where:` path, and repeat
        // the message.
        with_snippet: false,
    }
}

impl Manifest {
    pub(crate) fn parse(path: Utf8PathBuf) -> Result<Self, crate::config::ConfigError> {
        let contents = std::fs::read_to_string(path.as_std_path())?;
        let raw: RawManifest = serde_saphyr::from_str_with_options(&contents, yaml_options())
            .map_err(|error| crate::config::ConfigError::ManifestParse {
                path: path.clone(),
                source: crate::config::ManifestYamlError::new(&error),
            })?;

        let Listed::Sequence(layers) = raw.config_layers else {
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
            let Entry::Text(name) = item else {
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
            Some(Listed::Sequence(items)) => {
                if items.is_empty() {
                    return Err(crate::config::ConfigError::ManifestSchema {
                        path,
                        reason: "`profile-files` must not be empty when present".to_owned(),
                    });
                }
                let mut names = Vec::with_capacity(items.len());
                let mut seen = std::collections::BTreeSet::new();
                for item in items {
                    let Entry::Text(name) = item else {
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
            Some(Listed::NotASequence) => {
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

    // The tests below pin the inputs that read differently since the YAML
    // crate became serde-saphyr. Each states the result before the change.

    /// A plain scalar with a leading zero, an underscore, or an exponent too
    /// large for a float is a number in YAML 1.2, so it is not a name. A
    /// quoted spelling is. Before serde-saphyr each was a name as written.
    #[test]
    fn a_number_like_plain_scalar_is_not_a_string() {
        for spelling in ["007", "01", "00", "1_000", "1e999"] {
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
        assert_eq!(outcome(&layer_entry("\"007\"")), layers(&["007"]));
    }

    /// An application tag is an unsupported tag, refused by the parser on an
    /// entry, on a field value, and through an alias. Before serde-saphyr the
    /// schema refused the tagged value instead. Both exit with code 78.
    #[test]
    fn an_application_tag_is_a_parse_error() {
        for entry in [
            "!foo base",
            "!foo [base]",
            "!foo {a: base}",
            "!foo []",
            "!foo null",
            "!foo ~",
        ] {
            assert_eq!(outcome(&layer_entry(entry)), Outcome::Parse, "{entry}");
            assert_eq!(outcome(&profile_entry(entry)), Outcome::Parse, "{entry}");
        }
        for value in ["!foo [base]", "!foo []", "!foo null", "!foo ~", "!foo base"] {
            assert_eq!(outcome(&layers_value(value)), Outcome::Parse, "{value}");
            assert_eq!(outcome(&profile_value(value)), Outcome::Parse, "{value}");
        }
        assert_eq!(outcome(&layers_value("[&a !foo base, *a]")), Outcome::Parse);
    }

    /// An application tag on the root mapping is refused too. Before
    /// serde-saphyr it was ignored.
    #[test]
    fn an_application_tag_on_the_root_is_a_parse_error() {
        assert_eq!(outcome("!foo {config-layers: [base]}\n"), Outcome::Parse);
    }

    /// `!!seq` on a scalar entry and `!!str` on a list value name the wrong
    /// node kind, which the parser refuses. Before serde-saphyr the tag was
    /// ignored.
    #[test]
    fn a_mismatched_seq_or_str_tag_is_a_parse_error() {
        assert_eq!(outcome(&layer_entry("!!seq base")), Outcome::Parse);
        assert_eq!(outcome(&profile_entry("!!seq base")), Outcome::Parse);
        assert_eq!(outcome(&layers_value("!!str [base]")), Outcome::Parse);
        assert_eq!(outcome(&profile_value("!!str [base]")), Outcome::Parse);
    }

    /// `!!null` makes a value null whatever its text, so the schema judges a
    /// null: not a string as an entry, not a list as `config-layers`. Before
    /// serde-saphyr each was a parse error.
    #[test]
    fn a_null_tag_on_text_reads_as_null() {
        assert_eq!(
            outcome(&layer_entry("!!null base")),
            schema(LAYER_NOT_STRING)
        );
        assert_eq!(
            outcome(&profile_entry("!!null base")),
            schema(PROFILE_NOT_STRING)
        );
        assert_eq!(
            outcome(&layers_value("!!null base")),
            schema(LAYERS_NOT_LIST)
        );
    }

    /// A null `profile-files` is no list, whatever its text. Before
    /// serde-saphyr `profile-files: !!null base` was a parse error.
    #[test]
    fn a_null_tag_on_profile_files_is_no_list() {
        assert_eq!(outcome(&profile_value("!!null base")), layers(&["base"]));
    }

    /// The default parse budget refuses one anchor repeated by 100 aliases;
    /// 99 pass. Before serde-saphyr 100 aliases parsed in `config-layers`,
    /// and in `profile-files` the repeat was refused as a duplicate name.
    #[test]
    fn one_hundred_aliases_of_one_anchor_are_a_parse_error() {
        let many = format!("config-layers:\n  - &a base\n{}", "  - *a\n".repeat(100));
        assert_eq!(outcome(&many), Outcome::Parse);
        let files = format!(
            "config-layers: [base]\nprofile-files:\n  - &a a\n{}",
            "  - *a\n".repeat(100)
        );
        assert_eq!(outcome(&files), Outcome::Parse);
    }

    /// The parser reads the YAML 1.2 grammar: a tab after the indentation
    /// spaces, a tab after `-`, an unknown directive, and `%YAML 1.3` are
    /// accepted. Before serde-saphyr each was a parse error.
    #[test]
    fn yaml_1_2_spacing_and_directives_are_accepted() {
        for yaml in [
            "config-layers:\n  \t- base\n",
            "config-layers:\n  -\tbase\n",
            "%FOO bar\n---\nconfig-layers: [base]\n",
            "%FOO\n---\nconfig-layers: [base]\n",
            "%YAML 1.3\n---\nconfig-layers: [base]\n",
        ] {
            assert_eq!(outcome(yaml), layers(&["base"]), "{yaml:?}");
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
