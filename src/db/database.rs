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
            PersistedPackageVersionMap,
        },
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
pub(crate) struct PackageDbEntry<'a>(&'a PersistedPackageEntry);

impl<'a> From<&'a PersistedPackageEntry> for PackageDbEntry<'a> {
    fn from(entry: &'a PersistedPackageEntry) -> Self {
        Self(entry)
    }
}

impl<'a> PackageDbEntry<'a> {
    pub(crate) fn installation_state(&self) -> InstallationState {
        self.0.installation_state
    }

    pub(crate) fn activation_state(&self) -> ActivationState {
        self.0.activation_state
    }

    pub(crate) fn definition(&self) -> Arc<PackageDefinition> {
        Arc::clone(&self.0.definition)
    }

    pub(crate) fn font_entries(&self) -> impl Iterator<Item = FontEntry> + 'a {
        self.0
            .font_entries
            .iter()
            .map(|entry| FontEntry::new(entry.title.clone(), entry.file_name.clone()))
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

    fn raw_version_map(&self, pkg_name: &PackageName) -> Option<&PersistedPackageVersionMap> {
        self.persist_db.packages.get(pkg_name)
    }

    fn raw_entry(&self, pkg_id: &PackageId) -> Option<&PersistedPackageEntry> {
        let version_map = self.raw_version_map(pkg_id.name())?;
        version_map.versions.get(pkg_id.version())
    }

