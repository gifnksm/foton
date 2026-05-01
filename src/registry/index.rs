use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
    str::FromStr,
};

use snafu::{IntoError, OptionExt as _, ResultExt as _, Snafu};

use crate::{
    package::{
        PackageId, PackageManifest, PackageName, PackageNamespace, PackageQualifiedName,
        PackageSpec, PackageVersion, ParsePackageNamespaceError,
    },
    registry::RegistryId,
};

#[derive(Debug, Snafu)]
pub(crate) enum RegistryIndexError {
    #[snafu(display("manifest file not found for package {pkg_id}: {manifest_path}", manifest_path = manifest_path.display()))]
    MissingManifest {
        pkg_id: PackageId,
        manifest_path: PathBuf,
    },
    #[snafu(display(
        "failed to read manifest file for package {pkg_id}: {manifest_path}", manifest_path = manifest_path.display()
    ))]
    ReadManifest {
        pkg_id: PackageId,
        manifest_path: PathBuf,
        source: io::Error,
    },
    #[snafu(display(
        "failed to deserialize manifest for package {pkg_id}: {manifest_path}", manifest_path = manifest_path.display()
    ))]
    DeserializeManifest {
        pkg_id: PackageId,
        manifest_path: PathBuf,
        #[snafu(source(from(toml::de::Error, Box::new)))]
        source: Box<toml::de::Error>,
    },
    #[snafu(display(
        "package ID mismatch in manifest for package: expected {expected}, got {got}: {manifest_path}", manifest_path = manifest_path.display()
    ))]
    PackageIdMismatch {
        expected: PackageId,
        got: PackageId,
        manifest_path: PathBuf,
    },
    #[snafu(display("path is not a directory: {path}", path = path.display()))]
    NotADirectory { path: PathBuf },
    #[snafu(display("failed to read directory: {path}", path = path.display()))]
    ReadDir { path: PathBuf, source: io::Error },
    #[snafu(display("failed to read directory entry: {path}", path = path.display()))]
    ReadDirEntry { path: PathBuf, source: io::Error },
    #[snafu(display(
        "directory entry name `{name}` is not valid UTF-8: {path}", name = name.display(), path = path.display(),
    ))]
    NonUtf8DirectoryEntryName { name: OsString, path: PathBuf },
    #[snafu(display("failed to read metadata: {path}", path = path.display()))]
    ReadMetadata { path: PathBuf, source: io::Error },
    #[snafu(display("invalid namespace in directory entry: {path}", path = path.display()))]
    InvalidNamespaceInDirectoryEntry {
        path: PathBuf,
        source: ParsePackageNamespaceError,
    },
    #[snafu(display("invalid version in directory entry: {path}", path = path.display()))]
    InvalidVersionInDirectoryEntry {
        path: PathBuf,
        source: semver::Error,
    },
}

#[derive(Debug)]
pub(crate) struct RegistryIndex {
    id: RegistryId,
    path: PathBuf,
}

impl RegistryIndex {
    pub(crate) fn open(id: RegistryId, path: PathBuf) -> Result<Self, RegistryIndexError> {
        path.metadata()
            .context(ReadMetadataSnafu { path: &path })?
            .is_dir()
            .then_some(())
            .context(NotADirectorySnafu { path: &path })?;
        Ok(Self { id, path })
    }

    pub(crate) fn id(&self) -> &RegistryId {
        &self.id
    }

    pub(crate) fn find_latest_packages_by_spec(
        &self,
        pkg_spec: &PackageSpec,
    ) -> Result<BTreeMap<PackageQualifiedName, PackageManifest>, RegistryIndexError> {
        match pkg_spec {
            PackageSpec::Name(name) => self.find_latest_packages_by_name(name),
            PackageSpec::QualifiedName(qualified_name) => {
                let pkg = self
                    .find_latest_package_by_qualified_name(qualified_name)?
                    .into_iter()
                    .map(|manifest| (qualified_name.clone(), manifest))
                    .collect();
                Ok(pkg)
            }
            PackageSpec::Id(pkg_id) => {
                let pkg = self
                    .find_package_by_id(pkg_id)?
                    .into_iter()
                    .map(|manifest| (pkg_id.qualified_name().clone(), manifest))
                    .collect();
                Ok(pkg)
            }
        }
    }

