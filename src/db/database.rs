use std::{
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
        ActivationState, FontEntry, InstallationState, PackageDefinition, PackageId, PackageName,
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
        "database entry for package ID {pkg_id} is in unexpected installation state: expected {expected}, actual {actual}"
    ))]
    UnexpectedInstallationState {
        pkg_id: PackageId,
        expected: InstallationState,
        actual: InstallationState,
    },
    #[snafu(display(
        "database entry for package ID {pkg_id} is in unexpected activation state: expected {expected}, actual {actual}"
    ))]
    UnexpectedActivationState {
        pkg_id: PackageId,
        expected: ActivationState,
        actual: ActivationState,
    },
    #[snafu(display("package ID {pkg_id} is already installed"))]
    AlreadyInstalled { pkg_id: PackageId },
    #[cfg(test)]
    #[snafu(display("simulated failure to save package database"))]
    SimulatedSaveFailure,
}

#[derive(Debug)]
pub(crate) struct PackageDbEntry<'a> {
    id: PackageId,
    data: &'a PersistedPackageEntry,
}

impl<'a> PackageDbEntry<'a> {
    fn new(id: PackageId, data: &'a PersistedPackageEntry) -> Self {
        Self { id, data }
    }

    pub(crate) fn id(&self) -> &PackageId {
        &self.id
    }

    pub(crate) fn installation_state(&self) -> InstallationState {
        self.data.installation_state()
    }

    pub(crate) fn activation_state(&self) -> ActivationState {
        self.data.activation_state()
    }

    pub(crate) fn make_definition(&self) -> PackageDefinition {
        self.data.make_definition(self.id.clone())
    }

    pub(crate) fn font_entries(&self) -> impl Iterator<Item = FontEntry> + 'a {
        self.data.font_entries()
    }
}

#[derive(Debug)]
pub(crate) struct PackageDatabase<'lock> {
    persist_path: AbsolutePath,
    persisted_state: PersistedPackageDb,
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
            persisted_state: persist_db,
            _lock_file_guard: lock_file_guard,
            #[cfg(test)]
            simulate_save_failure: false,
        })
    }

    pub(crate) fn all_entries(&self) -> impl Iterator<Item = PackageDbEntry<'_>> {
        self.persisted_state
            .all_entries()
            .map(|(pkg_name, pkg_version, entry)| {
                let pkg_id = PackageId::new(pkg_name.clone(), pkg_version.clone());
                PackageDbEntry::new(pkg_id, entry)
            })
    }

    pub(crate) fn entries_by_spec<'a>(
        &'a self,
        pkg_spec: &'a PackageSpec,
    ) -> Box<dyn Iterator<Item = PackageDbEntry<'a>> + 'a> {
        match pkg_spec {
            PackageSpec::Id(id) => Box::new(self.entry_by_id(id).into_iter()),
            PackageSpec::Name(name) => Box::new(self.entries_by_name(name)),
        }
    }

    pub(crate) fn entry_by_id<'a>(&'a self, pkg_id: &PackageId) -> Option<PackageDbEntry<'a>> {
        self.persisted_state
            .entry(pkg_id)
            .map(|entry| PackageDbEntry::new(pkg_id.clone(), entry))
    }

    pub(crate) fn entries_by_name<'a>(
        &'a self,
        pkg_name: &PackageName,
    ) -> impl Iterator<Item = PackageDbEntry<'a>> {
        self.persisted_state
            .entries_by_name(pkg_name)
            .map(|(pkg_version, entry)| {
                let pkg_id = PackageId::new(pkg_name.clone(), pkg_version.clone());
                PackageDbEntry::new(pkg_id, entry)
            })
    }

    pub(crate) fn check_installability(&self, pkg_id: &PackageId) -> Installability {
        if let Some(entry) = self.persisted_state.entry(pkg_id)
            && entry.installation_state().is_installed()
        {
            return Installability::AlreadyInstalled;
        }
        Installability::Installable
    }

    pub(crate) fn check_activatability(&self, pkg_id: &PackageId) -> Activatability {
        let pkg_name = pkg_id.name();
        let pkg_version = pkg_id.version();
        let mut active_other_versions = vec![];
        let mut incomplete_activations = vec![];
        let mut incomplete_deactivations = vec![];
        for (entry_version, entry) in self.persisted_state.entries_by_name(pkg_name) {
            if entry_version == pkg_version {
                match entry.activation_state() {
                    ActivationState::Active => return Activatability::AlreadyActive,
                    ActivationState::Inactive
                    | ActivationState::IncompleteActivation
                    | ActivationState::IncompleteDeactivation => {}
                }
            } else {
                let entry_pkg_id = PackageId::new(pkg_name.clone(), entry_version.clone());
                match entry.activation_state() {
                    ActivationState::Active => {
                        active_other_versions.push(entry_pkg_id);
                    }
                    ActivationState::Inactive => {}
                    ActivationState::IncompleteActivation => {
                        incomplete_activations.push(entry_pkg_id);
                    }
                    ActivationState::IncompleteDeactivation => {
                        incomplete_deactivations.push(entry_pkg_id);
                    }
                }
            }
        }
        Activatability::Activatable {
            active_other_versions,
            incomplete_activations,
            incomplete_deactivations,
        }
    }

    pub(crate) fn transaction(&mut self) -> PackageDatabaseTransaction<'_, 'lock> {
        PackageDatabaseTransaction {
            working_state: self.persisted_state.clone(),
            #[cfg(test)]
            simulate_save_failure: self.simulate_save_failure,
            target: self,
        }
    }
}

