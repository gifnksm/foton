use std::fmt::{self, Display};

use semver::Version;

use crate::package::{PackageName, PackageNamespace};

#[derive(Debug, Clone)]
pub(crate) struct PackageId {
    namespace: PackageNamespace,
    name: PackageName,
    version: Version,
}

impl PackageId {
    pub(crate) fn new<NS, N, V>(namespace: NS, name: N, version: V) -> Self
    where
        NS: Into<PackageNamespace>,
        N: Into<PackageName>,
        V: Into<Version>,
    {
        let namespace = namespace.into();
        let name = name.into();
        let version = version.into();
        Self {
            namespace,
            name,
            version,
        }
    }

    pub(crate) fn namespace(&self) -> &PackageNamespace {
        &self.namespace
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
        write!(f, "{}/{}@{}", self.namespace, self.name, self.version)
    }
}
