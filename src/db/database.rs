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
        persist::{
            self, PersistError, PersistedFontEntry, PersistedPackageDb, PersistedPackageEntry,
        },
    },
    package::{
        ActivationState, FontEntry, InstallationState, PackageId, PackageManifest, PackageName,
        PackageSpec,
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
        expected: InstallationState,
        actual: InstallationState,
    },
    #[snafu(display("package ID {pkg_id} is already installed"))]
    AlreadyInstalled { pkg_id: PackageId },
    #[cfg(test)]
    #[snafu(display("simulated failure to save package database"))]
    SimulatedSaveFailure,
}

#[derive(Debug)]
pub(crate) struct PackageDbEntry {
    pub(crate) installation_state: InstallationState,
    pub(crate) manifest: Arc<PackageManifest>,
}

impl From<&PersistedPackageEntry> for PackageDbEntry {
    fn from(entry: &PersistedPackageEntry) -> Self {
        Self {
            installation_state: entry.installation_state,
            manifest: Arc::clone(&entry.manifest),
        }
    }
}

impl From<&FontEntry> for PersistedFontEntry {
    fn from(value: &FontEntry) -> Self {
        Self {
            title: value.title().to_owned(),
            file_name: value.file_name().clone(),
        }
    }
}

#[derive(Debug)]
pub(crate) struct PackageDatabase<'lock> {
    persist_path: AbsolutePath,
    persist_db: PersistedPackageDb,
    _lock_file_guard: DbLockFileGuard<'lock>,
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

impl<'lock> PackageDatabase<'lock> {
    pub(crate) fn load(
        app_dirs: &AppDirs,
        lock_file_guard: DbLockFileGuard<'lock>,
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

    pub(crate) fn entries(&self) -> impl Iterator<Item = PackageDbEntry> {
        self.persist_db
            .packages
            .values()
            .flat_map(|version_map| version_map.versions.values())
            .map(Into::into)
    }

    pub(crate) fn entries_by_spec(
        &'lock self,
        pkg_spec: &PackageSpec,
    ) -> Box<dyn Iterator<Item = PackageDbEntry> + 'lock> {
        match pkg_spec {
            PackageSpec::Id(id) => Box::new(self.entry_by_id(id).into_iter()),
            PackageSpec::Name(name) => Box::new(self.entries_by_name(name)),
        }
    }

    pub(crate) fn entry_by_id(&self, pkg_id: &PackageId) -> Option<PackageDbEntry> {
        self.persist_db
            .packages
            .get(pkg_id.name())
            .and_then(|version_map| version_map.versions.get(pkg_id.version()))
            .map(Into::into)
    }

    pub(crate) fn entries_by_name(
        &'lock self,
        pkg_name: &PackageName,
    ) -> impl Iterator<Item = PackageDbEntry> + 'lock {
        self.persist_db
            .packages
            .get(pkg_name)
            .into_iter()
            .flat_map(|version_map| version_map.versions.values().map(Into::into))
    }

    pub(crate) fn check_installability(&self, manifest: &PackageManifest) -> Installability {
        let pkg_name = manifest.name.clone();
        let pkg_version = manifest.version.clone();
        let mut installed_other_versions = vec![];
        let mut incomplete_installs = vec![];
        let mut incomplete_uninstalls = vec![];
        for entry in self.entries_by_name(&pkg_name) {
            if entry.manifest.version == pkg_version {
                match entry.installation_state {
                    InstallationState::Installed => return Installability::AlreadyInstalled,
                    InstallationState::IncompleteInstall
                    | InstallationState::IncompleteUninstall => {}
                }
            } else {
                match entry.installation_state {
                    InstallationState::Installed => {
                        installed_other_versions.push(entry.manifest.id());
                    }
                    InstallationState::IncompleteInstall => {
                        incomplete_installs.push(entry.manifest.id());
                    }
                    InstallationState::IncompleteUninstall => {
                        incomplete_uninstalls.push(entry.manifest.id());
                    }
                }
            }
        }
        Installability::Installable {
            installed_other_versions,
            incomplete_installs,
            incomplete_uninstalls,
        }
    }

    pub(crate) fn transaction(&mut self) -> PackageDatabaseTransaction<'_, 'lock> {
        PackageDatabaseTransaction {
            persist_db: self.persist_db.clone(),
            #[cfg(test)]
            simulate_save_failure: self.simulate_save_failure,
            db: self,
        }
    }
}

#[derive(Debug)]
pub(crate) struct PackageDatabaseTransaction<'db, 'lock> {
    db: &'db mut PackageDatabase<'lock>,
    persist_db: PersistedPackageDb,
    #[cfg(test)]
    simulate_save_failure: bool,
}

