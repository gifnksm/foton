use std::{
    ffi::OsStr,
    fmt::{self, Display},
    path::Path,
    str::FromStr,
    sync::LazyLock,
};

use regex::Regex;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct PackageNamespace(String);

const PACKAGE_NAMESPACE_REGEX_STR: &str = r"^[a-zA-Z][-_0-9a-zA-Z]*$";

#[derive(Debug, derive_more::Display, derive_more::Error)]
pub(crate) enum ParsePackageNamespaceError {
    #[display(
        "invalid package namespace `{name}`: must start with an ASCII letter and contain only ASCII letters, digits, `-` or `_`"
    )]
    InvalidFormat { name: String },
}

impl PackageNamespace {
    pub(crate) fn new<N>(name: N) -> Result<Self, ParsePackageNamespaceError>
    where
        N: Into<String>,
    {
        static NAMESPACE_REGEX: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(PACKAGE_NAMESPACE_REGEX_STR).unwrap());

        let name = name.into();
        if !NAMESPACE_REGEX.is_match(&name) {
            return Err(ParsePackageNamespaceError::InvalidFormat { name });
        }
        Ok(Self(name))
    }

    #[cfg(test)]
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for PackageNamespace {
    type Err = ParsePackageNamespaceError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

impl Display for PackageNamespace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Display::fmt(&self.0, f)
    }
}

impl AsRef<str> for PackageNamespace {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl AsRef<OsStr> for PackageNamespace {
    fn as_ref(&self) -> &OsStr {
        self.0.as_ref()
    }
}

impl AsRef<Path> for PackageNamespace {
    fn as_ref(&self) -> &Path {
        self.0.as_ref()
    }
}

macro_rules! impl_partial_eq_for_package_namespace {
    ($($ty:ty),* $(,)?) => {
        $(
            impl PartialEq<$ty> for PackageNamespace {
                fn eq(&self, other: &$ty) -> bool {
                    self.0 == *other
                }
            }

            impl PartialEq<PackageNamespace> for $ty {
                fn eq(&self, other: &PackageNamespace) -> bool {
                    *self == other.0
                }
            }
        )*
    };
}

impl_partial_eq_for_package_namespace!(String, str, &str);

impl PartialEq<&PackageNamespace> for PackageNamespace {
    fn eq(&self, other: &&PackageNamespace) -> bool {
        self.0 == other.0
    }
}

impl PartialEq<PackageNamespace> for &PackageNamespace {
    fn eq(&self, other: &PackageNamespace) -> bool {
        self.0 == other.0
    }
}

impl Serialize for PackageNamespace {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for PackageNamespace {
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
    use super::*;

    #[test]
    fn package_namespace_new_accepts_valid_names() {
        for name_str in [
            "hackgen",
            "HackGen",
            "hackgen-nerd",
            "hackgen_nerd",
            "a0",
            "x",
        ] {
            let name = PackageNamespace::new(name_str).unwrap();
            assert_eq!(name, name_str);
        }
    }

    #[test]
    fn package_namespace_new_rejects_invalid_names() {
        for name in [
            "",
            "0hackgen",
            "-hackgen",
            "_hackgen",
            "hackgen/nerd",
            r"hackgen\nerd",
            "hackgen:nerd",
        ] {
            PackageNamespace::new(name).unwrap_err();
        }
    }
}
