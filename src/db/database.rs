use std::{
    fs::File,
    io::{self, BufReader},
    path::PathBuf,
    sync::Arc,
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
    #[snafu(display("package ID {pkg_id} is already installed"))]
    AlreadyInstalled { pkg_id: PackageId },
    #[snafu(display(
        "package {pkg_name} version {installed_version} is already installed, cannot install version {attempted_version}",
    ))]
    OtherVersionInstalled {
        pkg_name: PackageQualifiedName,
        installed_version: PackageVersion,
        attempted_version: PackageVersion,
    },
    #[cfg(test)]
    #[snafu(display("simulated failure to save package database"))]
    SimulatedSaveFailure,
}

#[derive(Debug)]
pub(crate) struct PackageDatabase<'a> {
    persist_path: AbsolutePath,
    persist_db: PersistedPackageDb,
    saved_persist_db: PersistedPackageDb,
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
            persist_db: persist_db.clone(),
            saved_persist_db: persist_db,
            _lock_file_guard: lock_file_guard,
            #[cfg(test)]
            simulate_save_failure: false,
        })
    }

    #[cfg(test)]
    pub(crate) fn reload(&mut self) -> Result<(), PackageDatabaseError> {
        self.persist_db = load(&self.persist_path)?;
        self.saved_persist_db = self.persist_db.clone();
        Ok(())
    }

    fn save(&mut self) -> Result<(), PackageDatabaseError> {
        let res = (|| {
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
        })();
        if res.is_err() {
            self.persist_db = self.saved_persist_db.clone();
        } else {
            self.saved_persist_db = self.persist_db.clone();
        }
        res
    }

    pub(crate) fn entries(&self) -> impl Iterator<Item = (PackageState, Arc<PackageManifest>)> {
        self.persist_db
            .packages
            .values()
            .flat_map(|version_map| version_map.versions.values())
            .map(|entry| (entry.state, Arc::clone(&entry.manifest)))
    }

    pub(crate) fn entries_by_spec(
        &'a self,
        pkg_spec: &PackageSpec,
    ) -> Box<dyn Iterator<Item = (PackageState, Arc<PackageManifest>)> + 'a> {
        match pkg_spec {
            PackageSpec::Id(id) => Box::new(self.entry_by_id(id).into_iter()),
            PackageSpec::QualifiedName(qualified_name) => {
                Box::new(self.entries_by_qualified_name(qualified_name))
            }
            PackageSpec::Name(name) => Box::new(self.entries_by_name(name)),
        }
    }

    pub(crate) fn entry_by_id(
        &self,
        pkg_id: &PackageId,
    ) -> Option<(PackageState, Arc<PackageManifest>)> {
        self.persist_db
            .packages
            .get(pkg_id.qualified_name())
            .and_then(|version_map| version_map.versions.get(pkg_id.version()))
            .map(|entry| (entry.state, Arc::clone(&entry.manifest)))
    }

    pub(crate) fn entries_by_qualified_name(
        &'a self,
        pkg_name: &PackageQualifiedName,
    ) -> impl Iterator<Item = (PackageState, Arc<PackageManifest>)> + 'a {
        self.persist_db
            .packages
            .get(pkg_name)
            .into_iter()
            .flat_map(|version_map| {
                version_map
                    .versions
                    .values()
                    .map(|packages| (packages.state, Arc::clone(&packages.manifest)))
            })
    }

    pub(crate) fn entries_by_name(
        &'a self,
        pkg_name: &PackageName,
    ) -> impl Iterator<Item = (PackageState, Arc<PackageManifest>)> + 'a {
        let pkg_name = pkg_name.clone();
        self.persist_db
            .packages
            .iter()
            .filter(move |(qualified_name, _)| qualified_name.name() == pkg_name)
            .flat_map(|(_, version_map)| {
                version_map
                    .versions
                    .values()
                    .map(|packages| (packages.state, Arc::clone(&packages.manifest)))
            })
    }

    fn entry_mut(&mut self, pkg_id: &PackageId) -> Option<&mut PersistedPackageEntry> {
        let version_map = self.persist_db.packages.get_mut(pkg_id.qualified_name())?;
        version_map.versions.get_mut(pkg_id.version())
    }

    fn insert_entry(&mut self, state: PackageState, manifest: Arc<PackageManifest>) {
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

    pub(crate) fn check_installability(&self, manifest: &PackageManifest) -> Installability {
        let pkg_name = manifest.metadata.qualified_name.clone();
        let pkg_version = manifest.metadata.version.clone();
        for (state, m) in self.entries_by_qualified_name(&pkg_name) {
            match state {
                PackageState::Installed => {
                    let result = if m.metadata.version == pkg_version {
                        Installability::AlreadyInstalled
                    } else {
                        Installability::OtherVersionInstalled(m.metadata.version.clone())
                    };
                    return result;
                }
                PackageState::PendingInstall | PackageState::PendingUninstall => {}
            }
        }
        Installability::Installable
    }

    pub(crate) fn apply_install_transaction(
        &mut self,
        manifest: &Arc<PackageManifest>,
    ) -> Result<InstallTransactionResult, PackageDatabaseError> {
        let result = self.check_installability(manifest);
        let pkg_id = manifest.metadata.id();
        match result {
            Installability::Installable => {}
            Installability::AlreadyInstalled => {
                return Err(AlreadyInstalledSnafu { pkg_id }.build());
            }
            Installability::OtherVersionInstalled(installed_version) => {
                OtherVersionInstalledSnafu {
                    pkg_name: pkg_id.qualified_name(),
                    installed_version,
                    attempted_version: pkg_id.version(),
                }
                .fail()?;
            }
        }
        let mut pending_uninstalls = vec![];
        if let Some(version_map) = self.persist_db.packages.get_mut(pkg_id.qualified_name()) {
            for (version, entry) in &mut version_map.versions {
                assert!(matches!(
                    entry.state,
                    PackageState::PendingInstall | PackageState::PendingUninstall
                ));
                if version == pkg_id.version() {
                    continue;
                }
                entry.state = PackageState::PendingUninstall;
                pending_uninstalls.push(entry.manifest.metadata.id());
            }
        }
        self.insert_entry(PackageState::PendingInstall, Arc::clone(manifest));
        self.save()?;
        Ok(InstallTransactionResult { pending_uninstalls })
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
        self.save()?;
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
        version_map.versions.remove(pkg_id.version()).unwrap();
        if version_map.versions.is_empty() {
            self.persist_db.packages.remove(pkg_id.qualified_name());
        }
        self.save()?;
        Ok(())
    }

    pub(crate) fn apply_uninstall_transaction(
        &mut self,
        pkg_id: &PackageId,
    ) -> Result<(), PackageDatabaseError> {
        let entry = self
            .entry_mut(pkg_id)
            .context(EntryNotFoundSnafu { pkg_id })?;
        entry.state = PackageState::PendingUninstall;
        self.save()?;
        Ok(())
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
        self.remove_entry(pkg_id);
        self.save()?;
        Ok(())
    }
}