    pub(crate) fn find_package_by_id(
        &self,
        pkg_id: &PackageId,
    ) -> Result<Option<PackageManifest>, RegistryIndexError> {
        let package_dir = self
            .path
            .join(pkg_id.namespace())
            .join(pkg_id.name())
            .join(pkg_id.version().to_string());
        if check_dir_presence(&package_dir)?.is_not_found() {
            return Ok(None);
        }
        let manifest_path = package_dir.join("manifest.toml");
        let manifest_str = fs::read_to_string(&manifest_path).map_err(|source| {
            if source.kind() == io::ErrorKind::NotFound {
                MissingManifestSnafu {
                    pkg_id,
                    manifest_path: &manifest_path,
                }
                .build()
            } else {
                ReadManifestSnafu {
                    pkg_id,
                    manifest_path: &manifest_path,
                }
                .into_error(source)
            }
        })?;
        let manifest: PackageManifest =
            toml::from_str(&manifest_str).context(DeserializeManifestSnafu {
                pkg_id,
                manifest_path: &manifest_path,
            })?;
        let manifest_id = manifest.metadata.id();
        snafu::ensure!(
            manifest_id == *pkg_id,
            PackageIdMismatchSnafu {
                expected: pkg_id,
                got: manifest_id,
                manifest_path,
            }
        );
        Ok(Some(manifest))
    }

    pub(crate) fn find_latest_package_by_qualified_name(
        &self,
        qualified_name: &PackageQualifiedName,
    ) -> Result<Option<PackageManifest>, RegistryIndexError> {
        let base_path = self
            .path
            .join(qualified_name.namespace())
            .join(qualified_name.name());
        let Some(versions) = read_child_directories::<PackageVersion, _, _>(&base_path, |path| {
            InvalidVersionInDirectoryEntrySnafu { path }
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
    ) -> Result<BTreeMap<PackageQualifiedName, PackageManifest>, RegistryIndexError> {
        let Some(namespaces) =
            read_child_directories::<PackageNamespace, _, _>(&self.path, |path| {
                InvalidNamespaceInDirectoryEntrySnafu { path }
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

fn read_child_directories<T, F, C>(
    path: &Path,
    mut f: F,
) -> Result<Option<impl Iterator<Item = Result<T, RegistryIndexError>>>, RegistryIndexError>
where
    T: FromStr,
    F: FnMut(PathBuf) -> C,
    C: IntoError<RegistryIndexError, Source = T::Err>,
{
    let entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(source) => return Err(ReadDirSnafu { path }.into_error(source)),
    };

    Ok(Some(entries.into_iter().filter_map(move |entry| {
        (|| {
            let entry = entry.context(ReadDirEntrySnafu { path })?;
            let name = entry.file_name();
            let path = entry.path();
            let name = name.to_str().context(NonUtf8DirectoryEntryNameSnafu {
                name: &name,
                path: &path,
            })?;
            if name.starts_with('.') {
                return Ok(None);
            }
            let meta = entry
                .metadata()
                .context(ReadMetadataSnafu { path: &path })?;
            if !meta.is_dir() {
                return Ok(None);
            }
            let value = T::from_str(name).with_context(|_| f(path.clone()))?;
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

fn check_dir_presence(path: &Path) -> Result<DirPresence, RegistryIndexError> {
    let meta = match path.metadata() {
        Ok(meta) => meta,
        Err(source) if source.kind() == io::ErrorKind::NotFound => {
            return Ok(DirPresence::NotFound);
        }
        Err(source) => {
            let path = path.to_path_buf();
            return Err(RegistryIndexError::ReadMetadata { path, source });
        }
    };
    if !meta.is_dir() {
        let path = path.to_path_buf();
        return Err(RegistryIndexError::NotADirectory { path });
    }
    Ok(DirPresence::Exists)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::util::testing;

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

        assert!(matches!(err, RegistryIndexError::PackageIdMismatch { .. }));
    }
}
