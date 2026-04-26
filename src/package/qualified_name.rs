use std::{
    fmt::{self, Display},
    str::FromStr,
};

use serde::{Deserialize, Serialize};

use crate::package::{
    PackageName, PackageNamespace, ParsePackageNameError, ParsePackageNamespaceError,
};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct PackageQualifiedName {
    namespace: PackageNamespace,
    name: PackageName,
}

impl PackageQualifiedName {
    pub(crate) fn namespace(&self) -> &PackageNamespace {
        &self.namespace
    }

    pub(crate) fn name(&self) -> &PackageName {
        &self.name
    }
}

impl Display for PackageQualifiedName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.namespace, self.name)
    }
}

#[derive(Debug, derive_more::Display, derive_more::Error)]
#[expect(clippy::enum_variant_names)]
pub(crate) enum ParsePackageQualifiedNameError {
    #[display("invalid package qualified name format")]
    InvalidFormat,
    #[display("invalid namespace in package qualified name")]
    InvalidNamespace {
        #[error(source)]
        source: ParsePackageNamespaceError,
    },
    #[display("invalid name in package qualified name")]
    InvalidName {
        #[error(source)]
        source: ParsePackageNameError,
    },
}

impl FromStr for PackageQualifiedName {
    type Err = ParsePackageQualifiedNameError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let Some((namespace, name)) = s.split_once('/') else {
            return Err(ParsePackageQualifiedNameError::InvalidFormat);
        };
        if name.contains('/') || namespace.is_empty() || name.is_empty() {
            return Err(ParsePackageQualifiedNameError::InvalidFormat);
        }

        let namespace = namespace
            .parse()
            .map_err(|source| ParsePackageQualifiedNameError::InvalidNamespace { source })?;
        let name = name
            .parse()
            .map_err(|source| ParsePackageQualifiedNameError::InvalidName { source })?;

        Ok(Self { namespace, name })
    }
}

impl TryFrom<&str> for PackageQualifiedName {
    type Error = ParsePackageQualifiedNameError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl TryFrom<String> for PackageQualifiedName {
    type Error = ParsePackageQualifiedNameError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl Serialize for PackageQualifiedName {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.to_string().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for PackageQualifiedName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Self::from_str(&s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use serde::de::value::{Error as ValueError, StrDeserializer};

    use super::*;

    #[test]
    fn package_qualified_name_parses_valid_string() {
        let qualified_name: PackageQualifiedName = "yuru7/hackgen".parse().unwrap();

        assert_eq!(qualified_name.namespace().to_string(), "yuru7");
        assert_eq!(qualified_name.name().to_string(), "hackgen");
        assert_eq!(qualified_name.to_string(), "yuru7/hackgen");
    }

    #[test]
    fn package_qualified_name_rejects_invalid_format() {
        for input in [
            "yuru7",
            "hackgen",
            "yuru7/",
            "/hackgen",
            "yuru7/hackgen/nerd",
        ] {
            assert!(matches!(
                input.parse::<PackageQualifiedName>(),
                Err(ParsePackageQualifiedNameError::InvalidFormat)
            ));
        }
    }

    #[test]
    fn package_qualified_name_reports_invalid_namespace() {
        let err = "0yuru7/hackgen"
            .parse::<PackageQualifiedName>()
            .unwrap_err();

        assert!(matches!(
            err,
            ParsePackageQualifiedNameError::InvalidNamespace { .. }
        ));
    }

    #[test]
    fn package_qualified_name_reports_invalid_name() {
        let err = "yuru7/0hackgen"
            .parse::<PackageQualifiedName>()
            .unwrap_err();

        assert!(matches!(
            err,
            ParsePackageQualifiedNameError::InvalidName { .. }
        ));
    }

    #[test]
    fn package_qualified_name_deserializes_from_string() {
        let deserializer = StrDeserializer::<ValueError>::new("yuru7/hackgen");
        let qualified_name = PackageQualifiedName::deserialize(deserializer).unwrap();

        assert_eq!(qualified_name.to_string(), "yuru7/hackgen");
    }
}