    pub(crate) fn entries(&self) -> impl Iterator<Item = PackageDbEntry<'_>> {
        self.persist_db
            .packages
            .values()
            .flat_map(|version_map| version_map.versions.values())
            .map(Into::into)
    }

    pub(crate) fn entries_by_spec(
        &self,
        pkg_spec: &PackageSpec,
    ) -> Box<dyn Iterator<Item = PackageDbEntry<'_>> + '_> {
        match pkg_spec {
            PackageSpec::Id(id) => Box::new(self.entry_by_id(id).into_iter()),
            PackageSpec::Name(name) => Box::new(self.entries_by_name(name)),
        }
    }

    pub(crate) fn entry_by_id(&self, pkg_id: &PackageId) -> Option<PackageDbEntry<'_>> {
        self.raw_entry(pkg_id).map(Into::into)
    }

    pub(crate) fn entries_by_name(
        &self,
        pkg_name: &PackageName,
    ) -> impl Iterator<Item = PackageDbEntry<'_>> + '_ {
        self.raw_version_map(pkg_name)
            .into_iter()
            .flat_map(|version_map| version_map.versions.values().map(Into::into))
    }

    pub(crate) fn check_installability(&self, pkg_id: &PackageId) -> Installability {
        if let Some(entry) = self.raw_entry(pkg_id)
            && entry.installation_state.is_installed()
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
        if let Some(version_map) = self.raw_version_map(pkg_name) {
            for entry in version_map.versions.values() {
                if entry.definition.id.version() == pkg_version {
                    match entry.activation_state {
                        ActivationState::Active => return Activatability::AlreadyActive,
                        ActivationState::Inactive
                        | ActivationState::IncompleteActivation
                        | ActivationState::IncompleteDeactivation => {}
                    }
                } else {
                    match entry.activation_state {
                        ActivationState::Active => {
                            active_other_versions.push(entry.definition.id.clone());
                        }
                        ActivationState::Inactive => {}
                        ActivationState::IncompleteActivation => {
                            incomplete_activations.push(entry.definition.id.clone());
                        }
                        ActivationState::IncompleteDeactivation => {
                            incomplete_deactivations.push(entry.definition.id.clone());
                        }
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

    fn raw_version_map(&self, pkg_name: &PackageName) -> Option<&PersistedPackageVersionMap> {
        self.persist_db.packages.get(pkg_name)
    }

    fn raw_version_map_mut(
        &mut self,
        pkg_name: &PackageName,
    ) -> Option<&mut PersistedPackageVersionMap> {
        self.persist_db.packages.get_mut(pkg_name)
    }

    fn raw_entry(&self, pkg_id: &PackageId) -> Option<&PersistedPackageEntry> {
        let version_map = self.raw_version_map(pkg_id.name())?;
        version_map.versions.get(pkg_id.version())
    }

    fn raw_entry_mut(&mut self, pkg_id: &PackageId) -> Option<&mut PersistedPackageEntry> {
        let version_map = self.raw_version_map_mut(pkg_id.name())?;
        version_map.versions.get_mut(pkg_id.version())
    }

    fn insert_entry(&mut self, entry: PersistedPackageEntry) {
        let pkg_name = entry.definition.id.name().clone();
        let pkg_version = entry.definition.id.version().clone();
        self.persist_db
            .packages
            .entry(pkg_name)
            .or_default()
            .versions
            .insert(pkg_version, entry);
    }

    fn remove_entry(&mut self, pkg_id: &PackageId) -> Option<PersistedPackageEntry> {
        let version_map = self.raw_version_map_mut(pkg_id.name())?;
        let entry = version_map.versions.remove(pkg_id.version())?;
        if version_map.versions.is_empty() {
            self.persist_db.packages.remove(pkg_id.name());
        }
        Some(entry)
    }

    fn check_installation_state(
        &self,
        pkg_id: &PackageId,
        expected: InstallationState,
    ) -> Result<(), PackageDatabaseError> {
        let entry = self
            .raw_entry(pkg_id)
            .context(EntryNotFoundSnafu { pkg_id })?;
        let actual = entry.installation_state;
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
            .raw_entry(pkg_id)
            .context(EntryNotFoundSnafu { pkg_id })?;
        let actual = entry.activation_state;
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
            .raw_entry_mut(pkg_id)
            .context(EntryNotFoundSnafu { pkg_id })?;
        entry.installation_state = new_state;
        Ok(())
    }

    fn update_activation_state(
        &mut self,
        pkg_id: &PackageId,
        new_state: ActivationState,
    ) -> Result<(), PackageDatabaseError> {
        let entry = self
            .raw_entry_mut(pkg_id)
            .context(EntryNotFoundSnafu { pkg_id })?;
        entry.activation_state = new_state;
        Ok(())
    }

    pub(crate) fn begin_install(&mut self, definition: Arc<PackageDefinition>) {
        let entry = PersistedPackageEntry {
            installation_state: InstallationState::IncompleteInstall,
            activation_state: ActivationState::Inactive,
            definition,
            font_entries: vec![],
        };
        self.insert_entry(entry);
    }

    pub(crate) fn complete_install(
        &mut self,
        pkg_id: &PackageId,
        font_entries: &[FontEntry],
    ) -> Result<(), PackageDatabaseError> {
        self.check_installation_state(pkg_id, InstallationState::IncompleteInstall)?;

        let db_entry = self.raw_entry_mut(pkg_id).unwrap();
        db_entry.installation_state = InstallationState::Installed;
        db_entry.font_entries = font_entries.iter().map(Into::into).collect();
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
            self.remove_entry(pkg_id);
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
        self.remove_entry(pkg_id);
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
        assert_eq!(entry.definition().id, *id);
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
        id: &PackageId,
    ) -> Option<InstallationState> {
        let entry = tx
            .persist_db
            .packages
            .get(id.name())
            .and_then(|version_map| version_map.versions.get(id.version()))?;
        assert_eq!(entry.definition.id, *id);
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

        let pkg = testing::make_package_definition("example-font@0.1.0");
        let before = [(&pkg.id, None)];
        let staged = [(&pkg.id, Some(InstallationState::IncompleteInstall))];

        assert_persisted_states(&app_dirs, &mut lock_file, &before);
        with_db(&app_dirs, &mut lock_file, |db| {
            assert_transaction_stages_without_commit(db, &before, &staged, |tx| {
                tx.begin_install(Arc::clone(&pkg));
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
            tx.begin_install(Arc::clone(&pkg));
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
                tx.begin_install(Arc::clone(&pkg));
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
            tx.begin_install(Arc::clone(&pkg));
            tx.commit().unwrap();
        });

        assert_persisted_states(&app_dirs, &mut lock_file, &before);
        with_db(&app_dirs, &mut lock_file, |db| {
            assert_transaction_stages_without_commit(db, &before, &staged, |tx| {
                tx.complete_install(&pkg.id, &[]).unwrap();
                assert_eq!(
                    tx.raw_entry(&pkg.id).unwrap().activation_state,
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
                tx.raw_entry(&pkg.id).unwrap().activation_state,
                ActivationState::IncompleteActivation,
            );

            tx.complete_activate(&pkg.id).unwrap();
            assert_eq!(
                tx.raw_entry(&pkg.id).unwrap().activation_state,
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
                tx.raw_entry(&inactive_pkg.id).unwrap().activation_state,
                ActivationState::Inactive,
            );
            assert_eq!(
                tx.raw_entry(&cleanup_pkg.id).unwrap().activation_state,
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
                tx.raw_entry(&pkg.id).unwrap().activation_state,
                ActivationState::IncompleteDeactivation,
            );

            tx.complete_deactivate(&pkg.id).unwrap();
            assert_eq!(
                tx.raw_entry(&pkg.id).unwrap().activation_state,
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
