use std::collections::BTreeMap;

use semver::Version;
use serde::{Deserialize, Serialize};

use crate::package::{PackageManifest, PackageQualifiedName, PackageState};

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub(in crate::db) struct PersistedPackageDb {
    pub(in crate::db) packages: BTreeMap<PackageQualifiedName, PersistedPackageVersionMap>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(in crate::db) struct PersistedPackageVersionMap {
    pub(in crate::db) versions: BTreeMap<Version, PersistedPackageEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(in crate::db) struct PersistedPackageEntry {
    pub(in crate::db) state: PackageState,
    pub(in crate::db) manifest: PackageManifest,
}
