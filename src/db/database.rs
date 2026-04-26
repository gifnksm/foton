use std::{
    collections::BTreeSet,
    fs::File,
    io::{self, BufReader},
};

use semver::Version;
use tempfile::NamedTempFile;

use crate::{
    db::{
        DbLockFileGuard,
        persist::{self, PersistError, PersistedPackageDb, PersistedPackageEntry},
    },
    package::{PackageId, PackageManifest, PackageName, PackageQualifiedName, PackageState},
    util::{app_dirs::AppDirs, path::AbsolutePath},
};

#[derive(Debug, derive_more::Display, derive_more::Error)]
pub(crate) enum PackageDatabaseError {
    #[display("failed to open database file: {}", path.display())]
    OpenDatabase {
        path: AbsolutePath,
        #[error(source)]
        source: io::Error,
    },
    #[display("failed to create temporary file for database: {}", path.display())]
    CreateTempFile {
        path: AbsolutePath,
        #[error(source)]
        source: io::Error,
    },
    #[display("failed to deserialize database file: {}", path.display())]
    DeserializeDatabase {
        path: AbsolutePath,
        #[error(source)]
        source: PersistError,
    },
    #[display("failed to serialize database to temporary file: {}", path.display())]
    SerializeDatabase {
        path: AbsolutePath,
        #[error(source)]
        source: PersistError,
    },
    #[display("failed to persist temporary file to database file: {}", path.display())]
    PersistTempFile {
        path: AbsolutePath,
        #[error(source)]
        source: io::Error,
    },
    #[display("database entry not found for package ID: {pkg_id}")]
    EntryNotFound { pkg_id: PackageId },
    #[display(
        "database entry for package ID {pkg_id} is in unexpected state: expected {expected}, actual {actual}"
    )]
    UnexpectedState {
        pkg_id: PackageId,
        expected: PackageState,
        actual: PackageState,
    },
}

#[derive(Debug)]
pub(crate) struct PackageDatabase<'a> {
    persist_path: AbsolutePath,
    persist_db: PersistedPackageDb,
    _lock_file_guard: &'a DbLockFileGuard<'a>,
}

impl<'a> PackageDatabase<'a> {
    pub(crate) fn load(
        app_dirs: &AppDirs,
        lock_file_guard: &'a DbLockFileGuard<'a>,
    ) -> Result<Self, PackageDatabaseError> {
        let persist_path = app_dirs.db_json_file();

        let persist_db = if persist_path.as_path().exists() {
            let file = File::open(&persist_path).map_err(|source| {
                let path = persist_path.clone();
                PackageDatabaseError::OpenDatabase { path, source }
            })?;
            let mut reader = BufReader::new(file);
            persist::from_reader(&mut reader).map_err(|source| {
                let path = persist_path.clone();
                PackageDatabaseError::DeserializeDatabase { path, source }
            })?
        } else {
            PersistedPackageDb::default()
        };

        Ok(Self {
            persist_path,
            persist_db,
            _lock_file_guard: lock_file_guard,
        })
    }

    pub(crate) fn save(&mut self) -> Result<(), PackageDatabaseError> {
        let persist_dir = self.persist_path.as_path().parent().unwrap();
        let mut temp_file = NamedTempFile::new_in(persist_dir).map_err(|source| {
            let path = AbsolutePath::new(persist_dir).unwrap();
            PackageDatabaseError::CreateTempFile { path, source }
        })?;
        persist::to_writer(&temp_file, &self.persist_db).map_err(|source| {
            let path = AbsolutePath::new(temp_file.path()).unwrap();
            PackageDatabaseError::SerializeDatabase { path, source }
        })?;
        temp_file.as_file_mut().sync_all().map_err(|source| {
            let path = self.persist_path.clone();
            PackageDatabaseError::PersistTempFile { path, source }
        })?;
        temp_file.persist(&self.persist_path).map_err(|source| {
            let path = self.persist_path.clone();
            let source = source.error;
            PackageDatabaseError::PersistTempFile { path, source }
        })?;
        Ok(())
    }

    pub(crate) fn entry_by_id(
        &self,
        pkg_id: &PackageId,
    ) -> Option<(PackageState, &PackageManifest)> {
        self.persist_db
            .packages
            .get(pkg_id.qualified_name())
            .and_then(|version_map| version_map.versions.get(pkg_id.version()))
            .map(|entry| (entry.state, &entry.manifest))
    }

