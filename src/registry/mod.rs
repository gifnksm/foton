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

    use tempfile::TempDir;

    use super::*;

    fn make_registry() -> (TempDir, PackageRegistry) {
        let tempdir = TempDir::new().unwrap();
        let registry = PackageRegistry::new(tempdir.path().to_path_buf());
        (tempdir, registry)
    }

    fn write_manifest(
        root: &Path,
        namespace: &str,
        name: &str,
        version: &str,
        manifest_name: &str,
        manifest_version: &str,
    ) {
        let dir = root.join(namespace).join(name).join(version);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("manifest.toml"),
            format!(
                r#"
[package]
name = "{manifest_name}"
version = "{manifest_version}"

[[sources]]
url = "https://example.com/{name}-{version}.zip"
hash = "sha256:ed182e2a4b95792d94dea7932f6b45280b5ae353651be249d5f6b7867b788db7"
"#
            ),
        )
        .unwrap();
    }

    #[test]
    fn find_package_by_id_reads_manifest() {
        let (tempdir, registry) = make_registry();
        write_manifest(
            tempdir.path(),
            "yuru7",
            "hackgen",
            "2.10.0",
            "yuru7/hackgen",
            "2.10.0",
        );

        let pkg_id: PackageId = "yuru7/hackgen@2.10.0".parse().unwrap();
        let manifest = registry.find_package_by_id(&pkg_id).unwrap().unwrap();

        assert_eq!(manifest.metadata.id(), pkg_id);
    }

    #[test]
    fn find_latest_package_by_qualified_name_picks_latest_version() {
        let (tempdir, registry) = make_registry();
        write_manifest(
            tempdir.path(),
            "yuru7",
            "hackgen",
            "2.9.0",
            "yuru7/hackgen",
            "2.9.0",
        );
        write_manifest(
            tempdir.path(),
            "yuru7",
            "hackgen",
            "2.10.0",
            "yuru7/hackgen",
            "2.10.0",
        );

        let qualified_name: PackageQualifiedName = "yuru7/hackgen".parse().unwrap();
        let manifest = registry
            .find_latest_package_by_qualified_name(&qualified_name)
            .unwrap()
            .unwrap();

        assert_eq!(manifest.metadata.id().to_string(), "yuru7/hackgen@2.10.0");
    }

    #[test]
    fn find_latest_packages_by_name_returns_latest_manifest_per_package() {
        let (tempdir, registry) = make_registry();
        write_manifest(
            tempdir.path(),
            "yuru7",
            "hackgen",
            "2.9.0",
            "yuru7/hackgen",
            "2.9.0",
        );
        write_manifest(
            tempdir.path(),
            "yuru7",
            "hackgen",
            "2.10.0",
            "yuru7/hackgen",
            "2.10.0",
        );
        write_manifest(
            tempdir.path(),
            "someone",
            "hackgen",
            "1.0.0",
            "someone/hackgen",
            "1.0.0",
        );

        let name: PackageName = "hackgen".parse().unwrap();
        let manifests = registry.find_latest_packages_by_name(&name).unwrap();

        assert_eq!(manifests.len(), 2);
        assert_eq!(
            manifests[&"someone/hackgen".parse().unwrap()]
                .metadata
                .id()
                .to_string(),
            "someone/hackgen@1.0.0"
        );
        assert_eq!(
            manifests[&"yuru7/hackgen".parse().unwrap()]
                .metadata
                .id()
                .to_string(),
            "yuru7/hackgen@2.10.0"
        );
    }

    #[test]
    fn find_package_by_id_rejects_manifest_id_mismatch() {
        let (tempdir, registry) = make_registry();
        write_manifest(
            tempdir.path(),
            "yuru7",
            "hackgen",
            "2.10.0",
            "yuru7/hackgen",
            "2.9.0",
        );

        let pkg_id: PackageId = "yuru7/hackgen@2.10.0".parse().unwrap();
        let err = registry.find_package_by_id(&pkg_id).unwrap_err();

        assert!(matches!(
            err,
            PackageRegistryError::PackageIdMismatch { .. }
        ));
    }
}
