use std::{
    fmt::{self, Display},
    str::FromStr,
};

use serde::{Deserialize, Serialize};
use snafu::{ResultExt as _, Snafu};

use crate::package::{PackageName, PackageVersion, ParsePackageNameError};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct PackageId {
    name: PackageName,
    version: PackageVersion,
}

impl PackageId {
    pub(crate) fn new<N, V>(name: N, version: V) -> Self
    where
        N: Into<PackageName>,
        V: Into<PackageVersion>,
    {
        let name = name.into();
        let version = version.into();
        Self { name, version }
    }

    pub(crate) fn name(&self) -> &PackageName {
        &self.name
    }

    pub(crate) fn version(&self) -> &PackageVersion {
        &self.version
    }
}

impl Display for PackageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}@{}", self.name, self.version)
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
    #[snafu(display("invalid name in package ID"))]
    InvalidName { source: ParsePackageNameError },
    #[snafu(display("invalid version in package ID"))]
    InvalidVersion { source: semver::Error },
}

impl FromStr for PackageId {
    type Err = ParsePackageIdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let Some((name, version)) = s.split_once('@') else {
            return Err(InvalidFormatSnafu.build());
        };
        snafu::ensure!(
            !version.contains('@') && !name.is_empty() && !version.is_empty(),
            InvalidFormatSnafu
        );

        let name = name.parse().context(InvalidNameSnafu)?;
        let version = version.parse().context(InvalidVersionSnafu)?;

        Ok(Self { name, version })
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
        let pkg_id = PackageId::from_str("example-font@0.1.0").unwrap();

        assert_eq!(pkg_id.name().to_string(), "example-font");
        assert_eq!(pkg_id.version(), &Version::new(0, 1, 0));
        assert_eq!(pkg_id.to_string(), "example-font@0.1.0");
    }

    #[test]
    fn package_id_rejects_invalid_format() {
        for input in ["example-font", "example-font@", "example-font@0.1.0@latest"] {
            assert!(matches!(
                PackageId::from_str(input),
                Err(ParsePackageIdError::InvalidFormat)
            ));
        }
    }

    #[test]
    fn package_id_reports_invalid_name() {
        let err = PackageId::from_str("0example-font@0.1.0").unwrap_err();
        assert!(matches!(err, ParsePackageIdError::InvalidName { .. }));
    }

    #[test]
    fn package_id_reports_invalid_version() {
        let err = PackageId::from_str("example-font@latest").unwrap_err();
        assert!(matches!(err, ParsePackageIdError::InvalidVersion { .. }));
    }

    #[test]
    fn package_id_deserializes_from_string() {
        let deserializer = StrDeserializer::<ValueError>::new("example-font@0.1.0");
        let pkg_id = PackageId::deserialize(deserializer).unwrap();

        assert_eq!(pkg_id.to_string(), "example-font@0.1.0");
    }
}
