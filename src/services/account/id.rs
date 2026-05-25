//! `AccountId` newtype.
//!
//! What this is: validated owner of an account name.
//! What this is not: registry lookup or filesystem layout.

use std::str::FromStr;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct AccountId(String);

impl AccountId {
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    #[allow(dead_code)]
    pub(crate) const fn from_unchecked(value: String) -> Self {
        Self(value)
    }
}

impl std::fmt::Display for AccountId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for AccountId {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        validate(value)?;
        Ok(Self(value.to_owned()))
    }
}

impl serde::Serialize for AccountId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> serde::Deserialize<'de> for AccountId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse::<Self>().map_err(serde::de::Error::custom)
    }
}

fn validate(value: &str) -> Result<(), String> {
    if value.is_empty() {
        return Err("must not be empty".to_owned());
    }
    if value.len() > 32 {
        return Err("must be 32 bytes or fewer".to_owned());
    }

    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return Err("must not be empty".to_owned());
    };
    if !first.is_ascii_lowercase() && !first.is_ascii_digit() {
        return Err("must start with a lowercase ASCII letter or digit".to_owned());
    }
    if let Some(invalid) = chars
        .find(|ch| !ch.is_ascii_lowercase() && !ch.is_ascii_digit() && *ch != '_' && *ch != '-')
    {
        return Err(format!("contains invalid character `{invalid}`"));
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::AccountId;
    use std::str::FromStr;

    #[test]
    fn accepts_valid_values() {
        for value in ["default", "a", "a-b_c-1", "0", "z9_"] {
            assert_eq!(AccountId::from_str(value).unwrap().as_str(), value);
        }
    }

    #[test]
    fn rejects_invalid_values() {
        for value in [
            "",
            "-foo",
            "_foo",
            "FOO",
            "foo/bar",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ] {
            assert!(AccountId::from_str(value).is_err(), "{value}");
        }
    }
}