impl PackageDatabaseTransaction<'_, '_> {
    pub(crate) fn commit(self) -> Result<(), PackageDatabaseError> {
        #[cfg(test)]
        {
            if self.simulate_save_failure {
                return Err(SimulatedSaveFailureSnafu.build());
            }
        }

        let persist_dir = self.db.persist_path.parent().unwrap();
        let mut temp_file = NamedTempFile::new_in(&persist_dir)
            .context(CreateTempFileSnafu { path: &persist_dir })?;
        persist::to_writer(&temp_file, &self.persist_db).context(SerializeDatabaseSnafu {
            path: temp_file.path(),
        })?;
        temp_file
            .as_file_mut()
            .sync_all()
            .context(PersistTempFileSnafu {
                path: &self.db.persist_path,
            })?;
        temp_file.persist(&self.db.persist_path).map_err(|source| {
            PersistTempFileSnafu {
                path: &self.db.persist_path,
            }
            .into_error(source.error)
        })?;

        self.db.persist_db = self.persist_db;
        Ok(())
    }

    fn entry_mut(&mut self, pkg_id: &PackageId) -> Option<&mut PersistedPackageEntry> {
        let version_map = self.persist_db.packages.get_mut(pkg_id.name())?;
        version_map.versions.get_mut(pkg_id.version())
    }

    fn insert_entry(&mut self, entry: PersistedPackageEntry) {
        let pkg_name = entry.manifest.name.clone();
        let pkg_version = entry.manifest.version.clone();
        self.persist_db
            .packages
            .entry(pkg_name)
            .or_default()
            .versions
            .insert(pkg_version, entry);
    }

    fn remove_entry(&mut self, pkg_id: &PackageId) -> Option<PersistedPackageEntry> {
        let version_map = self.persist_db.packages.get_mut(pkg_id.name())?;
        let entry = version_map.versions.remove(pkg_id.version())?;
        if version_map.versions.is_empty() {
            self.persist_db.packages.remove(pkg_id.name());
        }
        Some(entry)
    }

    pub(crate) fn begin_install(&mut self, manifest: Arc<PackageManifest>) {
        let entry = PersistedPackageEntry {
            installation_state: InstallationState::IncompleteInstall,
            activation_state: ActivationState::Inactive,
            manifest,
            font_entries: vec![],
        };
        self.insert_entry(entry);
    }

    pub(crate) fn complete_install(
        &mut self,
        pkg_id: &PackageId,
        font_entries: &[FontEntry],
    ) -> Result<(), PackageDatabaseError> {
        let entry = self
            .entry_mut(pkg_id)
            .context(EntryNotFoundSnafu { pkg_id })?;
        snafu::ensure!(
            entry.installation_state == InstallationState::IncompleteInstall,
            UnexpectedStateSnafu {
                pkg_id,
                expected: InstallationState::IncompleteInstall,
                actual: entry.installation_state,
            }
        );

        let db_entry = self.entry_mut(pkg_id).unwrap();
        db_entry.installation_state = InstallationState::Installed;
        db_entry.activation_state = ActivationState::Active;
        db_entry.font_entries = font_entries.iter().map(Into::into).collect();
        Ok(())
    }

    pub(crate) fn cancel_install(
        &mut self,
        pkg_id: &PackageId,
        cleanup_required: bool,
    ) -> Result<(), PackageDatabaseError> {
        let version_map = self
            .persist_db
            .packages
            .get_mut(pkg_id.name())
            .context(EntryNotFoundSnafu { pkg_id })?;
        let entry = version_map
            .versions
            .get_mut(pkg_id.version())
            .context(EntryNotFoundSnafu { pkg_id })?;
        snafu::ensure!(
            entry.installation_state == InstallationState::IncompleteInstall,
            UnexpectedStateSnafu {
                pkg_id,
                expected: InstallationState::IncompleteInstall,
                actual: entry.installation_state,
            }
        );
        if cleanup_required {
            entry.installation_state = InstallationState::IncompleteUninstall;
        } else {
            version_map.versions.remove(pkg_id.version()).unwrap();
            if version_map.versions.is_empty() {
                self.persist_db.packages.remove(pkg_id.name());
            }
        }
        Ok(())
    }

