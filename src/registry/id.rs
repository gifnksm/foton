use std::{
    fmt::{self, Display},
    str::FromStr,
    sync::{Arc, LazyLock},
};

use regex::Regex;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct RegistryId(Arc<str>);

impl RegistryId {
    pub(crate) fn foton() -> Self {
        Self(Arc::from("foton"))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

const REGISTRY_ID_REGEX_STR: &str = r"^[a-zA-Z][-_0-9a-zA-Z]*$";

#[derive(Debug, derive_more::Display, derive_more::Error)]
pub(crate) enum RegistryIdError {
    #[display(
        "invalid registry id `{id}`: must start with an ASCII letter and contain only ASCII letters, digits, `-` or `_`"
    )]
    InvalidFormat { id: String },
}

impl RegistryId {
    pub(crate) fn new<I>(id: I) -> Result<Self, RegistryIdError>
    where
        I: Into<String>,
    {
        static ID_REGEX: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(REGISTRY_ID_REGEX_STR).unwrap());

        let id = id.into();
        if !ID_REGEX.is_match(&id) {
            return Err(RegistryIdError::InvalidFormat { id });
        }
        Ok(Self(id.into()))
    }
}

impl FromStr for RegistryId {
    type Err = RegistryIdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

impl TryFrom<String> for RegistryId {
    type Error = RegistryIdError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl TryFrom<&str> for RegistryId {
    type Error = RegistryIdError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl Display for RegistryId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Display::fmt(&self.0, f)
    }
}

impl AsRef<str> for RegistryId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

macro_rules! impl_partial_eq_for_registry_id {
    ($($ty:ty),* $(,)?) => {
        $(
            impl PartialEq<$ty> for RegistryId {
                fn eq(&self, other: &$ty) -> bool {
                    self.0[..] == other[..]
                }
            }

            impl PartialEq<RegistryId> for $ty {
                fn eq(&self, other: &RegistryId) -> bool {
                    self[..] == other.0[..]
                }
            }
        )*
    };
}

impl_partial_eq_for_registry_id!(String, str, &str);

impl PartialEq<&RegistryId> for RegistryId {
    fn eq(&self, other: &&RegistryId) -> bool {
        self.0 == other.0
    }
}

impl PartialEq<RegistryId> for &RegistryId {
    fn eq(&self, other: &RegistryId) -> bool {
        self.0 == other.0
    }
}

impl Serialize for RegistryId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for RegistryId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Self::new(s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{from_str, to_string};

    use super::*;

    #[test]
    fn registry_id_new_accepts_valid_names() {
        for name_str in [
            "example",
            "Example",
            "example-font",
            "example_font",
            "a0",
            "x",
        ] {
            let name = RegistryId::new(name_str).unwrap();
            assert_eq!(name, name_str);
        }
    }

    #[test]
    fn registry_id_new_rejects_invalid_names() {
        for name in [
            "",
            "0example",
            "-example",
            "_example",
            "example/font",
            r"example\font",
            "example:font",
        ] {
            RegistryId::new(name).unwrap_err();
        }
    }

    #[test]
    fn registry_id_serde_roundtrip_preserves_value() {
        let registry_id = RegistryId::new("local").unwrap();

        let serialized = to_string(&registry_id).unwrap();
        let deserialized: RegistryId = from_str(&serialized).unwrap();

        assert_eq!(serialized, "\"local\"");
        assert_eq!(deserialized, registry_id);
    }

    #[test]
    fn registry_id_deserialize_rejects_invalid_name() {
        let err = from_str::<RegistryId>("\"0invalid\"").unwrap_err();

        assert!(err.to_string().contains("0invalid"));
    }
}
