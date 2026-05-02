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
    #[cfg(test)]
    #[snafu(display("simulated failure to save package database"))]
    SimulatedSaveFailure,
}

#[derive(Debug)]
pub(crate) struct PackageDatabase<'a> {
    persist_path: AbsolutePath,
    persist_db: PersistedPackageDb,
    _lock_file_guard: DbLockFileGuard<'a>,
    #[cfg(test)]
    simulate_save_failure: bool,
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
            #[cfg(test)]
            simulate_save_failure: false,
        })
    }

    #[cfg(test)]
    pub(crate) fn reload(&mut self) -> Result<(), PackageDatabaseError> {
        self.persist_db = load(&self.persist_path)?;
        Ok(())
    }

    fn save(&mut self) -> Result<(), PackageDatabaseError> {
        #[cfg(test)]
        {
            if self.simulate_save_failure {
                return Err(SimulatedSaveFailureSnafu.build());
            }
        }

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

    fn entry_mut(&mut self, pkg_id: &PackageId) -> Option<&mut PersistedPackageEntry> {
        let version_map = self.persist_db.packages.get_mut(pkg_id.qualified_name())?;
        version_map.versions.get_mut(pkg_id.version())
    }

    fn insert_entry(&mut self, state: PackageState, manifest: PackageManifest) {
        let pkg_name = manifest.metadata.qualified_name.clone();
        let pkg_version = manifest.metadata.version.clone();
        self.persist_db
            .packages
            .entry(pkg_name)
            .or_default()
            .versions
            .insert(pkg_version, PersistedPackageEntry { state, manifest });
    }

    fn remove_entry(&mut self, pkg_id: &PackageId) -> Option<PersistedPackageEntry> {
        let version_map = self.persist_db.packages.get_mut(pkg_id.qualified_name())?;
        let entry = version_map.versions.remove(pkg_id.version())?;
        if version_map.versions.is_empty() {
            self.persist_db.packages.remove(pkg_id.qualified_name());
        }
        Some(entry)
    }

    pub(crate) fn begin_install(
        &mut self,
        manifest: &PackageManifest,
    ) -> Result<BeginInstallResult, PackageDatabaseError> {
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
                    return Ok(result);
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
            return Ok(BeginInstallResult::PendingUninstallFound(
                pending_uninstalls,
            ));
        }
        if !pending_installs.is_empty() {
            return Ok(BeginInstallResult::PendingInstallFound(pending_installs));
        }
        self.insert_entry(PackageState::PendingInstall, manifest.clone());
        if let Err(err) = self.save() {
            let pkg_id = PackageId::new(pkg_name, pkg_version);
            let _ = self.remove_entry(&pkg_id);
            return Err(err);
        }
        Ok(BeginInstallResult::CanInstall)
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
        if let Err(err) = self.save() {
            self.entry_mut(pkg_id).unwrap().state = PackageState::PendingInstall;
            return Err(err);
        }
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
        let entry = version_map.versions.remove(pkg_id.version()).unwrap();
        if version_map.versions.is_empty() {
            self.persist_db.packages.remove(pkg_id.qualified_name());
        }
        if let Err(err) = self.save() {
            self.insert_entry(PackageState::PendingInstall, entry.manifest);
            return Err(err);
        }
        Ok(())
    }

    pub(crate) fn begin_uninstall(
        &mut self,
        pkg_id: &PackageId,
    ) -> Result<BeginUninstallResult, PackageDatabaseError> {
        let Some(entry) = self
            .persist_db
            .packages
            .get_mut(pkg_id.qualified_name())
            .and_then(|version_map| version_map.versions.get_mut(pkg_id.version()))
        else {
            return Ok(BeginUninstallResult::NotFound);
        };
        let old_state = entry.state;
        entry.state = PackageState::PendingUninstall;
        if let Err(err) = self.save() {
            let entry = self.entry_mut(pkg_id).unwrap();
            entry.state = old_state;
            return Err(err);
        }
        Ok(BeginUninstallResult::CanUninstall)
    }

    pub(crate) fn complete_uninstall(
        &mut self,
        pkg_id: &PackageId,
    ) -> Result<(), PackageDatabaseError> {
        let Some(entry) = self.entry_mut(pkg_id) else {
            return Err(EntryNotFoundSnafu { pkg_id }.build());
        };
        snafu::ensure!(
            entry.state == PackageState::PendingUninstall,
            UnexpectedStateSnafu {
                pkg_id,
                expected: PackageState::PendingUninstall,
                actual: entry.state,
            }
        );
        let entry = self.remove_entry(pkg_id);
        if let Err(err) = self.save() {
            self.insert_entry(PackageState::PendingUninstall, entry.unwrap().manifest);
            return Err(err);
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

    fn get_entry_state(db: &PackageDatabase<'_>, id: &PackageId) -> Option<PackageState> {
        let (state, manifest) = db.entry_by_id(id)?;
        assert_eq!(manifest.metadata.id(), *id);
        Some(state)
    }

    fn get_persisted_state(
        app_dirs: &AppDirs,
        lock_file: &mut DbLockFile,
        id: &PackageId,
    ) -> Option<PackageState> {
        let lock_file_guard = lock_file.try_acquire().unwrap();
        let db = PackageDatabase::load(app_dirs, lock_file_guard).unwrap();
        get_entry_state(&db, id)
    }

    fn with_db<F, R>(app_dirs: &AppDirs, lock_file: &mut DbLockFile, f: F) -> R
    where
        F: FnOnce(&mut PackageDatabase<'_>) -> R,
    {
        let lock_file_guard = lock_file.try_acquire().unwrap();
        let mut db = PackageDatabase::load(app_dirs, lock_file_guard).unwrap();
        f(&mut db)
    }

    #[track_caller]
    fn assert_status_change<F>(
        app_dirs: &AppDirs,
        lock_file: &mut DbLockFile,
        id: &PackageId,
        before_state: Option<PackageState>,
        after_state: Option<PackageState>,
        f: F,
    ) where
        F: FnOnce(&mut PackageDatabase<'_>),
    {
        let conditions = &[(id, before_state, after_state)];
        assert_statuses_change(app_dirs, lock_file, conditions, f);
    }

    #[track_caller]
    fn assert_statuses_change<F>(
        app_dirs: &AppDirs,
        lock_file: &mut DbLockFile,
        conditions: &[(&PackageId, Option<PackageState>, Option<PackageState>)],
        f: F,
    ) where
        F: FnOnce(&mut PackageDatabase<'_>),
    {
        for (id, before_state, _) in conditions {
            assert_eq!(get_persisted_state(app_dirs, lock_file, id), *before_state);
        }

        with_db(app_dirs, lock_file, |db| {
            for (id, before_state, _) in conditions {
                assert_eq!(get_entry_state(db, id), *before_state);
            }
            f(db);
            for (id, _, after_state) in conditions {
                assert_eq!(get_entry_state(db, id), *after_state);
            }
        });

        for (id, _, after_state) in conditions {
            assert_eq!(get_persisted_state(app_dirs, lock_file, id), *after_state);
        }
    }

    #[test]
    fn complete_install_persists_installed_state() {
        let (_tempdir, app_dirs) = testing::make_app_dirs();
        let mut lock_file = DbLockFile::open(&app_dirs).unwrap();

        let manifest = testing::make_manifest("example-namespace", "example-font", "0.1.0");
        let pkg_id = manifest.metadata.id();

        assert_status_change(
            &app_dirs,
            &mut lock_file,
            &pkg_id,
            None,
            Some(PackageState::Installed),
            |db| {
                assert!(matches!(
                    db.begin_install(&manifest).unwrap(),
                    BeginInstallResult::CanInstall
                ));
                db.complete_install(&pkg_id).unwrap();
            },
        );
    }

    #[test]
    fn begin_install_reports_pending_install_versions() {
        let (_tempdir, app_dirs) = testing::make_app_dirs();
        let mut lock_file = DbLockFile::open(&app_dirs).unwrap();

        let manifest = testing::make_manifest("example-namespace", "example-font", "0.1.0");
        let pkg_id = manifest.metadata.id();

        with_db(&app_dirs, &mut lock_file, |db| {
            assert!(matches!(
                db.begin_install(&manifest).unwrap(),
                BeginInstallResult::CanInstall
            ));
        });
        assert_status_change(
            &app_dirs,
            &mut lock_file,
            &pkg_id,
            Some(PackageState::PendingInstall),
            Some(PackageState::PendingInstall),
            |db| {
                let result = db.begin_install(&manifest).unwrap();
                assert!(matches!(
                    result,
                    BeginInstallResult::PendingInstallFound(ref versions)
                        if versions.contains(&Version::new(0, 1, 0).into())
                ));
            },
        );
    }

    #[test]
    fn begin_install_reports_other_installed_version() {
        let (_tempdir, app_dirs) = testing::make_app_dirs();
        let mut lock_file = DbLockFile::open(&app_dirs).unwrap();

        let installed_manifest =
            testing::make_manifest("example-namespace", "example-font", "0.1.0");
        let installed_pkg_id = installed_manifest.metadata.id();
        let next_manifest = testing::make_manifest("example-namespace", "example-font", "0.2.0");
        let next_pkg_id = next_manifest.metadata.id();

        with_db(&app_dirs, &mut lock_file, |db| {
            assert!(matches!(
                db.begin_install(&installed_manifest).unwrap(),
                BeginInstallResult::CanInstall
            ));
            db.complete_install(&installed_pkg_id).unwrap();
        });
        assert_statuses_change(
            &app_dirs,
            &mut lock_file,
            &[
                (
                    &installed_pkg_id,
                    Some(PackageState::Installed),
                    Some(PackageState::Installed),
                ),
                (&next_pkg_id, None, None),
            ],
            |db| {
                let result = db.begin_install(&next_manifest).unwrap();
                assert!(matches!(
                    result,
                    BeginInstallResult::OtherVersionInstalled(version)
                        if version == Version::new(0, 1, 0)
                ));
            },
        );
    }

    #[test]
    fn cancel_install_removes_pending_entry() {
        let (_tempdir, app_dirs) = testing::make_app_dirs();
        let mut lock_file = DbLockFile::open(&app_dirs).unwrap();

        let manifest = testing::make_manifest("example-namespace", "example-font", "0.1.0");
        let pkg_id = manifest.metadata.id();

        with_db(&app_dirs, &mut lock_file, |db| {
            assert!(matches!(
                db.begin_install(&manifest).unwrap(),
                BeginInstallResult::CanInstall
            ));
        });
        assert_status_change(
            &app_dirs,
            &mut lock_file,
            &pkg_id,
            Some(PackageState::PendingInstall),
            None,
            |db| {
                db.cancel_install(&pkg_id).unwrap();
            },
        );
    }

    #[test]
    fn complete_uninstall_removes_last_version_entry() {
        let (_tempdir, app_dirs) = testing::make_app_dirs();
        let mut lock_file = DbLockFile::open(&app_dirs).unwrap();

        let manifest = testing::make_manifest("example-namespace", "example-font", "0.1.0");
        let pkg_id = manifest.metadata.id();

        with_db(&app_dirs, &mut lock_file, |db| {
            assert!(matches!(
                db.begin_install(&manifest).unwrap(),
                BeginInstallResult::CanInstall
            ));
            db.complete_install(&pkg_id).unwrap();
        });
        assert_status_change(
            &app_dirs,
            &mut lock_file,
            &pkg_id,
            Some(PackageState::Installed),
            None,
            |db| {
                assert!(matches!(
                    db.begin_uninstall(&pkg_id).unwrap(),
                    BeginUninstallResult::CanUninstall
                ));
                db.complete_uninstall(&pkg_id).unwrap();
            },
        );
    }

    #[test]
    fn begin_install_reports_pending_uninstall_versions() {
        let (_tempdir, app_dirs) = testing::make_app_dirs();
        let mut lock_file = DbLockFile::open(&app_dirs).unwrap();

        let manifest = testing::make_manifest("example-namespace", "example-font", "0.1.0");
        let pkg_id = manifest.metadata.id();

        with_db(&app_dirs, &mut lock_file, |db| {
            assert!(matches!(
                db.begin_install(&manifest).unwrap(),
                BeginInstallResult::CanInstall
            ));
            db.complete_install(&pkg_id).unwrap();
            assert!(matches!(
                db.begin_uninstall(&pkg_id).unwrap(),
                BeginUninstallResult::CanUninstall
            ));
        });
        assert_status_change(
            &app_dirs,
            &mut lock_file,
            &pkg_id,
            Some(PackageState::PendingUninstall),
            Some(PackageState::PendingUninstall),
            |db| {
                let result = db.begin_install(&manifest).unwrap();
                assert!(matches!(
                    result,
                    BeginInstallResult::PendingUninstallFound(ref versions)
                        if versions.contains(&Version::new(0, 1, 0).into())
                ));
            },
        );
    }

    #[test]
    fn begin_install_save_failure_rolls_back_pending_entry() {
        let (_tempdir, app_dirs) = testing::make_app_dirs();
        let mut lock_file = DbLockFile::open(&app_dirs).unwrap();

        let manifest = testing::make_manifest("example-namespace", "example-font", "0.1.0");
        let pkg_id = manifest.metadata.id();

        assert_status_change(&app_dirs, &mut lock_file, &pkg_id, None, None, |db| {
            db.simulate_save_failure = true;
            let err = db.begin_install(&manifest).unwrap_err();
            assert!(matches!(err, PackageDatabaseError::SimulatedSaveFailure));
        });
    }

    #[test]
    fn complete_install_save_failure_restores_pending_install_state() {
        let (_tempdir, app_dirs) = testing::make_app_dirs();
        let mut lock_file = DbLockFile::open(&app_dirs).unwrap();

        let manifest = testing::make_manifest("example-namespace", "example-font", "0.1.0");
        let pkg_id = manifest.metadata.id();

        with_db(&app_dirs, &mut lock_file, |db| {
            assert!(matches!(
                db.begin_install(&manifest).unwrap(),
                BeginInstallResult::CanInstall
            ));
        });
        assert_status_change(
            &app_dirs,
            &mut lock_file,
            &pkg_id,
            Some(PackageState::PendingInstall),
            Some(PackageState::PendingInstall),
            |db| {
                db.simulate_save_failure = true;
                let err = db.complete_install(&pkg_id).unwrap_err();
                assert!(matches!(err, PackageDatabaseError::SimulatedSaveFailure));
            },
        );
    }

    #[test]
    fn cancel_install_save_failure_restores_pending_install_state() {
        let (_tempdir, app_dirs) = testing::make_app_dirs();
        let mut lock_file = DbLockFile::open(&app_dirs).unwrap();

        let manifest = testing::make_manifest("example-namespace", "example-font", "0.1.0");
        let pkg_id = manifest.metadata.id();

        with_db(&app_dirs, &mut lock_file, |db| {
            assert!(matches!(
                db.begin_install(&manifest).unwrap(),
                BeginInstallResult::CanInstall
            ));
        });
        assert_status_change(
            &app_dirs,
            &mut lock_file,
            &pkg_id,
            Some(PackageState::PendingInstall),
            Some(PackageState::PendingInstall),
            |db| {
                db.simulate_save_failure = true;
                let err = db.cancel_install(&pkg_id).unwrap_err();
                assert!(matches!(err, PackageDatabaseError::SimulatedSaveFailure));
            },
        );
    }

    #[test]
    fn begin_uninstall_save_failure_restores_installed_state() {
        let (_tempdir, app_dirs) = testing::make_app_dirs();
        let mut lock_file = DbLockFile::open(&app_dirs).unwrap();

        let manifest = testing::make_manifest("example-namespace", "example-font", "0.1.0");
        let pkg_id = manifest.metadata.id();

        with_db(&app_dirs, &mut lock_file, |db| {
            assert!(matches!(
                db.begin_install(&manifest).unwrap(),
                BeginInstallResult::CanInstall
            ));
            db.complete_install(&pkg_id).unwrap();
        });
        assert_status_change(
            &app_dirs,
            &mut lock_file,
            &pkg_id,
            Some(PackageState::Installed),
            Some(PackageState::Installed),
            |db| {
                db.simulate_save_failure = true;
                let err = db.begin_uninstall(&pkg_id).unwrap_err();
                assert!(matches!(err, PackageDatabaseError::SimulatedSaveFailure));
            },
        );
    }

    #[test]
    fn complete_uninstall_save_failure_restores_pending_uninstall_state() {
        let (_tempdir, app_dirs) = testing::make_app_dirs();
        let mut lock_file = DbLockFile::open(&app_dirs).unwrap();

        let manifest = testing::make_manifest("example-namespace", "example-font", "0.1.0");
        let pkg_id = manifest.metadata.id();

        with_db(&app_dirs, &mut lock_file, |db| {
            assert!(matches!(
                db.begin_install(&manifest).unwrap(),
                BeginInstallResult::CanInstall
            ));
            db.complete_install(&pkg_id).unwrap();
            assert!(matches!(
                db.begin_uninstall(&pkg_id).unwrap(),
                BeginUninstallResult::CanUninstall
            ));
        });
        assert_status_change(
            &app_dirs,
            &mut lock_file,
            &pkg_id,
            Some(PackageState::PendingUninstall),
            Some(PackageState::PendingUninstall),
            |db| {
                db.simulate_save_failure = true;
                let err = db.complete_uninstall(&pkg_id).unwrap_err();
                assert!(matches!(err, PackageDatabaseError::SimulatedSaveFailure));
            },
        );
    }
}