    pub(crate) fn begin_uninstall(
        &mut self,
        pkg_id: &PackageId,
    ) -> Result<(), PackageDatabaseError> {
        let entry = self
            .entry_mut(pkg_id)
            .context(EntryNotFoundSnafu { pkg_id })?;
        entry.installation_state = InstallationState::IncompleteUninstall;
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
            entry.installation_state == InstallationState::IncompleteUninstall,
            UnexpectedStateSnafu {
                pkg_id,
                expected: InstallationState::IncompleteUninstall,
                actual: entry.installation_state,
            }
        );
        self.remove_entry(pkg_id);
        Ok(())
    }
}

#[derive(Debug, Clone, derive_more::IsVariant)]
pub(crate) enum Installability {
    Installable {
        installed_other_versions: Vec<PackageId>,
        incomplete_installs: Vec<PackageId>,
        incomplete_uninstalls: Vec<PackageId>,
    },
    AlreadyInstalled,
}

#[cfg(test)]
mod tests {
    use std::assert_matches;

    use super::*;
    use crate::{db::DbLockFile, util::testing};

    fn get_entry_installation_state(
        db: &PackageDatabase<'_>,
        id: &PackageId,
    ) -> Option<InstallationState> {
        let entry = db.entry_by_id(id)?;
        assert_eq!(entry.manifest.id(), *id);
        Some(entry.installation_state)
    }

    fn get_persisted_state(
        app_dirs: &AppDirs,
        lock_file: &mut DbLockFile,
        id: &PackageId,
    ) -> Option<InstallationState> {
        let lock_file_guard = lock_file.try_acquire().unwrap();
        let db = PackageDatabase::load(app_dirs, lock_file_guard).unwrap();
        get_entry_installation_state(&db, id)
    }

    fn with_db<F, R>(app_dirs: &AppDirs, lock_file: &mut DbLockFile, f: F) -> R
    where
        F: FnOnce(&mut PackageDatabase<'_>) -> R,
    {
        let lock_file_guard = lock_file.try_acquire().unwrap();
        let mut db = PackageDatabase::load(app_dirs, lock_file_guard).unwrap();
        f(&mut db)
    }

    type StateCondition<'a> = (&'a PackageId, Option<InstallationState>);

    fn get_transaction_installation_state(
        tx: &PackageDatabaseTransaction<'_, '_>,
        id: &PackageId,
    ) -> Option<InstallationState> {
        let entry = tx
            .persist_db
            .packages
            .get(id.name())
            .and_then(|version_map| version_map.versions.get(id.version()))?;
        assert_eq!(entry.manifest.id(), *id);
        Some(entry.installation_state)
    }

