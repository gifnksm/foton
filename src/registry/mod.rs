use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
    str::FromStr,
};

use crate::package::{
    PackageId, PackageManifest, PackageName, PackageNamespace, PackageQualifiedName,
    PackageVersion, ParsePackageNamespaceError,
};

#[derive(Debug, derive_more::Display, derive_more::Error)]
pub(crate) enum PackageRegistryError {
    #[display("manifest file not found for package {pkg_id}: {path}", path = path.display())]
    MissingManifest { pkg_id: PackageId, path: PathBuf },
    #[display("failed to read manifest file for package {pkg_id}: {path}", path = path.display())]
    ReadManifest {
        pkg_id: PackageId,
        path: PathBuf,
        #[error(source)]
        source: io::Error,
    },
    #[display("failed to deserialize manifest for package {pkg_id}: {path}", path = path.display())]
    DeserializeManifest {
        pkg_id: PackageId,
        path: PathBuf,
        #[error(source)]
        source: Box<toml::de::Error>,
    },
    #[display("package ID mismatch in manifest for package: expected {expected}, got {got}: {path}", path = path.display())]
    PackageIdMismatch {
        expected: PackageId,
        got: PackageId,
        path: PathBuf,
    },
    #[display("path is not a directory: {path}", path = path.display())]
    NotADirectory { path: PathBuf },
    #[display("failed to read directory: {path}", path = path.display())]
    ReadDir {
        path: PathBuf,
        #[error(source)]
        source: io::Error,
    },
    #[display("failed to read directory entry: {path}", path = path.display())]
    ReadDirEntry {
        path: PathBuf,
        #[error(source)]
        source: io::Error,
    },
    #[display("directory entry name `{name}` is not valid UTF-8: {path}", name = name.display(), path = path.display())]
    NonUtf8DirectoryEntryName { name: OsString, path: PathBuf },
    #[display("failed to read metadata: {path}", path = path.display())]
    ReadMetadata {
        path: PathBuf,
        #[error(source)]
        source: io::Error,
    },
    #[display("invalid namespace in directory entry: {path}", path = path.display())]
    InvalidNamespaceInDirectoryEntry {
        path: PathBuf,
        #[error(source)]
        source: ParsePackageNamespaceError,
    },
    #[display("invalid version in directory entry: {path}", path = path.display())]
    InvalidVersionInDirectoryEntry {
        path: PathBuf,
        #[error(source)]
        source: semver::Error,
    },
}

#[derive(Debug)]
pub(crate) struct PackageRegistry {
    path: PathBuf,
}

impl PackageRegistry {
    pub(crate) fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub(crate) fn find_package_by_id(
        &self,
        pkg_id: &PackageId,
    ) -> Result<Option<PackageManifest>, PackageRegistryError> {
        let package_dir = self
            .path
            .join(pkg_id.namespace())
            .join(pkg_id.name())
            .join(pkg_id.version().to_string());
        if check_dir_presence(&package_dir)?.is_not_found() {
            return Ok(None);
        }
        let manifest_path = package_dir.join("manifest.toml");
        let manifest_str = match fs::read_to_string(&manifest_path) {
            Ok(s) => s,
            Err(source) if source.kind() == io::ErrorKind::NotFound => {
                let pkg_id = pkg_id.clone();
                let path = manifest_path.clone();
                return Err(PackageRegistryError::MissingManifest { pkg_id, path });
            }
            Err(source) => {
                let pkg_id = pkg_id.clone();
                let path = manifest_path.clone();
                return Err(PackageRegistryError::ReadManifest {
                    pkg_id,
                    path,
                    source,
                });
            }
        };
        let manifest: PackageManifest = toml::from_str(&manifest_str).map_err(|source| {
            let pkg_id = pkg_id.clone();
            let path = manifest_path.clone();
            let source = source.into();
            PackageRegistryError::DeserializeManifest {
                pkg_id,
                path,
                source,
            }
        })?;
        let manifest_id = manifest.metadata.id();
        if manifest_id != *pkg_id {
            let expected = pkg_id.clone();
            let got = manifest_id;
            let path = manifest_path.clone();
            return Err(PackageRegistryError::PackageIdMismatch {
                expected,
                got,
                path,
            });
        }
        Ok(Some(manifest))
    }

    pub(crate) fn find_latest_package_by_qualified_name(
        &self,
        qualified_name: &PackageQualifiedName,
    ) -> Result<Option<PackageManifest>, PackageRegistryError> {
        let base_path = self
            .path
            .join(qualified_name.namespace())
            .join(qualified_name.name());
        let Some(versions) =
            read_child_directories::<PackageVersion, _>(&base_path, |path, source| {
                PackageRegistryError::InvalidVersionInDirectoryEntry { path, source }
            })?
        else {
            return Ok(None);
        };
        let Some(version) = versions.collect::<Result<Vec<_>, _>>()?.into_iter().max() else {
            return Ok(None);
        };
        let pkg_id = PackageId::new(qualified_name.clone(), version.clone());
        self.find_package_by_id(&pkg_id)
    }

    pub(crate) fn find_latest_packages_by_name(
        &self,
        name: &PackageName,
    ) -> Result<BTreeMap<PackageQualifiedName, PackageManifest>, PackageRegistryError> {
        let Some(namespaces) =
            read_child_directories::<PackageNamespace, _>(&self.path, |path, source| {
                PackageRegistryError::InvalidNamespaceInDirectoryEntry { path, source }
            })?
        else {
            return Ok(BTreeMap::new());
        };
        namespaces
            .filter_map(|namespace| {
                (|| {
                    let namespace = namespace?;
                    let versions_dir = self.path.join(&namespace).join(name);
                    if check_dir_presence(&versions_dir)?.is_not_found() {
                        return Ok(None);
                    }
                    let qualified_name = PackageQualifiedName::new(namespace.clone(), name.clone());
                    let manifest = self.find_latest_package_by_qualified_name(&qualified_name)?;
                    Ok(manifest.map(|manifest| (qualified_name, manifest)))
                })()
                .transpose()
            })
            .collect()
    }
}

