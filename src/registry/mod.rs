use std::path::PathBuf;

use crate::package::{PackageId, PackageName};

pub(crate) use self::{fetch::*, id::*, index::*, source::*};

mod fetch;
mod id;
mod index;
mod source;

pub(crate) const ROOT_MARKER_FILE_NAME: &str = ".foton-registry-root";
pub(crate) const PACKAGES_DIR_NAME: &str = "packages";
pub(crate) const MANIFEST_FILE_NAME: &str = "manifest.toml";

pub(crate) fn package_root_path_in_registry() -> PathBuf {
    PathBuf::from(PACKAGES_DIR_NAME)
}

pub(crate) fn package_versions_path_in_registry(name: &PackageName) -> PathBuf {
    package_root_path_in_registry().join(name)
}

pub(crate) fn package_dir_path_in_registry(pkg_id: &PackageId) -> PathBuf {
    package_versions_path_in_registry(pkg_id.name()).join(pkg_id.version().to_string())
}

pub(crate) fn package_manifest_path_in_registry(pkg_id: &PackageId) -> PathBuf {
    package_dir_path_in_registry(pkg_id).join(MANIFEST_FILE_NAME)
}

#[derive(Debug)]
pub(crate) struct RegistrySpec {
    id: RegistryId,
    source: RegistrySource,
}

impl RegistrySpec {
    pub(crate) fn new(id: RegistryId, source: RegistrySource) -> Self {
        Self { id, source }
    }

    pub(crate) fn id(&self) -> &RegistryId {
        &self.id
    }
}
