use std::{
    fmt::{self, Display},
    str::FromStr,
    sync::Arc,
};

use semver::Version;
use serde::{Deserialize, Serialize};
use snafu::Snafu;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct PackageVersion(Arc<Version>);

#[derive(Debug, Snafu)]
pub(crate) enum ParsePackageVersionError {
    #[snafu(transparent)]
    SemVer { source: semver::Error },
}

impl FromStr for PackageVersion {
    type Err = ParsePackageVersionError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let version = Version::parse(s)?;
        Ok(Self(version.into()))
    }
}

impl TryFrom<String> for PackageVersion {
    type Error = ParsePackageVersionError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::from_str(&value)
    }
}

impl TryFrom<&str> for PackageVersion {
    type Error = ParsePackageVersionError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::from_str(value)
    }
}

impl Display for PackageVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Display::fmt(&self.0, f)
    }
}

impl From<&PackageVersion> for PackageVersion {
    fn from(version: &PackageVersion) -> Self {
        version.clone()
    }
}
