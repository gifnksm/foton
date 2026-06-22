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
        PackageDefinition, PackageId, PackageManifest, PackageManifestError, PackageName,
        PackageSpec, PackageVersion, ParsePackageNameError, ParsePackageVersionError,
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
        source: ParsePackageVersionError,
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
    root_path: PathBuf,
}

impl RegistryIndex {
    pub(crate) fn open(id: RegistryId, root_path: PathBuf) -> Result<Self, RegistryIndexError> {
        root_path
            .metadata()
            .context(ReadMetadataSnafu { path: &root_path })?
            .is_dir()
            .then_some(())
            .context(NotADirectorySnafu { path: &root_path })?;
        Ok(Self { id, root_path })
    }

    pub(crate) fn id(&self) -> &RegistryId {
        &self.id
    }

    fn package_root_path(&self) -> PathBuf {
        self.root_path.join("packages")
    }

    fn package_versions_path(&self, name: &PackageName) -> PathBuf {
        self.package_root_path().join(name)
    }

    fn package_dir_path(&self, pkg_id: &PackageId) -> PathBuf {
        self.package_versions_path(pkg_id.name())
            .join(pkg_id.version().to_string())
    }

    pub(crate) fn find_latest_package_by_spec(
        &self,
        pkg_spec: &PackageSpec,
        include_pre_release: bool,
    ) -> Result<Option<Arc<PackageDefinition>>, RegistryIndexError> {
        match pkg_spec {
            PackageSpec::Name(name) => self.find_latest_package_by_name(name, include_pre_release),
            PackageSpec::Id(pkg_id) => self.find_package_by_id(pkg_id),
        }
    }

    pub(crate) fn find_package_by_id(
        &self,
        pkg_id: &PackageId,
    ) -> Result<Option<Arc<PackageDefinition>>, RegistryIndexError> {
        let package_dir = self.package_dir_path(pkg_id);
        if check_dir_presence(&package_dir)?.is_not_found() {
            return Ok(None);
        }
        let manifest_path = package_dir.join("manifest.toml");
        let manifest =
            PackageManifest::read(&manifest_path).context(ReadManifestSnafu { pkg_id })?;
        let pkg = PackageDefinition::from(manifest);
        snafu::ensure!(
            pkg.id == *pkg_id,
            PackageIdMismatchSnafu {
                expected: pkg_id,
                got: &pkg.id,
                manifest_path,
            }
        );
        Ok(Some(Arc::new(pkg)))
    }

    pub(crate) fn find_latest_package_by_name(
        &self,
        name: &PackageName,
        include_pre_release: bool,
    ) -> Result<Option<Arc<PackageDefinition>>, RegistryIndexError> {
        let versions_dir = self.package_versions_path(name);
        let Some(versions) =
            read_child_directories::<PackageVersion, _, _, _>(versions_dir, |path| {
                InvalidVersionInDirectoryEntrySnafu { path }
            })?
        else {
            return Ok(None);
        };
        let Some(version) = versions
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .filter(|version| include_pre_release || !version.is_pre_release())
            .max()
        else {
            return Ok(None);
        };
        let pkg_id = PackageId::new(name.clone(), version.clone());
        self.find_package_by_id(&pkg_id)
    }

    pub(crate) fn all_latest_packages(
        &self,
        include_pre_release: bool,
    ) -> Result<
        Option<impl Iterator<Item = Result<Arc<PackageDefinition>, RegistryIndexError>>>,
        RegistryIndexError,
    > {
        let package_root_dir = self.package_root_path();
        let Some(names) =
            read_child_directories::<PackageName, _, _, _>(package_root_dir, |path| {
                InvalidPackageNameInDirectoryEntrySnafu { path }
            })?
        else {
            return Ok(None);
        };
        Ok(Some(names.filter_map(move |res| {
            res.and_then(|name| self.find_latest_package_by_name(&name, include_pre_release))
                .transpose()
        })))
    }
}

