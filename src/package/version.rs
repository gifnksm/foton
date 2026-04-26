use std::{
    fmt::{self, Display},
    str::FromStr,
    sync::Arc,
};

use semver::Version;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct PackageVersion(Arc<Version>);

impl From<Version> for PackageVersion {
    fn from(version: Version) -> Self {
        Self(version.into())
    }
}

impl FromStr for PackageVersion {
    type Err = semver::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let version = Version::parse(s)?;
        Ok(Self(version.into()))
    }
}

impl Display for PackageVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Display::fmt(&self.0, f)
    }
}

impl PartialEq<Version> for PackageVersion {
    fn eq(&self, other: &Version) -> bool {
        self.0.as_ref() == other
    }
}

impl PartialEq<PackageVersion> for Version {
    fn eq(&self, other: &PackageVersion) -> bool {
        self == other.0.as_ref()
    }
}