#[derive(Debug)]
pub(crate) struct PackageDatabaseTransaction<'db, 'lock> {
    target: &'db mut PackageDatabase<'lock>,
    working_state: PersistedPackageDb,
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

        let persist_dir = self.target.persist_path.parent().unwrap();
        let mut temp_file = NamedTempFile::new_in(&persist_dir)
            .context(CreateTempFileSnafu { path: &persist_dir })?;
        persist::to_writer(&temp_file, &self.working_state).context(SerializeDatabaseSnafu {
            path: temp_file.path(),
        })?;
        temp_file
            .as_file_mut()
            .sync_all()
            .context(PersistTempFileSnafu {
                path: &self.target.persist_path,
            })?;
        temp_file
            .persist(&self.target.persist_path)
            .map_err(|source| {
                PersistTempFileSnafu {
                    path: &self.target.persist_path,
                }
                .into_error(source.error)
            })?;

        self.target.persisted_state = self.working_state;
        Ok(())
    }

    fn check_installation_state(
        &self,
        pkg_id: &PackageId,
        expected: InstallationState,
    ) -> Result<(), PackageDatabaseError> {
        let entry = self
            .working_state
            .entry(pkg_id)
            .context(EntryNotFoundSnafu { pkg_id })?;
        let actual = entry.installation_state();
        snafu::ensure!(
            actual == expected,
            UnexpectedInstallationStateSnafu {
                pkg_id,
                expected,
                actual,
            }
        );
        Ok(())
    }

    fn check_activation_state(
        &self,
        pkg_id: &PackageId,
        expected: ActivationState,
    ) -> Result<(), PackageDatabaseError> {
        let entry = self
            .working_state
            .entry(pkg_id)
            .context(EntryNotFoundSnafu { pkg_id })?;
        let actual = entry.activation_state();
        snafu::ensure!(
            actual == expected,
            UnexpectedActivationStateSnafu {
                pkg_id,
                expected,
                actual,
            }
        );
        Ok(())
    }

    fn update_installation_state(
        &mut self,
        pkg_id: &PackageId,
        new_state: InstallationState,
    ) -> Result<(), PackageDatabaseError> {
        let entry = self
            .working_state
            .entry_mut(pkg_id)
            .context(EntryNotFoundSnafu { pkg_id })?;
        entry.set_installation_state(new_state);
        Ok(())
    }

    fn update_activation_state(
        &mut self,
        pkg_id: &PackageId,
        new_state: ActivationState,
    ) -> Result<(), PackageDatabaseError> {
        let entry = self
            .working_state
            .entry_mut(pkg_id)
            .context(EntryNotFoundSnafu { pkg_id })?;
        entry.set_activation_state(new_state);
        Ok(())
    }

    pub(crate) fn begin_install(&mut self, pkg: &PackageDefinition) {
        let entry = PersistedPackageEntry::new(
            InstallationState::IncompleteInstall,
            ActivationState::Inactive,
            pkg,
        );
        self.working_state.insert_entry(&pkg.id, entry);
    }

    pub(crate) fn complete_install(
        &mut self,
        pkg_id: &PackageId,
        font_entries: &[FontEntry],
    ) -> Result<(), PackageDatabaseError> {
        self.check_installation_state(pkg_id, InstallationState::IncompleteInstall)?;

        let db_entry = self.working_state.entry_mut(pkg_id).unwrap();
        db_entry.set_installation_state(InstallationState::Installed);
        db_entry.set_font_entries(font_entries);
        Ok(())
    }

    pub(crate) fn cancel_install(
        &mut self,
        pkg_id: &PackageId,
        cleanup_required: bool,
    ) -> Result<(), PackageDatabaseError> {
        self.check_installation_state(pkg_id, InstallationState::IncompleteInstall)?;
        if cleanup_required {
            self.update_installation_state(pkg_id, InstallationState::IncompleteUninstall)?;
        } else {
            self.working_state.remove_entry(pkg_id);
        }
        Ok(())
    }

    pub(crate) fn begin_uninstall(
        &mut self,
        pkg_id: &PackageId,
    ) -> Result<(), PackageDatabaseError> {
        self.check_activation_state(pkg_id, ActivationState::Inactive)?;
        self.update_installation_state(pkg_id, InstallationState::IncompleteUninstall)?;
        Ok(())
    }

    pub(crate) fn complete_uninstall(
        &mut self,
        pkg_id: &PackageId,
    ) -> Result<(), PackageDatabaseError> {
        self.check_installation_state(pkg_id, InstallationState::IncompleteUninstall)?;
        self.working_state.remove_entry(pkg_id);
        Ok(())
    }

    pub(crate) fn begin_activate(
        &mut self,
        pkg_id: &PackageId,
    ) -> Result<(), PackageDatabaseError> {
        self.check_installation_state(pkg_id, InstallationState::Installed)?;
        self.update_activation_state(pkg_id, ActivationState::IncompleteActivation)?;
        Ok(())
    }

    pub(crate) fn complete_activate(
        &mut self,
        pkg_id: &PackageId,
    ) -> Result<(), PackageDatabaseError> {
        self.check_activation_state(pkg_id, ActivationState::IncompleteActivation)?;
        self.update_activation_state(pkg_id, ActivationState::Active)?;
        Ok(())
    }

    pub(crate) fn cancel_activate(
        &mut self,
        pkg_id: &PackageId,
        cleanup_required: bool,
    ) -> Result<(), PackageDatabaseError> {
        self.check_activation_state(pkg_id, ActivationState::IncompleteActivation)?;
        if cleanup_required {
            self.update_activation_state(pkg_id, ActivationState::IncompleteDeactivation)?;
        } else {
            self.update_activation_state(pkg_id, ActivationState::Inactive)?;
        }
        Ok(())
    }

    pub(crate) fn begin_deactivate(
        &mut self,
        pkg_id: &PackageId,
    ) -> Result<(), PackageDatabaseError> {
        self.update_activation_state(pkg_id, ActivationState::IncompleteDeactivation)?;
        Ok(())
    }

    pub(crate) fn complete_deactivate(
        &mut self,
        pkg_id: &PackageId,
    ) -> Result<(), PackageDatabaseError> {
        self.check_activation_state(pkg_id, ActivationState::IncompleteDeactivation)?;
        self.update_activation_state(pkg_id, ActivationState::Inactive)?;
        Ok(())
    }
}

