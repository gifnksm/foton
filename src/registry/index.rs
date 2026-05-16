use std::{
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
    str::FromStr,
    sync::Arc,
};

use snafu::{IntoError, OptionExt as _, ResultExt as _, Snafu};

use crate::{
    package::{
        PackageId, PackageManifest, PackageManifestError, PackageName, PackageSpec, PackageVersion,
        ParsePackageNameError,
    },
    registry::RegistryId,
};

#[derive(Debug, Snafu)]
pub(crate) enum RegistryIndexError {
    #[snafu(display("failed to read manifest for package {pkg_id}"))]
    ReadManifest {
        pkg_id: PackageId,
        source: PackageManifestError,
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
    #[snafu(display("invalid package version in directory entry: {path}", path = path.display()))]
    InvalidVersionInDirectoryEntry {
        path: PathBuf,
        source: semver::Error,
    },
    #[snafu(display("invalid package name in directory entry: {path}", path = path.display()))]
    InvalidPackageNameInDirectoryEntry {
        path: PathBuf,
        source: ParsePackageNameError,
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

    pub(crate) fn find_latest_package_by_spec(
        &self,
        pkg_spec: &PackageSpec,
    ) -> Result<Option<Arc<PackageManifest>>, RegistryIndexError> {
        match pkg_spec {
            PackageSpec::Name(name) => self.find_latest_package_by_name(name),
            PackageSpec::Id(pkg_id) => self.find_package_by_id(pkg_id),
        }
    }

    pub(crate) fn find_package_by_id(
        &self,
        pkg_id: &PackageId,
    ) -> Result<Option<Arc<PackageManifest>>, RegistryIndexError> {
        let package_dir = self
            .path
            .join(pkg_id.name())
            .join(pkg_id.version().to_string());
        if check_dir_presence(&package_dir)?.is_not_found() {
            return Ok(None);
        }
        let manifest_path = package_dir.join("manifest.toml");
        let manifest =
            PackageManifest::read(&manifest_path).context(ReadManifestSnafu { pkg_id })?;
        let manifest_id = manifest.id();
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

    pub(crate) fn find_latest_package_by_name(
        &self,
        name: &PackageName,
    ) -> Result<Option<Arc<PackageManifest>>, RegistryIndexError> {
        let base_path = self.path.join(name);
        let Some(versions) = read_child_directories::<PackageVersion, _, _>(&base_path, |path| {
            InvalidVersionInDirectoryEntrySnafu { path }
        })?
        else {
            return Ok(None);
        };
        let Some(version) = versions.collect::<Result<Vec<_>, _>>()?.into_iter().max() else {
            return Ok(None);
        };
        let pkg_id = PackageId::new(name.clone(), version.clone());
        self.find_package_by_id(&pkg_id)
    }

    pub(crate) fn all_latest_packages(
        &self,
    ) -> Result<
        Option<impl Iterator<Item = Result<Arc<PackageManifest>, RegistryIndexError>>>,
        RegistryIndexError,
    > {
        let Some(names) = read_child_directories::<PackageName, _, _>(&self.path, |path| {
            InvalidPackageNameInDirectoryEntrySnafu { path }
        })?
        else {
            return Ok(None);
        };
        Ok(Some(names.filter_map(|res| {
            res.and_then(|name| self.find_latest_package_by_name(&name))
                .transpose()
        })))
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

    #[test]
    fn find_package_by_id_reads_manifest() {
        let (registry_dir, registry) = testing::make_registry_index("test-registry");
        testing::write_manifest(registry_dir.path(), "example-font@0.1.0");

        let pkg_id: PackageId = "example-font@0.1.0".parse().unwrap();
        let manifest = registry.find_package_by_id(&pkg_id).unwrap().unwrap();

        assert_eq!(manifest.id(), pkg_id);
    }

    #[test]
    fn find_latest_package_by_name_picks_latest_version() {
        let (registry_dir, registry) = testing::make_registry_index("test-registry");
        testing::write_manifest(registry_dir.path(), "example-font@0.1.0");
        testing::write_manifest(registry_dir.path(), "example-font@0.2.0");

        let name: PackageName = "example-font".parse().unwrap();
        let manifest = registry
            .find_latest_package_by_name(&name)
            .unwrap()
            .unwrap();

        assert_eq!(manifest.id().to_string(), "example-font@0.2.0");
    }

    #[test]
    fn find_package_by_id_rejects_manifest_id_mismatch() {
        let (registry_dir, registry) = testing::make_registry_index("test-registry");
        testing::write_manifest_str(
            registry_dir.path(),
            "example-font@0.1.0",
            &testing::make_manifest_str("example-font@0.1.1"),
        );

        let pkg_id: PackageId = "example-font@0.1.0".parse().unwrap();
        let err = registry.find_package_by_id(&pkg_id).unwrap_err();
        assert!(matches!(err, RegistryIndexError::PackageIdMismatch { .. }));
    }

    #[test]
    fn all_latest_packages_returns_latest_manifest_per_package_name() {
        let (registry_dir, registry) = testing::make_registry_index("test-registry");
        testing::write_manifest(registry_dir.path(), "example-font@0.1.0");
        testing::write_manifest(registry_dir.path(), "example-font@0.2.0");
        testing::write_manifest(registry_dir.path(), "other-font@1.0.0");
        fs::create_dir_all(registry_dir.path().join("empty-font")).unwrap();

        let manifests = registry
            .all_latest_packages()
            .unwrap()
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let mut ids = manifests
            .into_iter()
            .map(|manifest| manifest.id().to_string())
            .collect::<Vec<_>>();
        ids.sort();

        assert_eq!(ids, ["example-font@0.2.0", "other-font@1.0.0"]);
    }
}
