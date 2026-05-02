use std::{
    collections::BTreeSet,
    fs::File,
    io::{self, BufReader},
    path::PathBuf,
};

use snafu::{IntoError as _, OptionExt as _, ResultExt as _, Snafu};
use tempfile::NamedTempFile;

use crate::{
    db::{
        DbLockFileGuard,
        persist::{self, PersistError, PersistedPackageDb, PersistedPackageEntry},
    },
    package::{
        PackageId, PackageManifest, PackageName, PackageQualifiedName, PackageSpec, PackageState,
        PackageVersion,
    },
    util::{app_dirs::AppDirs, path::AbsolutePath},
};

#[derive(Debug, Snafu)]
pub(crate) enum PackageDatabaseError {
    #[snafu(display("failed to open database file: {}", path.display()))]
    OpenDatabase { path: PathBuf, source: io::Error },
    #[snafu(display("failed to create temporary file for database: {}", path.display()))]
    CreateTempFile { path: PathBuf, source: io::Error },
    #[snafu(display("failed to deserialize database file: {}", path.display()))]
    DeserializeDatabase { path: PathBuf, source: PersistError },
    #[snafu(display("failed to serialize database to temporary file: {}", path.display()))]
    SerializeDatabase { path: PathBuf, source: PersistError },
    #[snafu(display("failed to persist temporary file to database file: {}", path.display()))]
    PersistTempFile { path: PathBuf, source: io::Error },
    #[snafu(display("database entry not found for package ID: {pkg_id}"))]
    EntryNotFound { pkg_id: PackageId },
    #[snafu(display(
        "database entry for package ID {pkg_id} is in unexpected state: expected {expected}, actual {actual}"
    ))]
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
    _lock_file_guard: DbLockFileGuard<'a>,
}

fn load(path: &AbsolutePath) -> Result<PersistedPackageDb, PackageDatabaseError> {
    if !path.as_path().exists() {
        return Ok(PersistedPackageDb::default());
    }

    let file = File::open(path).context(OpenDatabaseSnafu { path })?;
    let mut reader = BufReader::new(file);
    let db = persist::from_reader(&mut reader).context(DeserializeDatabaseSnafu { path })?;

    Ok(db)
}

impl<'a> PackageDatabase<'a> {
    pub(crate) fn load(
        app_dirs: &AppDirs,
        lock_file_guard: DbLockFileGuard<'a>,
    ) -> Result<Self, PackageDatabaseError> {
        let persist_path = app_dirs.db_json_file();
        let persist_db = load(&persist_path)?;
        Ok(Self {
            persist_path,
            persist_db,
            _lock_file_guard: lock_file_guard,
        })
    }

    #[cfg(test)]
    pub(crate) fn reload(&mut self) -> Result<(), PackageDatabaseError> {
        self.persist_db = load(&self.persist_path)?;
        Ok(())
    }

    pub(crate) fn save(&mut self) -> Result<(), PackageDatabaseError> {
        let persist_dir = self.persist_path.parent().unwrap();
        let mut temp_file = NamedTempFile::new_in(&persist_dir)
            .context(CreateTempFileSnafu { path: &persist_dir })?;
        persist::to_writer(&temp_file, &self.persist_db).context(SerializeDatabaseSnafu {
            path: temp_file.path(),
        })?;
        temp_file
            .as_file_mut()
            .sync_all()
            .context(PersistTempFileSnafu {
                path: &self.persist_path,
            })?;
        temp_file.persist(&self.persist_path).map_err(|source| {
            PersistTempFileSnafu {
                path: &self.persist_path,
            }
            .into_error(source.error)
        })?;
        Ok(())
    }

    pub(crate) fn entries(&self) -> impl Iterator<Item = (PackageState, &PackageManifest)> {
        self.persist_db
            .packages
            .values()
            .flat_map(|version_map| version_map.versions.values())
            .map(|entry| (entry.state, &entry.manifest))
    }

