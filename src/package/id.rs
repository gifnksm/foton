use std::fmt::{self, Display};

use semver::Version;

use crate::package::PackageName;

#[derive(Debug, Clone)]
pub(crate) struct PackageId {
    name: PackageName,
    version: Version,
}

impl PackageId {
    pub(crate) fn new<N, V>(name: N, version: V) -> Self
    where
        N: Into<PackageName>,
        V: Into<Version>,
    {
        let name = name.into();
        let version = version.into();
        Self { name, version }
    }

    pub(crate) fn name(&self) -> &PackageName {
        &self.name
    }

    pub(crate) fn version(&self) -> &Version {
        &self.version
    }
}

impl Display for PackageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}@{}", self.name, self.version)
    }
}