    pub(crate) fn entries_by_qualified_name(
        &self,
        pkg_name: &PackageQualifiedName,
    ) -> impl Iterator<Item = (PackageState, &PackageManifest)> {
        self.persist_db
            .packages
            .get(pkg_name)
            .into_iter()
            .flat_map(|version_map| {
                version_map
                    .versions
                    .values()
                    .map(|packages| (packages.state, &packages.manifest))
            })
    }

    pub(crate) fn entries_by_name(
        &self,
        pkg_name: &PackageName,
    ) -> impl Iterator<Item = (PackageState, &PackageManifest)> {
        self.persist_db
            .packages
            .iter()
            .filter(move |(qualified_name, _)| qualified_name.name() == pkg_name)
            .flat_map(|(_, version_map)| {
                version_map
                    .versions
                    .values()
                    .map(|packages| (packages.state, &packages.manifest))
            })
    }

    pub(crate) fn begin_install(&mut self, manifest: &PackageManifest) -> BeginInstallResult {
        let mut pending_installs = BTreeSet::new();
        let mut pending_uninstalls = BTreeSet::new();
        let pkg_name = manifest.metadata.qualified_name.clone();
        let pkg_version = manifest.metadata.version.clone();
        for (state, m) in self.entries_by_qualified_name(&pkg_name) {
            match state {
                PackageState::Installed => {
                    let result = if m.metadata.version == pkg_version {
                        BeginInstallResult::AlreadyInstalled
                    } else {
                        BeginInstallResult::OtherVersionInstalled(m.metadata.version.clone())
                    };
                    return result;
                }
                PackageState::PendingInstall => {
                    pending_installs.insert(m.metadata.version.clone());
                }
                PackageState::PendingUninstall => {
                    pending_uninstalls.insert(m.metadata.version.clone());
                }
            }
        }
        if !pending_uninstalls.is_empty() {
            return BeginInstallResult::HavePendingUninstall(pending_uninstalls);
        }
        if !pending_installs.is_empty() {
            return BeginInstallResult::HavePendingInstall(pending_installs);
        }
        self.persist_db
            .packages
            .entry(pkg_name)
            .or_default()
            .versions
            .insert(
                pkg_version.clone(),
                PersistedPackageEntry {
                    state: PackageState::PendingInstall,
                    manifest: manifest.clone(),
                },
            );
        BeginInstallResult::CanInstall
    }

    pub(crate) fn complete_install(
        &mut self,
        pkg_id: &PackageId,
    ) -> Result<(), PackageDatabaseError> {
        let entry = self
            .persist_db
            .packages
            .get_mut(pkg_id.qualified_name())
            .and_then(|version_map| version_map.versions.get_mut(pkg_id.version()))
            .ok_or_else(|| {
                let pkg_id = pkg_id.clone();
                PackageDatabaseError::EntryNotFound { pkg_id }
            })?;
        if entry.state != PackageState::PendingInstall {
            return Err({
                let pkg_id = pkg_id.clone();
                let expected = PackageState::PendingInstall;
                let actual = entry.state;
                PackageDatabaseError::UnexpectedState {
                    pkg_id,
                    expected,
                    actual,
                }
            });
        }
        entry.state = PackageState::Installed;
        Ok(())
    }

    pub(crate) fn cancel_install(
        &mut self,
        pkg_id: &PackageId,
    ) -> Result<(), PackageDatabaseError> {
        let version_map = self
            .persist_db
            .packages
            .get_mut(pkg_id.qualified_name())
            .ok_or_else(|| {
                let pkg_id = pkg_id.clone();
                PackageDatabaseError::EntryNotFound { pkg_id }
            })?;
        let entry = version_map
            .versions
            .get_mut(pkg_id.version())
            .ok_or_else(|| {
                let pkg_id = pkg_id.clone();
                PackageDatabaseError::EntryNotFound { pkg_id }
            })?;
        if entry.state != PackageState::PendingInstall {
            return Err({
                let pkg_id = pkg_id.clone();
                let expected = PackageState::PendingInstall;
                let actual = entry.state;
                PackageDatabaseError::UnexpectedState {
                    pkg_id,
                    expected,
                    actual,
                }
            });
        }
        version_map.versions.remove(pkg_id.version());
        if version_map.versions.is_empty() {
            self.persist_db.packages.remove(pkg_id.qualified_name());
        }
        Ok(())
    }