fn read_child_directories<T, P, F, C>(
    path: P,
    mut f: F,
) -> Result<Option<impl Iterator<Item = Result<T, RegistryIndexError>>>, RegistryIndexError>
where
    T: FromStr,
    P: AsRef<Path>,
    F: FnMut(PathBuf) -> C,
    C: IntoError<RegistryIndexError, Source = T::Err>,
{
    let entries = match fs::read_dir(path.as_ref()) {
        Ok(entries) => entries,
        Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(ReadDirSnafu {
                path: path.as_ref(),
            }
            .into_error(source));
        }
    };

    Ok(Some(entries.into_iter().filter_map(move |entry| {
        (|| {
            let path = path.as_ref();
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
    use std::{assert_matches, fs};

    use super::*;
    use crate::util::testing;

    #[test]
    fn find_package_by_id_reads_manifest() {
        let (registry_dir, registry) = testing::make_registry_index("test-registry");
        testing::write_manifest(registry_dir.path(), "example-font@0.1.0");

        let pkg_id: PackageId = "example-font@0.1.0".parse().unwrap();
        let pkg = registry.find_package_by_id(&pkg_id).unwrap().unwrap();

        assert_eq!(pkg.id, pkg_id);
    }

    #[test]
    fn find_latest_package_by_name_picks_latest_version() {
        let (registry_dir, registry) = testing::make_registry_index("test-registry");
        testing::write_manifest(registry_dir.path(), "example-font@0.1.0");
        testing::write_manifest(registry_dir.path(), "example-font@0.2.0");

        let name: PackageName = "example-font".parse().unwrap();
        let pkg = registry
            .find_latest_package_by_name(&name, false)
            .unwrap()
            .unwrap();

        assert_eq!(pkg.id.to_string(), "example-font@0.2.0");
    }

    #[test]
    fn find_latest_package_by_name_skips_pre_release_by_default() {
        let (registry_dir, registry) = testing::make_registry_index("test-registry");
        testing::write_manifest(registry_dir.path(), "example-font@1.0.0");
        testing::write_manifest(registry_dir.path(), "example-font@2.0.0-rc-1");

        let name: PackageName = "example-font".parse().unwrap();
        let stable = registry
            .find_latest_package_by_name(&name, false)
            .unwrap()
            .unwrap();
        let pre_release = registry
            .find_latest_package_by_name(&name, true)
            .unwrap()
            .unwrap();

        assert_eq!(stable.id.to_string(), "example-font@1.0.0");
        assert_eq!(pre_release.id.to_string(), "example-font@2.0.0-rc-1");
    }

    #[test]
    fn find_latest_package_by_spec_allows_exact_pre_release_id_without_flag() {
        let (registry_dir, registry) = testing::make_registry_index("test-registry");
        testing::write_manifest(registry_dir.path(), "example-font@2.0.0-rc-1");

        let spec: PackageSpec = "example-font@2.0.0-rc-1".parse().unwrap();
        let pkg = registry
            .find_latest_package_by_spec(&spec, false)
            .unwrap()
            .unwrap();

        assert_eq!(pkg.id.to_string(), "example-font@2.0.0-rc-1");
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
        assert_matches!(err, RegistryIndexError::PackageIdMismatch { .. });
    }

    #[test]
    fn all_latest_packages_returns_latest_manifest_per_package_name() {
        let (registry_dir, registry) = testing::make_registry_index("test-registry");
        testing::write_manifest(registry_dir.path(), "example-font@0.1.0");
        testing::write_manifest(registry_dir.path(), "example-font@0.2.0");
        testing::write_manifest(registry_dir.path(), "other-font@1.0.0");
        fs::create_dir_all(registry_dir.path().join("empty-font")).unwrap();

        let pkgs = registry
            .all_latest_packages(false)
            .unwrap()
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let mut ids = pkgs
            .into_iter()
            .map(|pkg| pkg.id.to_string())
            .collect::<Vec<_>>();
        ids.sort();

        assert_eq!(ids, ["example-font@0.2.0", "other-font@1.0.0"]);
    }

    #[test]
    fn all_latest_packages_skips_pre_release_only_packages_by_default() {
        let (registry_dir, registry) = testing::make_registry_index("test-registry");
        testing::write_manifest(registry_dir.path(), "alpha-font@1.0.0-rc-1");
        testing::write_manifest(registry_dir.path(), "stable-font@1.0.0");
        testing::write_manifest(registry_dir.path(), "stable-font@2.0.0-rc-1");

        let stable_ids = registry
            .all_latest_packages(false)
            .unwrap()
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
            .into_iter()
            .map(|pkg| pkg.id.to_string())
            .collect::<Vec<_>>();
        let all_ids = registry
            .all_latest_packages(true)
            .unwrap()
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
            .into_iter()
            .map(|pkg| pkg.id.to_string())
            .collect::<Vec<_>>();

        assert_eq!(stable_ids, ["stable-font@1.0.0"]);
        assert_eq!(all_ids.len(), 2);
        assert!(all_ids.contains(&"alpha-font@1.0.0-rc-1".to_owned()));
        assert!(all_ids.contains(&"stable-font@2.0.0-rc-1".to_owned()));
    }
}
