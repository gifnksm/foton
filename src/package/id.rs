use std::{
    fmt::{self, Display},
    str::FromStr,
};

use serde::{Deserialize, Serialize};
use snafu::{IntoError as _, ResultExt as _, Snafu};

use crate::package::{
    PackageName, PackageNamespace, PackageQualifiedName, PackageVersion, ParsePackageNameError,
    ParsePackageNamespaceError, ParsePackageQualifiedNameError,
};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct PackageId {
    qualified_name: PackageQualifiedName,
    version: PackageVersion,
}

impl PackageId {
    pub(crate) fn new<N, V>(qualified_name: N, version: V) -> Self
    where
        N: Into<PackageQualifiedName>,
        V: Into<PackageVersion>,
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

    pub(crate) fn version(&self) -> &PackageVersion {
        &self.version
    }
}

impl Display for PackageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}@{}", self.qualified_name, self.version)
    }
}

impl From<&PackageId> for PackageId {
    fn from(pkg_id: &PackageId) -> Self {
        pkg_id.clone()
    }
}

impl TryFrom<String> for PackageId {
    type Error = ParsePackageIdError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl TryFrom<&str> for PackageId {
    type Error = ParsePackageIdError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        value.parse()
    }
}

#[derive(Debug, Snafu)]
#[expect(clippy::enum_variant_names)]
pub(crate) enum ParsePackageIdError {
    #[snafu(display("invalid package ID format"))]
    InvalidFormat,
    #[snafu(display("invalid namespace in package ID"))]
    InvalidNamespace { source: ParsePackageNamespaceError },
    #[snafu(display("invalid name in package ID"))]
    InvalidName { source: ParsePackageNameError },
    #[snafu(display("invalid version in package ID"))]
    InvalidVersion { source: semver::Error },
}

impl FromStr for PackageId {
    type Err = ParsePackageIdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let Some((qualified_name, version)) = s.split_once('@') else {
            return Err(InvalidFormatSnafu.build());
        };
        snafu::ensure!(
            !version.contains('@') && !qualified_name.is_empty() && !version.is_empty(),
            InvalidFormatSnafu
        );

        let qualified_name = qualified_name.parse().map_err(|source| match source {
            ParsePackageQualifiedNameError::InvalidFormat => InvalidFormatSnafu.build(),
            ParsePackageQualifiedNameError::InvalidNamespace { source } => {
                InvalidNamespaceSnafu.into_error(source)
            }
            ParsePackageQualifiedNameError::InvalidName { source } => {
                InvalidNameSnafu.into_error(source)
            }
        })?;
        let version = version.parse().context(InvalidVersionSnafu)?;

        Ok(Self {
            qualified_name,
            version,
        })
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
    use semver::Version;
    use serde::de::value::{Error as ValueError, StrDeserializer};

    use super::*;

    #[test]
    fn package_id_parses_valid_string() {
        let pkg_id = PackageId::from_str("example-namespace/example-font@0.1.0").unwrap();

        assert_eq!(pkg_id.namespace().to_string(), "example-namespace");
        assert_eq!(pkg_id.name().to_string(), "example-font");
        assert_eq!(pkg_id.version(), &Version::new(0, 1, 0));
        assert_eq!(pkg_id.to_string(), "example-namespace/example-font@0.1.0");
    }

    #[test]
    fn package_id_rejects_invalid_format() {
        for input in [
            "example-namespace",
            "example-font@0.1.0",
            "example-namespace/example-font",
            "example-namespace/example-font@",
            "example-namespace/example-font@0.1.0@latest",
        ] {
            assert!(matches!(
                PackageId::from_str(input),
                Err(ParsePackageIdError::InvalidFormat)
            ));
        }
    }

    #[test]
    fn package_id_reports_invalid_namespace() {
        let err = PackageId::from_str("0example-namespace/example-font@0.1.0").unwrap_err();
        assert!(matches!(err, ParsePackageIdError::InvalidNamespace { .. }));
    }

    #[test]
    fn package_id_reports_invalid_name() {
        let err = PackageId::from_str("example-namespace/0example-font@0.1.0").unwrap_err();
        assert!(matches!(err, ParsePackageIdError::InvalidName { .. }));
    }

    #[test]
    fn package_id_reports_invalid_version() {
        let err = PackageId::from_str("example-namespace/example-font@latest").unwrap_err();
        assert!(matches!(err, ParsePackageIdError::InvalidVersion { .. }));
    }

    #[test]
    fn package_id_deserializes_from_string() {
        let deserializer =
            StrDeserializer::<ValueError>::new("example-namespace/example-font@0.1.0");
        let pkg_id = PackageId::deserialize(deserializer).unwrap();

        assert_eq!(pkg_id.to_string(), "example-namespace/example-font@0.1.0");
    }
}