fn read_child_directories<T, F>(
    path: &Path,
    mut f: F,
) -> Result<Option<impl Iterator<Item = Result<T, PackageRegistryError>>>, PackageRegistryError>
where
    T: FromStr,
    F: FnMut(PathBuf, T::Err) -> PackageRegistryError,
{
    let entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            let path = path.to_path_buf();
            return Err(PackageRegistryError::ReadDir { path, source });
        }
    };

    Ok(Some(entries.into_iter().filter_map(move |entry| {
        (|| {
            let entry = entry.map_err(|source| {
                let path = path.to_path_buf();
                PackageRegistryError::ReadDirEntry { path, source }
            })?;
            let name = entry.file_name();
            let name = name.to_str().ok_or_else(|| {
                let name = name.clone();
                let path = entry.path();
                PackageRegistryError::NonUtf8DirectoryEntryName { name, path }
            })?;
            if name.starts_with('.') {
                return Ok(None);
            }
            let meta = entry.metadata().map_err(|source| {
                let path = entry.path();
                PackageRegistryError::ReadMetadata { path, source }
            })?;
            if !meta.is_dir() {
                return Ok(None);
            }
            let value = T::from_str(name).map_err(|source| {
                let path = entry.path();
                f(path, source)
            })?;
            Ok(Some(value))
        })()
        .transpose()
    })))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, derive_more::IsVariant)]
enum DirPresence {
    Exists,
    NotFound,
}

fn check_dir_presence(path: &Path) -> Result<DirPresence, PackageRegistryError> {
    let meta = match path.metadata() {
        Ok(meta) => meta,
        Err(source) if source.kind() == io::ErrorKind::NotFound => {
            return Ok(DirPresence::NotFound);
        }
        Err(source) => {
            let path = path.to_path_buf();
            return Err(PackageRegistryError::ReadMetadata { path, source });
        }
    };
    if !meta.is_dir() {
        let path = path.to_path_buf();
        return Err(PackageRegistryError::NotADirectory { path });
    }
    Ok(DirPresence::Exists)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use crate::util::testing;

    use super::*;

    fn write_manifest(root: &Path, namespace: &str, name: &str, version: &str) {
        write_manifest_str(
            root,
            namespace,
            name,
            version,
            &testing::make_manifest_str(namespace, name, version),
        );
    }

    fn write_manifest_str(
        root: &Path,
        namespace: &str,
        name: &str,
        version: &str,
        manifest_str: &str,
    ) {
        let dir = root.join(namespace).join(name).join(version);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("manifest.toml"), manifest_str).unwrap();
    }

    #[test]
    fn find_package_by_id_reads_manifest() {
        let (tempdir, registry) = testing::make_registry();
        write_manifest(tempdir.path(), "example-namespace", "example-font", "0.1.0");

        let pkg_id: PackageId = "example-namespace/example-font@0.1.0".parse().unwrap();
        let manifest = registry.find_package_by_id(&pkg_id).unwrap().unwrap();

        assert_eq!(manifest.metadata.id(), pkg_id);
    }

    #[test]
    fn find_latest_package_by_qualified_name_picks_latest_version() {
        let (tempdir, registry) = testing::make_registry();
        write_manifest(tempdir.path(), "example-namespace", "example-font", "0.1.0");
        write_manifest(tempdir.path(), "example-namespace", "example-font", "0.2.0");

        let qualified_name: PackageQualifiedName =
            "example-namespace/example-font".parse().unwrap();
        let manifest = registry
            .find_latest_package_by_qualified_name(&qualified_name)
            .unwrap()
            .unwrap();

        assert_eq!(
            manifest.metadata.id().to_string(),
            "example-namespace/example-font@0.2.0"
        );
    }

    #[test]
    fn find_latest_packages_by_name_returns_latest_manifest_per_package() {
        let (tempdir, registry) = testing::make_registry();
        write_manifest(tempdir.path(), "example-namespace", "example-font", "0.1.0");
        write_manifest(tempdir.path(), "example-namespace", "example-font", "0.2.0");
        write_manifest(tempdir.path(), "other-namespace", "example-font", "1.0.0");

        let name: PackageName = "example-font".parse().unwrap();
        let manifests = registry.find_latest_packages_by_name(&name).unwrap();

        assert_eq!(manifests.len(), 2);
        assert_eq!(
            manifests[&"other-namespace/example-font".parse().unwrap()]
                .metadata
                .id()
                .to_string(),
            "other-namespace/example-font@1.0.0"
        );
        assert_eq!(
            manifests[&"example-namespace/example-font".parse().unwrap()]
                .metadata
                .id()
                .to_string(),
            "example-namespace/example-font@0.2.0"
        );
    }

    #[test]
    fn find_package_by_id_rejects_manifest_id_mismatch() {
        let (tempdir, registry) = testing::make_registry();
        write_manifest_str(
            tempdir.path(),
            "example-namespace",
            "example-font",
            "0.1.0",
            &testing::make_manifest_str("example-namespace", "example-font", "0.1.1"),
        );

        let pkg_id: PackageId = "example-namespace/example-font@0.1.0".parse().unwrap();
        let err = registry.find_package_by_id(&pkg_id).unwrap_err();

        assert!(matches!(
            err,
            PackageRegistryError::PackageIdMismatch { .. }
        ));
    }
}
