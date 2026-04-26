use std::{
    fmt::{self, Display},
    str::FromStr,
};

use semver::Version;
use serde::{Deserialize, Serialize};

use crate::package::{
    PackageName, PackageNamespace, PackageQualifiedName, ParsePackageNameError,
    ParsePackageNamespaceError, ParsePackageQualifiedNameError,
};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct PackageId {
    qualified_name: PackageQualifiedName,
    version: Version,
}

impl PackageId {
    pub(crate) fn new<N, V>(qualified_name: N, version: V) -> Self
    where
        N: Into<PackageQualifiedName>,
        V: Into<Version>,
    {
        let qualified_name = qualified_name.into();
        let version = version.into();
        Self {
            qualified_name,
            version,
        }
    }

    pub(crate) fn qualified_name(&self) -> &PackageQualifiedName {
        &self.qualified_name
    }

    pub(crate) fn namespace(&self) -> &PackageNamespace {
        self.qualified_name.namespace()
    }

    pub(crate) fn name(&self) -> &PackageName {
        self.qualified_name.name()
    }

    pub(crate) fn version(&self) -> &Version {
        &self.version
    }
}

impl Display for PackageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}@{}", self.qualified_name, self.version)
    }
}

#[derive(Debug, derive_more::Display, derive_more::Error)]
#[expect(clippy::enum_variant_names)]
pub(crate) enum ParsePackageIdError {
    #[display("invalid package ID format")]
    InvalidFormat,
    #[display("invalid namespace in package ID")]
    InvalidNamespace {
        #[error(source)]
        source: ParsePackageNamespaceError,
    },
    #[display("invalid name in package ID")]
    InvalidName {
        #[error(source)]
        source: ParsePackageNameError,
    },
    #[display("invalid version in package ID")]
    InvalidVersion {
        #[error(source)]
        source: semver::Error,
    },
}

impl FromStr for PackageId {
    type Err = ParsePackageIdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let Some((qualified_name, version)) = s.split_once('@') else {
            return Err(ParsePackageIdError::InvalidFormat);
        };
        if version.contains('@') || qualified_name.is_empty() || version.is_empty() {
            return Err(ParsePackageIdError::InvalidFormat);
        }

        let qualified_name = qualified_name.parse().map_err(|source| match source {
            ParsePackageQualifiedNameError::InvalidFormat => ParsePackageIdError::InvalidFormat,
            ParsePackageQualifiedNameError::InvalidNamespace { source } => {
                ParsePackageIdError::InvalidNamespace { source }
            }
            ParsePackageQualifiedNameError::InvalidName { source } => {
                ParsePackageIdError::InvalidName { source }
            }
        })?;
        let version = version
            .parse()
            .map_err(|source| ParsePackageIdError::InvalidVersion { source })?;

        Ok(Self {
            qualified_name,
            version,
        })
    }
}

impl TryFrom<&str> for PackageId {
    type Error = ParsePackageIdError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl TryFrom<String> for PackageId {
    type Error = ParsePackageIdError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl Serialize for PackageId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.to_string().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for PackageId {
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
    fn package_id_parses_valid_string() {
        let pkg_id: PackageId = "yuru7/hackgen@2.10.0".parse().unwrap();

        assert_eq!(pkg_id.namespace().to_string(), "yuru7");
        assert_eq!(pkg_id.name().to_string(), "hackgen");
        assert_eq!(pkg_id.version(), &Version::new(2, 10, 0));
        assert_eq!(pkg_id.to_string(), "yuru7/hackgen@2.10.0");
    }

    #[test]
    fn package_id_rejects_invalid_format() {
        for input in [
            "yuru7",
            "hackgen@2.10.0",
            "yuru7/hackgen",
            "yuru7/hackgen@",
            "yuru7/hackgen@2.10.0@latest",
        ] {
            assert!(matches!(
                input.parse::<PackageId>(),
                Err(ParsePackageIdError::InvalidFormat)
            ));
        }
    }

    #[test]
    fn package_id_reports_invalid_namespace() {
        let err = "0yuru7/hackgen@2.10.0".parse::<PackageId>().unwrap_err();

        assert!(matches!(err, ParsePackageIdError::InvalidNamespace { .. }));
    }

    #[test]
    fn package_id_reports_invalid_name() {
        let err = "yuru7/0hackgen@2.10.0".parse::<PackageId>().unwrap_err();

        assert!(matches!(err, ParsePackageIdError::InvalidName { .. }));
    }

    #[test]
    fn package_id_reports_invalid_version() {
        let err = "yuru7/hackgen@latest".parse::<PackageId>().unwrap_err();

        assert!(matches!(err, ParsePackageIdError::InvalidVersion { .. }));
    }

    #[test]
    fn package_id_deserializes_from_string() {
        let deserializer = StrDeserializer::<ValueError>::new("yuru7/hackgen@2.10.0");
        let pkg_id = PackageId::deserialize(deserializer).unwrap();

        assert_eq!(pkg_id.to_string(), "yuru7/hackgen@2.10.0");
    }
}