    #[track_caller]
    fn assert_loaded_database_states(db: &PackageDatabase<'_>, conditions: &[StateCondition<'_>]) {
        for (id, expected_state) in conditions {
            assert_eq!(get_entry_installation_state(db, id), *expected_state);
        }
    }

    #[track_caller]
    fn assert_persisted_states(
        app_dirs: &AppDirs,
        lock_file: &mut DbLockFile,
        conditions: &[StateCondition<'_>],
    ) {
        for (id, expected_state) in conditions {
            assert_eq!(
                get_persisted_state(app_dirs, lock_file, id),
                *expected_state
            );
        }
    }

    #[track_caller]
    fn assert_transaction_states(
        tx: &PackageDatabaseTransaction<'_, '_>,
        conditions: &[StateCondition<'_>],
    ) {
        for (id, expected_state) in conditions {
            assert_eq!(get_transaction_installation_state(tx, id), *expected_state);
        }
    }

    #[track_caller]
    fn assert_transaction_stages_without_commit<F>(
        db: &mut PackageDatabase<'_>,
        before: &[StateCondition<'_>],
        staged: &[StateCondition<'_>],
        f: F,
    ) where
        F: FnOnce(&mut PackageDatabaseTransaction<'_, '_>),
    {
        assert_loaded_database_states(db, before);

        {
            let mut tx = db.transaction();
            f(&mut tx);
            assert_transaction_states(&tx, staged);
        }

        assert_loaded_database_states(db, before);
    }

    #[test]
    fn transaction_without_commit_discards_changes() {
        let (_tempdir, app_dirs) = testing::make_app_dirs();
        let mut lock_file = DbLockFile::open(&app_dirs).unwrap();

        let manifest = testing::make_manifest("example-font@0.1.0");
        let pkg_id = manifest.id();
        let before = [(&pkg_id, None)];
        let staged = [(&pkg_id, Some(InstallationState::IncompleteInstall))];

        assert_persisted_states(&app_dirs, &mut lock_file, &before);
        with_db(&app_dirs, &mut lock_file, |db| {
            assert_transaction_stages_without_commit(db, &before, &staged, |tx| {
                tx.begin_install(Arc::clone(&manifest));
            });
        });
        assert_persisted_states(&app_dirs, &mut lock_file, &before);
    }

    #[test]
    fn transaction_commit_failure_keeps_database_unchanged() {
        let (_tempdir, app_dirs) = testing::make_app_dirs();
        let mut lock_file = DbLockFile::open(&app_dirs).unwrap();

        let installed_manifest = testing::make_manifest("example-font@0.1.0");
        let incomplete_install_manifest = testing::make_manifest("example-font@0.2.0");
        let incomplete_uninstall_manifest = testing::make_manifest("example-font@0.3.0");
        let before = [
            (&installed_manifest.id(), Some(InstallationState::Installed)),
            (
                &incomplete_install_manifest.id(),
                Some(InstallationState::IncompleteInstall),
            ),
            (
                &incomplete_uninstall_manifest.id(),
                Some(InstallationState::IncompleteUninstall),
            ),
        ];
        let staged = [
            (
                &installed_manifest.id(),
                Some(InstallationState::IncompleteUninstall),
            ),
            (
                &incomplete_install_manifest.id(),
                Some(InstallationState::IncompleteUninstall),
            ),
            (
                &incomplete_uninstall_manifest.id(),
                Some(InstallationState::IncompleteUninstall),
            ),
        ];

        with_db(&app_dirs, &mut lock_file, |db| {
            testing::mark_as_installed(db, &installed_manifest);
            testing::mark_as_incomplete_install(db, &incomplete_install_manifest);
            testing::mark_as_incomplete_uninstall(db, &incomplete_uninstall_manifest);
        });

        assert_persisted_states(&app_dirs, &mut lock_file, &before);
        with_db(&app_dirs, &mut lock_file, |db| {
            assert_loaded_database_states(db, &before);

            db.simulate_save_failure = true;
            let mut tx = db.transaction();
            tx.begin_uninstall(&installed_manifest.id()).unwrap();
            tx.begin_uninstall(&incomplete_install_manifest.id())
                .unwrap();
            tx.begin_uninstall(&incomplete_uninstall_manifest.id())
                .unwrap();
            assert_transaction_states(&tx, &staged);

            let err = tx.commit().unwrap_err();
            assert_matches!(err, PackageDatabaseError::SimulatedSaveFailure);

            assert_loaded_database_states(db, &before);
        });
        assert_persisted_states(&app_dirs, &mut lock_file, &before);
    }

    #[test]
    fn transaction_commit_persists_multiple_install_changes_together() {
        let (_tempdir, app_dirs) = testing::make_app_dirs();
        let mut lock_file = DbLockFile::open(&app_dirs).unwrap();

        let installed_manifest = testing::make_manifest("example-font@0.1.0");
        let installed_pkg_id = installed_manifest.id();
        let manifest = testing::make_manifest("example-font@0.2.0");
        let pkg_id = manifest.id();

        let before = [
            (&installed_pkg_id, Some(InstallationState::Installed)),
            (&pkg_id, None),
        ];
        let staged = [
            (&installed_pkg_id, Some(InstallationState::Installed)),
            (&pkg_id, Some(InstallationState::Installed)),
        ];

        with_db(&app_dirs, &mut lock_file, |db| {
            testing::mark_as_installed(db, &installed_manifest);
        });

        assert_persisted_states(&app_dirs, &mut lock_file, &before);
        with_db(&app_dirs, &mut lock_file, |db| {
            assert_loaded_database_states(db, &before);

            let mut tx = db.transaction();
            tx.begin_install(Arc::clone(&manifest));
            tx.complete_install(&pkg_id, &[]).unwrap();
            assert_transaction_states(&tx, &staged);

            tx.commit().unwrap();
            assert_loaded_database_states(db, &staged);
        });
        assert_persisted_states(&app_dirs, &mut lock_file, &staged);
    }

    #[test]
    fn begin_install_keeps_same_version_incomplete_install_in_transaction() {
        let (_tempdir, app_dirs) = testing::make_app_dirs();
        let mut lock_file = DbLockFile::open(&app_dirs).unwrap();

        let manifest = testing::make_manifest("example-font@0.1.0");
        let pkg_id = manifest.id();
        let expected = [(&pkg_id, Some(InstallationState::IncompleteInstall))];

        with_db(&app_dirs, &mut lock_file, |db| {
            testing::mark_as_incomplete_install(db, &manifest);
        });

        assert_persisted_states(&app_dirs, &mut lock_file, &expected);
        with_db(&app_dirs, &mut lock_file, |db| {
            assert_transaction_stages_without_commit(db, &expected, &expected, |tx| {
                tx.begin_install(Arc::clone(&manifest));
            });
        });
        assert_persisted_states(&app_dirs, &mut lock_file, &expected);
    }

    #[test]
    fn complete_install_marks_target_installed_without_touching_other_versions_in_transaction() {
        let (_tempdir, app_dirs) = testing::make_app_dirs();
        let mut lock_file = DbLockFile::open(&app_dirs).unwrap();

        let installed_manifest = testing::make_manifest("example-font@0.1.0");
        let installed_pkg_id = installed_manifest.id();
        let manifest = testing::make_manifest("example-font@0.2.0");
        let pkg_id = manifest.id();

        let before = [
            (&installed_pkg_id, Some(InstallationState::Installed)),
            (&pkg_id, Some(InstallationState::IncompleteInstall)),
        ];
        let staged = [
            (&installed_pkg_id, Some(InstallationState::Installed)),
            (&pkg_id, Some(InstallationState::Installed)),
        ];

        with_db(&app_dirs, &mut lock_file, |db| {
            testing::mark_as_installed(db, &installed_manifest);
            let mut tx = db.transaction();
            tx.begin_install(Arc::clone(&manifest));
            tx.commit().unwrap();
        });

        assert_persisted_states(&app_dirs, &mut lock_file, &before);
        with_db(&app_dirs, &mut lock_file, |db| {
            assert_transaction_stages_without_commit(db, &before, &staged, |tx| {
                tx.complete_install(&pkg_id, &[]).unwrap();
            });
        });
        assert_persisted_states(&app_dirs, &mut lock_file, &before);
    }

    #[test]
    fn cancel_install_removes_incomplete_entry_from_transaction() {
        let (_tempdir, app_dirs) = testing::make_app_dirs();
        let mut lock_file = DbLockFile::open(&app_dirs).unwrap();

        let manifest = testing::make_manifest("example-font@0.1.0");
        let pkg_id = manifest.id();
        let before = [(&pkg_id, Some(InstallationState::IncompleteInstall))];
        let staged = [(&pkg_id, None)];

        with_db(&app_dirs, &mut lock_file, |db| {
            testing::mark_as_incomplete_install(db, &manifest);
        });

        assert_persisted_states(&app_dirs, &mut lock_file, &before);
        with_db(&app_dirs, &mut lock_file, |db| {
            assert_transaction_stages_without_commit(db, &before, &staged, |tx| {
                tx.cancel_install(&pkg_id, false).unwrap();
            });
        });
        assert_persisted_states(&app_dirs, &mut lock_file, &before);
    }

    #[test]
    fn cancel_install_marks_entry_incomplete_uninstall_in_transaction_when_cleanup_failed() {
        let (_tempdir, app_dirs) = testing::make_app_dirs();
        let mut lock_file = DbLockFile::open(&app_dirs).unwrap();

        let manifest = testing::make_manifest("example-font@0.1.0");
        let pkg_id = manifest.id();
        let before = [(&pkg_id, Some(InstallationState::IncompleteInstall))];
        let staged = [(&pkg_id, Some(InstallationState::IncompleteUninstall))];

        with_db(&app_dirs, &mut lock_file, |db| {
            testing::mark_as_incomplete_install(db, &manifest);
        });

        assert_persisted_states(&app_dirs, &mut lock_file, &before);
        with_db(&app_dirs, &mut lock_file, |db| {
            assert_transaction_stages_without_commit(db, &before, &staged, |tx| {
                tx.cancel_install(&pkg_id, true).unwrap();
            });
        });
        assert_persisted_states(&app_dirs, &mut lock_file, &before);
    }

    #[test]
    fn complete_uninstall_removes_last_version_entry_from_transaction() {
        let (_tempdir, app_dirs) = testing::make_app_dirs();
        let mut lock_file = DbLockFile::open(&app_dirs).unwrap();

        let manifest = testing::make_manifest("example-font@0.1.0");
        let pkg_id = manifest.id();
        let before = [(&pkg_id, Some(InstallationState::IncompleteUninstall))];
        let staged = [(&pkg_id, None)];

        with_db(&app_dirs, &mut lock_file, |db| {
            testing::mark_as_incomplete_uninstall(db, &manifest);
        });

        assert_persisted_states(&app_dirs, &mut lock_file, &before);
        with_db(&app_dirs, &mut lock_file, |db| {
            assert_transaction_stages_without_commit(db, &before, &staged, |tx| {
                tx.complete_uninstall(&pkg_id).unwrap();
            });
        });
        assert_persisted_states(&app_dirs, &mut lock_file, &before);
    }
}