    pub(crate) fn begin_uninstall(&mut self, pkg_id: &PackageId) -> BeginUninstallResult {
        let Some(entry) = self
            .persist_db
            .packages
            .get_mut(pkg_id.qualified_name())
            .and_then(|version_map| version_map.versions.get_mut(pkg_id.version()))
        else {
            return BeginUninstallResult::NotFound;
        };
        entry.state = PackageState::PendingUninstall;
        BeginUninstallResult::CanUninstall
    }

    pub(crate) fn complete_uninstall(
        &mut self,
        pkg_id: &PackageId,
    ) -> Result<(), PackageDatabaseError> {
        let version_map = self
            .persist_db
            .packages
            .get_mut(pkg_id.qualified_name())
            .ok_or_else(|| {
                let pkg_id = pkg_id.clone();
                PackageDatabaseError::EntryNotFound { pkg_id }
            })?;
        let entry = version_map
            .versions
            .get_mut(pkg_id.version())
            .ok_or_else(|| {
                let pkg_id = pkg_id.clone();
                PackageDatabaseError::EntryNotFound { pkg_id }
            })?;
        if entry.state != PackageState::PendingUninstall {
            return Err({
                let pkg_id = pkg_id.clone();
                let expected = PackageState::PendingUninstall;
                let actual = entry.state;
                PackageDatabaseError::UnexpectedState {
                    pkg_id,
                    expected,
                    actual,
                }
            });
        }
        version_map.versions.remove(pkg_id.version());
        if version_map.versions.is_empty() {
            self.persist_db.packages.remove(pkg_id.qualified_name());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, derive_more::IsVariant)]
pub(crate) enum BeginInstallResult {
    CanInstall,
    AlreadyInstalled,
    OtherVersionInstalled(Version),
    HavePendingInstall(BTreeSet<Version>),
    HavePendingUninstall(BTreeSet<Version>),
}

#[derive(Debug, Clone, derive_more::IsVariant)]
pub(crate) enum BeginUninstallResult {
    CanUninstall,
    NotFound,
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;
    use crate::db::DbLockFile;

    fn make_app_dirs() -> (TempDir, AppDirs) {
        let tempdir = tempfile::tempdir().unwrap();
        let data_local_dir = AbsolutePath::new(tempdir.path()).unwrap();
        let app_dirs = AppDirs::new_for_test(data_local_dir);
        (tempdir, app_dirs)
    }