#[derive(Debug, Clone, derive_more::IsVariant)]
pub(crate) enum Installability {
    Installable,
    AlreadyInstalled,
}

#[derive(Debug, Clone, derive_more::IsVariant)]
pub(crate) enum Activatability {
    Activatable {
        active_other_versions: Vec<PackageId>,
        incomplete_activations: Vec<PackageId>,
        incomplete_deactivations: Vec<PackageId>,
    },
    AlreadyActive,
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
        assert_eq!(entry.id(), id);
        Some(entry.installation_state())
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
        pkg_id: &PackageId,
    ) -> Option<InstallationState> {
        let entry = tx.working_state.entry(pkg_id)?;
        Some(entry.installation_state())
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

        let pkg = testing::make_package_definition("example-font@0.1.0");
        let before = [(&pkg.id, None)];
        let staged = [(&pkg.id, Some(InstallationState::IncompleteInstall))];

        assert_persisted_states(&app_dirs, &mut lock_file, &before);
        with_db(&app_dirs, &mut lock_file, |db| {
            assert_transaction_stages_without_commit(db, &before, &staged, |tx| {
                tx.begin_install(&pkg);
            });
        });
        assert_persisted_states(&app_dirs, &mut lock_file, &before);
    }

    #[test]
    fn transaction_commit_failure_keeps_database_unchanged() {
        let (_tempdir, app_dirs) = testing::make_app_dirs();
        let mut lock_file = DbLockFile::open(&app_dirs).unwrap();

        let installed_pkg = testing::make_package_definition("example-font@0.1.0");
        let incomplete_install_pkg = testing::make_package_definition("example-font@0.2.0");
        let incomplete_uninstall_pkg = testing::make_package_definition("example-font@0.3.0");
        let before = [
            (&installed_pkg.id, Some(InstallationState::Installed)),
            (
                &incomplete_install_pkg.id,
                Some(InstallationState::IncompleteInstall),
            ),
            (
                &incomplete_uninstall_pkg.id,
                Some(InstallationState::IncompleteUninstall),
            ),
        ];
        let staged = [
            (
                &installed_pkg.id,
                Some(InstallationState::IncompleteUninstall),
            ),
            (
                &incomplete_install_pkg.id,
                Some(InstallationState::IncompleteUninstall),
            ),
            (
                &incomplete_uninstall_pkg.id,
                Some(InstallationState::IncompleteUninstall),
            ),
        ];

        with_db(&app_dirs, &mut lock_file, |db| {
            testing::mark_as_installed(db, &installed_pkg);
            testing::mark_as_incomplete_install(db, &incomplete_install_pkg);
            testing::mark_as_incomplete_uninstall(db, &incomplete_uninstall_pkg);
        });

        assert_persisted_states(&app_dirs, &mut lock_file, &before);
        with_db(&app_dirs, &mut lock_file, |db| {
            assert_loaded_database_states(db, &before);

            db.simulate_save_failure = true;
            let mut tx = db.transaction();
            tx.begin_uninstall(&installed_pkg.id).unwrap();
            tx.begin_uninstall(&incomplete_install_pkg.id).unwrap();
            tx.begin_uninstall(&incomplete_uninstall_pkg.id).unwrap();
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

        let installed_pkg = testing::make_package_definition("example-font@0.1.0");
        let pkg = testing::make_package_definition("example-font@0.2.0");

        let before = [
            (&installed_pkg.id, Some(InstallationState::Installed)),
            (&pkg.id, None),
        ];
        let staged = [
            (&installed_pkg.id, Some(InstallationState::Installed)),
            (&pkg.id, Some(InstallationState::Installed)),
        ];

        with_db(&app_dirs, &mut lock_file, |db| {
            testing::mark_as_installed(db, &installed_pkg);
        });

        assert_persisted_states(&app_dirs, &mut lock_file, &before);
        with_db(&app_dirs, &mut lock_file, |db| {
            assert_loaded_database_states(db, &before);

            let mut tx = db.transaction();
            tx.begin_install(&pkg);
            tx.complete_install(&pkg.id, &[]).unwrap();
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

        let pkg = testing::make_package_definition("example-font@0.1.0");
        let expected = [(&pkg.id, Some(InstallationState::IncompleteInstall))];

        with_db(&app_dirs, &mut lock_file, |db| {
            testing::mark_as_incomplete_install(db, &pkg);
        });

        assert_persisted_states(&app_dirs, &mut lock_file, &expected);
        with_db(&app_dirs, &mut lock_file, |db| {
            assert_transaction_stages_without_commit(db, &expected, &expected, |tx| {
                tx.begin_install(&pkg);
            });
        });
        assert_persisted_states(&app_dirs, &mut lock_file, &expected);
    }

    #[test]
    fn complete_install_marks_target_installed_without_touching_other_versions_in_transaction() {
        let (_tempdir, app_dirs) = testing::make_app_dirs();
        let mut lock_file = DbLockFile::open(&app_dirs).unwrap();

        let installed_pkg = testing::make_package_definition("example-font@0.1.0");
        let pkg = testing::make_package_definition("example-font@0.2.0");

        let before = [
            (&installed_pkg.id, Some(InstallationState::Installed)),
            (&pkg.id, Some(InstallationState::IncompleteInstall)),
        ];
        let staged = [
            (&installed_pkg.id, Some(InstallationState::Installed)),
            (&pkg.id, Some(InstallationState::Installed)),
        ];

        with_db(&app_dirs, &mut lock_file, |db| {
            testing::mark_as_installed(db, &installed_pkg);
            let mut tx = db.transaction();
            tx.begin_install(&pkg);
            tx.commit().unwrap();
        });

        assert_persisted_states(&app_dirs, &mut lock_file, &before);
        with_db(&app_dirs, &mut lock_file, |db| {
            assert_transaction_stages_without_commit(db, &before, &staged, |tx| {
                tx.complete_install(&pkg.id, &[]).unwrap();
                assert_eq!(
                    tx.working_state.entry(&pkg.id).unwrap().activation_state(),
                    ActivationState::Inactive,
                );
            });
        });
        assert_persisted_states(&app_dirs, &mut lock_file, &before);
    }

    #[test]
    fn cancel_install_removes_incomplete_entry_from_transaction() {
        let (_tempdir, app_dirs) = testing::make_app_dirs();
        let mut lock_file = DbLockFile::open(&app_dirs).unwrap();

        let pkg = testing::make_package_definition("example-font@0.1.0");
        let before = [(&pkg.id, Some(InstallationState::IncompleteInstall))];
        let staged = [(&pkg.id, None)];

        with_db(&app_dirs, &mut lock_file, |db| {
            testing::mark_as_incomplete_install(db, &pkg);
        });

        assert_persisted_states(&app_dirs, &mut lock_file, &before);
        with_db(&app_dirs, &mut lock_file, |db| {
            assert_transaction_stages_without_commit(db, &before, &staged, |tx| {
                tx.cancel_install(&pkg.id, false).unwrap();
            });
        });
        assert_persisted_states(&app_dirs, &mut lock_file, &before);
    }

    #[test]
    fn cancel_install_marks_entry_incomplete_uninstall_in_transaction_when_cleanup_failed() {
        let (_tempdir, app_dirs) = testing::make_app_dirs();
        let mut lock_file = DbLockFile::open(&app_dirs).unwrap();

        let pkg = testing::make_package_definition("example-font@0.1.0");
        let before = [(&pkg.id, Some(InstallationState::IncompleteInstall))];
        let staged = [(&pkg.id, Some(InstallationState::IncompleteUninstall))];

        with_db(&app_dirs, &mut lock_file, |db| {
            testing::mark_as_incomplete_install(db, &pkg);
        });

        assert_persisted_states(&app_dirs, &mut lock_file, &before);
        with_db(&app_dirs, &mut lock_file, |db| {
            assert_transaction_stages_without_commit(db, &before, &staged, |tx| {
                tx.cancel_install(&pkg.id, true).unwrap();
            });
        });
        assert_persisted_states(&app_dirs, &mut lock_file, &before);
    }

    #[test]
    fn begin_activate_and_complete_activate_mark_entry_active_in_transaction() {
        let (_tempdir, app_dirs) = testing::make_app_dirs();
        let mut lock_file = DbLockFile::open(&app_dirs).unwrap();

        let pkg = testing::make_package_definition("example-font@0.1.0");

        with_db(&app_dirs, &mut lock_file, |db| {
            testing::mark_as_installed(db, &pkg);

            let mut tx = db.transaction();
            tx.begin_activate(&pkg.id).unwrap();
            assert_eq!(
                tx.working_state.entry(&pkg.id).unwrap().activation_state(),
                ActivationState::IncompleteActivation,
            );

            tx.complete_activate(&pkg.id).unwrap();
            assert_eq!(
                tx.working_state.entry(&pkg.id).unwrap().activation_state(),
                ActivationState::Active
            );

            tx.commit().unwrap();
            assert_eq!(
                db.entry_by_id(&pkg.id).unwrap().activation_state(),
                ActivationState::Active,
            );
        });
    }

    #[test]
    fn cancel_activate_handles_cleanup_and_non_cleanup_transitions() {
        let (_tempdir, app_dirs) = testing::make_app_dirs();
        let mut lock_file = DbLockFile::open(&app_dirs).unwrap();

        let inactive_pkg = testing::make_package_definition("example-font-inactive@0.1.0");
        let cleanup_pkg = testing::make_package_definition("example-font-cleanup@0.1.0");

        with_db(&app_dirs, &mut lock_file, |db| {
            testing::mark_as_installed(db, &inactive_pkg);
            testing::mark_as_installed(db, &cleanup_pkg);

            let mut tx = db.transaction();
            tx.begin_activate(&inactive_pkg.id).unwrap();
            tx.begin_activate(&cleanup_pkg.id).unwrap();

            tx.cancel_activate(&inactive_pkg.id, false).unwrap();
            tx.cancel_activate(&cleanup_pkg.id, true).unwrap();

            assert_eq!(
                tx.working_state
                    .entry(&inactive_pkg.id)
                    .unwrap()
                    .activation_state(),
                ActivationState::Inactive,
            );
            assert_eq!(
                tx.working_state
                    .entry(&cleanup_pkg.id)
                    .unwrap()
                    .activation_state(),
                ActivationState::IncompleteDeactivation,
            );
        });
    }

    #[test]
    fn begin_deactivate_and_complete_deactivate_mark_entry_inactive_in_transaction() {
        let (_tempdir, app_dirs) = testing::make_app_dirs();
        let mut lock_file = DbLockFile::open(&app_dirs).unwrap();

        let pkg = testing::make_package_definition("example-font@0.1.0");

        with_db(&app_dirs, &mut lock_file, |db| {
            testing::mark_as_active(db, &pkg);

            let mut tx = db.transaction();
            tx.begin_deactivate(&pkg.id).unwrap();
            assert_eq!(
                tx.working_state.entry(&pkg.id).unwrap().activation_state(),
                ActivationState::IncompleteDeactivation,
            );

            tx.complete_deactivate(&pkg.id).unwrap();
            assert_eq!(
                tx.working_state.entry(&pkg.id).unwrap().activation_state(),
                ActivationState::Inactive
            );

            tx.commit().unwrap();
            assert_eq!(
                db.entry_by_id(&pkg.id).unwrap().activation_state(),
                ActivationState::Inactive,
            );
        });
    }

    #[test]
    fn begin_uninstall_rejects_active_entries() {
        let (_tempdir, app_dirs) = testing::make_app_dirs();
        let mut lock_file = DbLockFile::open(&app_dirs).unwrap();

        let pkg = testing::make_package_definition("example-font@0.1.0");

        with_db(&app_dirs, &mut lock_file, |db| {
            testing::mark_as_active(db, &pkg);

            let mut tx = db.transaction();
            let err = tx.begin_uninstall(&pkg.id).unwrap_err();
            assert_matches!(
                err,
                PackageDatabaseError::UnexpectedActivationState {
                    pkg_id: actual_pkg_id,
                    expected: ActivationState::Inactive,
                    actual: ActivationState::Active,
                } if actual_pkg_id == pkg.id
            );
        });
    }

    #[test]
    fn complete_uninstall_removes_last_version_entry_from_transaction() {
        let (_tempdir, app_dirs) = testing::make_app_dirs();
        let mut lock_file = DbLockFile::open(&app_dirs).unwrap();

        let pkg = testing::make_package_definition("example-font@0.1.0");
        let before = [(&pkg.id, Some(InstallationState::IncompleteUninstall))];
        let staged = [(&pkg.id, None)];

        with_db(&app_dirs, &mut lock_file, |db| {
            testing::mark_as_incomplete_uninstall(db, &pkg);
        });

        assert_persisted_states(&app_dirs, &mut lock_file, &before);
        with_db(&app_dirs, &mut lock_file, |db| {
            assert_transaction_stages_without_commit(db, &before, &staged, |tx| {
                tx.complete_uninstall(&pkg.id).unwrap();
            });
        });
        assert_persisted_states(&app_dirs, &mut lock_file, &before);
    }
}