    pub(crate) fn entries_by_spec(
        &'a self,
        pkg_spec: &PackageSpec,
    ) -> Box<dyn Iterator<Item = (PackageState, &'a PackageManifest)> + 'a> {
        match pkg_spec {
            PackageSpec::Id(id) => Box::new(self.entry_by_id(id).into_iter()),
            PackageSpec::QualifiedName(qualified_name) => {
                Box::new(self.entries_by_qualified_name(qualified_name))
            }
            PackageSpec::Name(name) => Box::new(self.entries_by_name(name)),
        }
    }

    pub(crate) fn entry_by_id(
        &'a self,
        pkg_id: &PackageId,
    ) -> Option<(PackageState, &'a PackageManifest)> {
        self.persist_db
            .packages
            .get(pkg_id.qualified_name())
            .and_then(|version_map| version_map.versions.get(pkg_id.version()))
            .map(|entry| (entry.state, &entry.manifest))
    }

    pub(crate) fn entries_by_qualified_name(
        &'a self,
        pkg_name: &PackageQualifiedName,
    ) -> impl Iterator<Item = (PackageState, &'a PackageManifest)> + 'a {
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
        &'a self,
        pkg_name: &PackageName,
    ) -> impl Iterator<Item = (PackageState, &'a PackageManifest)> + 'a {
        let pkg_name = pkg_name.clone();
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
            return BeginInstallResult::PendingUninstallFound(pending_uninstalls);
        }
        if !pending_installs.is_empty() {
            return BeginInstallResult::PendingInstallFound(pending_installs);
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
            .context(EntryNotFoundSnafu { pkg_id })?;
        snafu::ensure!(
            entry.state == PackageState::PendingInstall,
            UnexpectedStateSnafu {
                pkg_id,
                expected: PackageState::PendingInstall,
                actual: entry.state,
            }
        );
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
            .context(EntryNotFoundSnafu { pkg_id })?;
        let entry = version_map
            .versions
            .get_mut(pkg_id.version())
            .context(EntryNotFoundSnafu { pkg_id })?;
        snafu::ensure!(
            entry.state == PackageState::PendingInstall,
            UnexpectedStateSnafu {
                pkg_id,
                expected: PackageState::PendingInstall,
                actual: entry.state,
            }
        );
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
            .context(EntryNotFoundSnafu { pkg_id })?;
        let entry = version_map
            .versions
            .get_mut(pkg_id.version())
            .context(EntryNotFoundSnafu { pkg_id })?;
        snafu::ensure!(
            entry.state == PackageState::PendingUninstall,
            UnexpectedStateSnafu {
                pkg_id,
                expected: PackageState::PendingUninstall,
                actual: entry.state,
            }
        );
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
    OtherVersionInstalled(PackageVersion),
    PendingInstallFound(BTreeSet<PackageVersion>),
    PendingUninstallFound(BTreeSet<PackageVersion>),
}

#[derive(Debug, Clone, derive_more::IsVariant)]
pub(crate) enum BeginUninstallResult {
    CanUninstall,
    NotFound,
}

#[cfg(test)]
mod tests {
    use semver::Version;

    use super::*;
    use crate::{db::DbLockFile, util::testing};

    fn get_entry_state(db: &PackageDatabase<'_>, id: &PackageId) -> PackageState {
        let (state, manifest) = db.entry_by_id(id).unwrap();
        assert_eq!(manifest.metadata.id(), *id);
        state
    }

    #[test]
    fn save_and_load_round_trip_installed_entry() {
        let (_tempdir, app_dirs) = testing::make_app_dirs();
        let mut lock_file = DbLockFile::open(&app_dirs).unwrap();

        let manifest = testing::make_manifest("example-namespace", "example-font", "0.1.0");
        let pkg_id = manifest.metadata.id();

        {
            let lock_file_guard = lock_file.try_acquire().unwrap();
            let mut db = PackageDatabase::load(&app_dirs, lock_file_guard).unwrap();

            assert!(matches!(
                db.begin_install(&manifest),
                BeginInstallResult::CanInstall
            ));
            db.complete_install(&pkg_id).unwrap();
            db.save().unwrap();
        }

        {
            let lock_file_guard = lock_file.try_acquire().unwrap();
            let db = PackageDatabase::load(&app_dirs, lock_file_guard).unwrap();
            assert_eq!(get_entry_state(&db, &pkg_id), PackageState::Installed);
        }
    }

    #[test]
    fn begin_install_reports_pending_install_versions() {
        let (_tempdir, app_dirs) = testing::make_app_dirs();
        let mut lock_file = DbLockFile::open(&app_dirs).unwrap();
        let lock_file_guard = lock_file.try_acquire().unwrap();

        let manifest = testing::make_manifest("example-namespace", "example-font", "0.1.0");
        let mut db = PackageDatabase::load(&app_dirs, lock_file_guard).unwrap();

        assert!(matches!(
            db.begin_install(&manifest),
            BeginInstallResult::CanInstall
        ));

        let result = db.begin_install(&manifest);
        assert!(matches!(
            result,
            BeginInstallResult::PendingInstallFound(ref versions)
                if versions.contains(&Version::new(0, 1, 0).into())
        ));
    }

    #[test]
    fn begin_install_reports_other_installed_version() {
        let (_tempdir, app_dirs) = testing::make_app_dirs();
        let mut lock_file = DbLockFile::open(&app_dirs).unwrap();
        let lock_file_guard = lock_file.try_acquire().unwrap();

        let installed_manifest =
            testing::make_manifest("example-namespace", "example-font", "0.1.0");
        let installed_pkg_id = installed_manifest.metadata.id();
        let next_manifest = testing::make_manifest("example-namespace", "example-font", "0.2.0");
        let mut db = PackageDatabase::load(&app_dirs, lock_file_guard).unwrap();

        assert!(matches!(
            db.begin_install(&installed_manifest),
            BeginInstallResult::CanInstall
        ));
        db.complete_install(&installed_pkg_id).unwrap();

        let result = db.begin_install(&next_manifest);
        assert!(matches!(
            result,
            BeginInstallResult::OtherVersionInstalled(version)
                if version == Version::new(0, 1, 0)
        ));
    }

    #[test]
    fn cancel_install_removes_pending_entry() {
        let (_tempdir, app_dirs) = testing::make_app_dirs();
        let mut lock_file = DbLockFile::open(&app_dirs).unwrap();
        let lock_file_guard = lock_file.try_acquire().unwrap();

        let manifest = testing::make_manifest("example-namespace", "example-font", "0.1.0");
        let pkg_id = manifest.metadata.id();
        let mut db = PackageDatabase::load(&app_dirs, lock_file_guard).unwrap();

        assert!(matches!(
            db.begin_install(&manifest),
            BeginInstallResult::CanInstall
        ));
        db.cancel_install(&pkg_id).unwrap();

        assert!(db.entry_by_id(&pkg_id).is_none());
    }

    #[test]
    fn complete_uninstall_removes_last_version_entry() {
        let (_tempdir, app_dirs) = testing::make_app_dirs();
        let mut lock_file = DbLockFile::open(&app_dirs).unwrap();
        let lock_file_guard = lock_file.try_acquire().unwrap();

        let manifest = testing::make_manifest("example-namespace", "example-font", "0.1.0");
        let pkg_id = manifest.metadata.id();
        let mut db = PackageDatabase::load(&app_dirs, lock_file_guard).unwrap();

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

        assert!(db.entry_by_id(&pkg_id).is_none());
    }

    #[test]
    fn begin_install_reports_pending_uninstall_versions() {
        let (_tempdir, app_dirs) = testing::make_app_dirs();
        let mut lock_file = DbLockFile::open(&app_dirs).unwrap();
        let lock_file_guard = lock_file.try_acquire().unwrap();

        let manifest = testing::make_manifest("example-namespace", "example-font", "0.1.0");
        let pkg_id = manifest.metadata.id();
        let mut db = PackageDatabase::load(&app_dirs, lock_file_guard).unwrap();

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
            BeginInstallResult::PendingUninstallFound(ref versions)
                if versions.contains(&Version::new(0, 1, 0).into())
        ));
    }
}