    fn load_db<'a>(
        app_dirs: &AppDirs,
        lock_file_guard: &'a DbLockFileGuard<'a>,
    ) -> PackageDatabase<'a> {
        PackageDatabase::load(app_dirs, lock_file_guard).unwrap()
    }

    fn test_manifest(version: &str) -> PackageManifest {
        toml::from_str(&format!(
            r#"
[package]
name = "yuru7/hackgen"
version = "{version}"

[[sources]]
url = "https://example.com/hackgen.zip"
hash = "sha256:ed182e2a4b95792d94dea7932f6b45280b5ae353651be249d5f6b7867b788db7"
"#
        ))
        .unwrap()
    }

    #[test]
    fn save_and_load_round_trip_installed_entry() {
        let (_tempdir, app_dirs) = make_app_dirs();
        let mut lock_file = DbLockFile::open(&app_dirs).unwrap();
        let lock_file_guard = lock_file.try_acquire().unwrap();

        {
            let manifest = test_manifest("2.10.0");
            let pkg_id = manifest.metadata.id();
            let mut db = load_db(&app_dirs, &lock_file_guard);

            assert!(matches!(
                db.begin_install(&manifest),
                BeginInstallResult::CanInstall
            ));
            db.complete_install(&pkg_id).unwrap();
            db.save().unwrap();
        }

        let db = load_db(&app_dirs, &lock_file_guard);
        let pkg_name = "yuru7/hackgen".parse::<PackageQualifiedName>().unwrap();
        let entries = db.entries_by_qualified_name(&pkg_name).collect::<Vec<_>>();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].0, PackageState::Installed);
        assert_eq!(entries[0].1.metadata.version, Version::new(2, 10, 0));
    }

    #[test]
    fn begin_install_reports_pending_install_versions() {
        let (_tempdir, app_dirs) = make_app_dirs();
        let mut lock_file = DbLockFile::open(&app_dirs).unwrap();
        let lock_file_guard = lock_file.try_acquire().unwrap();

        let manifest = test_manifest("2.10.0");
        let mut db = load_db(&app_dirs, &lock_file_guard);

        assert!(matches!(
            db.begin_install(&manifest),
            BeginInstallResult::CanInstall
        ));

        let result = db.begin_install(&manifest);
        assert!(matches!(
            result,
            BeginInstallResult::HavePendingInstall(ref versions)
                if versions.contains(&Version::new(2, 10, 0))
        ));
    }

    #[test]
    fn begin_install_reports_other_installed_version() {
        let (_tempdir, app_dirs) = make_app_dirs();
        let mut lock_file = DbLockFile::open(&app_dirs).unwrap();
        let lock_file_guard = lock_file.try_acquire().unwrap();

        let installed_manifest = test_manifest("2.10.0");
        let installed_pkg_id = installed_manifest.metadata.id();
        let next_manifest = test_manifest("2.11.0");
        let mut db = load_db(&app_dirs, &lock_file_guard);

        assert!(matches!(
            db.begin_install(&installed_manifest),
            BeginInstallResult::CanInstall
        ));
        db.complete_install(&installed_pkg_id).unwrap();

        let result = db.begin_install(&next_manifest);
        assert!(matches!(
            result,
            BeginInstallResult::OtherVersionInstalled(version)
                if version == Version::new(2, 10, 0)
        ));
    }

    #[test]
    fn cancel_install_removes_pending_entry() {
        let (_tempdir, app_dirs) = make_app_dirs();
        let mut lock_file = DbLockFile::open(&app_dirs).unwrap();
        let lock_file_guard = lock_file.try_acquire().unwrap();

        let manifest = test_manifest("2.10.0");
        let pkg_id = manifest.metadata.id();
        let pkg_name = pkg_id.qualified_name().clone();
        let mut db = load_db(&app_dirs, &lock_file_guard);

        assert!(matches!(
            db.begin_install(&manifest),
            BeginInstallResult::CanInstall
        ));
        db.cancel_install(&pkg_id).unwrap();

        assert_eq!(db.entries_by_qualified_name(&pkg_name).count(), 0);
    }

    #[test]
    fn complete_uninstall_removes_last_version_entry() {
        let (_tempdir, app_dirs) = make_app_dirs();
        let mut lock_file = DbLockFile::open(&app_dirs).unwrap();
        let lock_file_guard = lock_file.try_acquire().unwrap();

        let manifest = test_manifest("2.10.0");
        let pkg_id = manifest.metadata.id();
        let pkg_name = pkg_id.qualified_name().clone();
        let mut db = load_db(&app_dirs, &lock_file_guard);

        assert!(matches!(
            db.begin_install(&manifest),
            BeginInstallResult::CanInstall
        ));
        db.complete_install(&pkg_id).unwrap();
        assert!(matches!(
            db.begin_uninstall(&pkg_id),
            BeginUninstallResult::CanUninstall
        ));
        db.complete_uninstall(&pkg_id).unwrap();

        assert_eq!(db.entries_by_qualified_name(&pkg_name).count(), 0);
    }

    #[test]
    fn begin_install_reports_pending_uninstall_versions() {
        let (_tempdir, app_dirs) = make_app_dirs();
        let mut lock_file = DbLockFile::open(&app_dirs).unwrap();
        let lock_file_guard = lock_file.try_acquire().unwrap();

        let manifest = test_manifest("2.10.0");
        let pkg_id = manifest.metadata.id();
        let mut db = load_db(&app_dirs, &lock_file_guard);

        assert!(matches!(
            db.begin_install(&manifest),
            BeginInstallResult::CanInstall
        ));
        db.complete_install(&pkg_id).unwrap();
        assert!(matches!(
            db.begin_uninstall(&pkg_id),
            BeginUninstallResult::CanUninstall
        ));

        let result = db.begin_install(&manifest);
        assert!(matches!(
            result,
            BeginInstallResult::HavePendingUninstall(ref versions)
                if versions.contains(&Version::new(2, 10, 0))
        ));
    }
}