#[derive(Debug, Clone, derive_more::IsVariant)]
pub(crate) enum Installability {
    Installable,
    AlreadyInstalled,
    OtherVersionInstalled(PackageVersion),
}

#[derive(Debug)]
pub(crate) struct InstallTransactionResult {
    pub(crate) pending_uninstalls: Vec<PackageId>,
}

#[cfg(test)]
mod tests {
    use std::str::FromStr as _;

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

        let manifest = testing::make_manifest("example-namespace/example-font@0.1.0");
        let pkg_id = manifest.metadata.id();

        assert_status_change(
            &app_dirs,
            &mut lock_file,
            &pkg_id,
            None,
            Some(PackageState::Installed),
            |db| {
                db.apply_install_transaction(&manifest).unwrap();
                db.complete_install(&pkg_id).unwrap();
            },
        );
    }

    #[test]
    fn apply_install_transaction_keeps_same_version_pending_install() {
        let (_tempdir, app_dirs) = testing::make_app_dirs();
        let mut lock_file = DbLockFile::open(&app_dirs).unwrap();

        let manifest = testing::make_manifest("example-namespace/example-font@0.1.0");
        let pkg_id = manifest.metadata.id();

        with_db(&app_dirs, &mut lock_file, |db| {
            testing::mark_as_pending_installed(db, &manifest);
        });
        assert_status_change(
            &app_dirs,
            &mut lock_file,
            &pkg_id,
            Some(PackageState::PendingInstall),
            Some(PackageState::PendingInstall),
            |db| {
                let res = db.apply_install_transaction(&manifest).unwrap();
                assert!(res.pending_uninstalls.is_empty());
            },
        );
    }

    #[test]
    fn apply_install_transaction_moves_other_pending_install_to_pending_uninstall() {
        let (_tempdir, app_dirs) = testing::make_app_dirs();
        let mut lock_file = DbLockFile::open(&app_dirs).unwrap();

        let old_manifest = testing::make_manifest("example-namespace/example-font@0.1.0");
        let old_pkg_id = old_manifest.metadata.id();
        let new_manifest = testing::make_manifest("example-namespace/example-font@0.2.0");
        let new_pkg_id = new_manifest.metadata.id();

        with_db(&app_dirs, &mut lock_file, |db| {
            testing::mark_as_pending_installed(db, &old_manifest);
        });
        assert_statuses_change(
            &app_dirs,
            &mut lock_file,
            &[
                (
                    &old_pkg_id,
                    Some(PackageState::PendingInstall),
                    Some(PackageState::PendingUninstall),
                ),
                (&new_pkg_id, None, Some(PackageState::PendingInstall)),
            ],
            |db| {
                let res = db.apply_install_transaction(&new_manifest).unwrap();
                assert_eq!(res.pending_uninstalls, vec![old_pkg_id.clone()]);
            },
        );
    }

    #[test]
    fn apply_install_transaction_reports_other_installed_version() {
        let (_tempdir, app_dirs) = testing::make_app_dirs();
        let mut lock_file = DbLockFile::open(&app_dirs).unwrap();

        let installed_manifest = testing::make_manifest("example-namespace/example-font@0.1.0");
        let installed_pkg_id = installed_manifest.metadata.id();
        let next_manifest = testing::make_manifest("example-namespace/example-font@0.2.0");
        let next_pkg_id = next_manifest.metadata.id();

        with_db(&app_dirs, &mut lock_file, |db| {
            testing::mark_as_installed(db, &installed_manifest);
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
                let err = db.apply_install_transaction(&next_manifest).unwrap_err();
                assert!(matches!(
                    err,
                    PackageDatabaseError::OtherVersionInstalled { installed_version, ..}
                        if installed_version == Version::new(0, 1, 0)
                ));
            },
        );
    }

    #[test]
    fn apply_install_transaction_save_failure_restores_other_pending_install_state() {
        let (_tempdir, app_dirs) = testing::make_app_dirs();
        let mut lock_file = DbLockFile::open(&app_dirs).unwrap();

        let old_manifest = testing::make_manifest("example-namespace/example-font@0.1.0");
        let old_pkg_id = old_manifest.metadata.id();
        let new_manifest = testing::make_manifest("example-namespace/example-font@0.2.0");
        let new_pkg_id = new_manifest.metadata.id();

        with_db(&app_dirs, &mut lock_file, |db| {
            testing::mark_as_pending_installed(db, &old_manifest);
        });
        assert_statuses_change(
            &app_dirs,
            &mut lock_file,
            &[
                (
                    &old_pkg_id,
                    Some(PackageState::PendingInstall),
                    Some(PackageState::PendingInstall),
                ),
                (&new_pkg_id, None, None),
            ],
            |db| {
                db.simulate_save_failure = true;
                let err = db.apply_install_transaction(&new_manifest).unwrap_err();
                assert!(matches!(err, PackageDatabaseError::SimulatedSaveFailure));
            },
        );
    }

    #[test]
    fn cancel_install_removes_pending_entry() {
        let (_tempdir, app_dirs) = testing::make_app_dirs();
        let mut lock_file = DbLockFile::open(&app_dirs).unwrap();

        let manifest = testing::make_manifest("example-namespace/example-font@0.1.0");
        let pkg_id = manifest.metadata.id();

        with_db(&app_dirs, &mut lock_file, |db| {
            testing::mark_as_pending_installed(db, &manifest);
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

        let manifest = testing::make_manifest("example-namespace/example-font@0.1.0");
        let pkg_id = manifest.metadata.id();

        with_db(&app_dirs, &mut lock_file, |db| {
            testing::mark_as_installed(db, &manifest);
        });
        assert_status_change(
            &app_dirs,
            &mut lock_file,
            &pkg_id,
            Some(PackageState::Installed),
            None,
            |db| {
                db.apply_uninstall_transaction(&pkg_id).unwrap();
                db.complete_uninstall(&pkg_id).unwrap();
            },
        );
    }

    #[test]
    fn apply_install_transaction_replaces_same_version_pending_uninstall() {
        let (_tempdir, app_dirs) = testing::make_app_dirs();
        let mut lock_file = DbLockFile::open(&app_dirs).unwrap();

        let manifest = testing::make_manifest("example-namespace/example-font@0.1.0");
        let pkg_id = manifest.metadata.id();

        with_db(&app_dirs, &mut lock_file, |db| {
            testing::mark_as_pending_uninstalled(db, &manifest);
        });
        assert_status_change(
            &app_dirs,
            &mut lock_file,
            &pkg_id,
            Some(PackageState::PendingUninstall),
            Some(PackageState::PendingInstall),
            |db| {
                let res = db.apply_install_transaction(&manifest).unwrap();
                assert!(res.pending_uninstalls.is_empty());
            },
        );
    }

    #[test]
    fn apply_install_transaction_save_failure_rolls_back_pending_entry() {
        let (_tempdir, app_dirs) = testing::make_app_dirs();
        let mut lock_file = DbLockFile::open(&app_dirs).unwrap();

        let manifest = testing::make_manifest("example-namespace/example-font@0.1.0");
        let pkg_id = manifest.metadata.id();

        assert_status_change(&app_dirs, &mut lock_file, &pkg_id, None, None, |db| {
            db.simulate_save_failure = true;
            let err = db.apply_install_transaction(&manifest).unwrap_err();
            assert!(matches!(err, PackageDatabaseError::SimulatedSaveFailure));
        });
    }

    #[test]
    fn complete_install_save_failure_restores_pending_install_state() {
        let (_tempdir, app_dirs) = testing::make_app_dirs();
        let mut lock_file = DbLockFile::open(&app_dirs).unwrap();

        let manifest = testing::make_manifest("example-namespace/example-font@0.1.0");
        let pkg_id = manifest.metadata.id();

        with_db(&app_dirs, &mut lock_file, |db| {
            testing::mark_as_pending_installed(db, &manifest);
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

        let manifest = testing::make_manifest("example-namespace/example-font@0.1.0");
        let pkg_id = manifest.metadata.id();

        with_db(&app_dirs, &mut lock_file, |db| {
            testing::mark_as_pending_installed(db, &manifest);
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
    fn apply_uninstall_transaction_reports_missing_entry() {
        let (_tempdir, app_dirs) = testing::make_app_dirs();
        let mut lock_file = DbLockFile::open(&app_dirs).unwrap();

        let pkg_id = PackageId::from_str("example-namespace/example-font@0.1.0").unwrap();

        assert_status_change(&app_dirs, &mut lock_file, &pkg_id, None, None, |db| {
            let err = db.apply_uninstall_transaction(&pkg_id).unwrap_err();
            assert!(matches!(
                err,
                PackageDatabaseError::EntryNotFound { pkg_id: missing_pkg_id }
                    if missing_pkg_id == pkg_id
            ));
        });
    }

    #[test]
    fn apply_uninstall_transaction_save_failure_restores_installed_state() {
        let (_tempdir, app_dirs) = testing::make_app_dirs();
        let mut lock_file = DbLockFile::open(&app_dirs).unwrap();

        let manifest = testing::make_manifest("example-namespace/example-font@0.1.0");
        let pkg_id = manifest.metadata.id();

        with_db(&app_dirs, &mut lock_file, |db| {
            testing::mark_as_installed(db, &manifest);
        });
        assert_status_change(
            &app_dirs,
            &mut lock_file,
            &pkg_id,
            Some(PackageState::Installed),
            Some(PackageState::Installed),
            |db| {
                db.simulate_save_failure = true;
                let err = db.apply_uninstall_transaction(&pkg_id).unwrap_err();
                assert!(matches!(err, PackageDatabaseError::SimulatedSaveFailure));
            },
        );
    }

    #[test]
    fn complete_uninstall_save_failure_restores_pending_uninstall_state() {
        let (_tempdir, app_dirs) = testing::make_app_dirs();
        let mut lock_file = DbLockFile::open(&app_dirs).unwrap();

        let manifest = testing::make_manifest("example-namespace/example-font@0.1.0");
        let pkg_id = manifest.metadata.id();

        with_db(&app_dirs, &mut lock_file, |db| {
            testing::mark_as_pending_uninstalled(db, &manifest);
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
