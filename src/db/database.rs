use std::{
    fs::File,
    io::{self, BufReader},
    path::PathBuf,
};

use snafu::{IntoError as _, ResultExt as _, Snafu};
use tempfile::NamedTempFile;

use crate::{
    db::{
        DbLockFileGuard, PackageDbEntry, PackageDbState, PackageDbStateError,
        persist::{self, PersistError, PersistedPackageDb},
        simulation::SimulatedPackageDbState,
    },
    package::{PackageDefinition, PackageFont, PackageId, PackageSpec},
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
    #[snafu(transparent)]
    State { source: PackageDbStateError },
    #[cfg(test)]
    #[snafu(display("simulated failure to save package database"))]
    SimulatedSaveFailure,
}

#[derive(Debug)]
pub(crate) struct PackageDatabase<'lock> {
    persist_path: AbsolutePath,
    persisted_state: PackageDbState,
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
            persisted_state: PackageDbState::new(persist_db),
            _lock_file_guard: lock_file_guard,
            #[cfg(test)]
            simulate_save_failure: false,
        })
    }

    pub(crate) fn all_entries(&self) -> impl Iterator<Item = PackageDbEntry<'_>> {
        self.persisted_state.all_entries()
    }

    pub(crate) fn entries_by_spec<'a>(
        &'a self,
        pkg_spec: &PackageSpec,
    ) -> impl Iterator<Item = PackageDbEntry<'a>> {
        self.persisted_state.entries_by_spec(pkg_spec)
    }

    pub(crate) fn entry_by_id<'a>(&'a self, pkg_id: &PackageId) -> Option<PackageDbEntry<'a>> {
        self.persisted_state.entry_by_id(pkg_id)
    }

    pub(crate) fn simulated_state(&self) -> SimulatedPackageDbState {
        SimulatedPackageDbState::new(self.persisted_state.clone())
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
    working_state: PackageDbState,
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
        persist::to_writer(&temp_file, self.working_state.data()).context(
            SerializeDatabaseSnafu {
                path: temp_file.path(),
            },
        )?;
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

    pub(crate) fn begin_install(&mut self, pkg: &PackageDefinition) {
        self.working_state.begin_install(pkg);
    }

    pub(crate) fn complete_install(
        &mut self,
        pkg_id: &PackageId,
        pkg_fonts: &[PackageFont],
    ) -> Result<(), PackageDatabaseError> {
        self.working_state
            .complete_install(pkg_id, pkg_fonts)
            .map_err(Into::into)
    }

    pub(crate) fn cancel_install(
        &mut self,
        pkg_id: &PackageId,
        cleanup_required: bool,
    ) -> Result<(), PackageDatabaseError> {
        self.working_state
            .cancel_install(pkg_id, cleanup_required)
            .map_err(Into::into)
    }

    pub(crate) fn begin_uninstall(
        &mut self,
        pkg_id: &PackageId,
    ) -> Result<(), PackageDatabaseError> {
        self.working_state
            .begin_uninstall(pkg_id)
            .map_err(Into::into)
    }

    pub(crate) fn complete_uninstall(
        &mut self,
        pkg_id: &PackageId,
    ) -> Result<(), PackageDatabaseError> {
        self.working_state
            .complete_uninstall(pkg_id)
            .map_err(Into::into)
    }

    pub(crate) fn begin_activate(
        &mut self,
        pkg_id: &PackageId,
    ) -> Result<(), PackageDatabaseError> {
        self.working_state
            .begin_activate(pkg_id)
            .map_err(Into::into)
    }

    pub(crate) fn complete_activate(
        &mut self,
        pkg_id: &PackageId,
    ) -> Result<(), PackageDatabaseError> {
        self.working_state
            .complete_activate(pkg_id)
            .map_err(Into::into)
    }

    pub(crate) fn cancel_activate(
        &mut self,
        pkg_id: &PackageId,
        cleanup_required: bool,
    ) -> Result<(), PackageDatabaseError> {
        self.working_state
            .cancel_activate(pkg_id, cleanup_required)
            .map_err(Into::into)
    }

    pub(crate) fn begin_deactivate(
        &mut self,
        pkg_id: &PackageId,
    ) -> Result<(), PackageDatabaseError> {
        self.working_state
            .begin_deactivate(pkg_id)
            .map_err(Into::into)
    }

    pub(crate) fn complete_deactivate(
        &mut self,
        pkg_id: &PackageId,
    ) -> Result<(), PackageDatabaseError> {
        self.working_state
            .complete_deactivate(pkg_id)
            .map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use std::assert_matches;

    use super::*;
    use crate::{db::DbLockFile, package::InstallationState, util::testing};

    fn with_db<F, R>(app_dirs: &AppDirs, lock_file: &mut DbLockFile, f: F) -> R
    where
        F: FnOnce(&mut PackageDatabase<'_>) -> R,
    {
        let lock_file_guard = lock_file.try_lock().unwrap();
        let mut db = PackageDatabase::load(app_dirs, lock_file_guard).unwrap();
        f(&mut db)
    }

    fn load_db<'a>(app_dirs: &'a AppDirs, lock_file: &'a mut DbLockFile) -> PackageDatabase<'a> {
        let lock_file_guard = lock_file.try_lock().unwrap();
        PackageDatabase::load(app_dirs, lock_file_guard).unwrap()
    }

    fn get_installation_state(
        db: &PackageDatabase<'_>,
        pkg_id: &PackageId,
    ) -> Option<InstallationState> {
        db.entry_by_id(pkg_id)
            .map(|entry| entry.installation_state())
    }

    #[track_caller]
    fn assert_installation_state(
        db: &PackageDatabase<'_>,
        pkg_id: &PackageId,
        expected: Option<InstallationState>,
    ) {
        assert_eq!(get_installation_state(db, pkg_id), expected);
    }

    #[test]
    fn load_missing_database_starts_empty() {
        let (_tempdir, app_dirs) = testing::make_app_dirs();
        let mut lock_file = DbLockFile::open(&app_dirs).unwrap();
        let db = load_db(&app_dirs, &mut lock_file);

        assert_eq!(db.all_entries().count(), 0);
    }

    #[test]
    fn transaction_without_commit_discards_changes() {
        let (_tempdir, app_dirs) = testing::make_app_dirs();
        let mut lock_file = DbLockFile::open(&app_dirs).unwrap();
        let pkg = testing::make_package_definition("example-font@0.1.0");

        with_db(&app_dirs, &mut lock_file, |db| {
            {
                let mut tx = db.transaction();
                tx.begin_install(&pkg);

                assert_eq!(
                    tx.working_state
                        .entry_by_id(&pkg.id)
                        .map(|entry| entry.installation_state()),
                    Some(InstallationState::IncompleteInstall),
                );
            }

            assert_installation_state(db, &pkg.id, None);
        });

        let db = load_db(&app_dirs, &mut lock_file);
        assert_installation_state(&db, &pkg.id, None);
    }

    #[test]
    fn transaction_commit_persists_staged_changes() {
        let (_tempdir, app_dirs) = testing::make_app_dirs();
        let mut lock_file = DbLockFile::open(&app_dirs).unwrap();
        let installed_pkg = testing::make_package_definition("example-font@0.1.0");
        let pkg = testing::make_package_definition("example-font@0.2.0");

        with_db(&app_dirs, &mut lock_file, |db| {
            testing::mark_as_installed(db, &installed_pkg);

            let mut tx = db.transaction();
            tx.begin_install(&pkg);
            tx.complete_install(&pkg.id, &[]).unwrap();
            tx.commit().unwrap();

            assert_installation_state(db, &installed_pkg.id, Some(InstallationState::Installed));
            assert_installation_state(db, &pkg.id, Some(InstallationState::Installed));
        });

        let db = load_db(&app_dirs, &mut lock_file);
        assert_installation_state(&db, &installed_pkg.id, Some(InstallationState::Installed));
        assert_installation_state(&db, &pkg.id, Some(InstallationState::Installed));
    }

    #[test]
    fn transaction_commit_failure_keeps_database_unchanged() {
        let (_tempdir, app_dirs) = testing::make_app_dirs();
        let mut lock_file = DbLockFile::open(&app_dirs).unwrap();
        let installed_pkg = testing::make_package_definition("example-font@0.1.0");
        let incomplete_install_pkg = testing::make_package_definition("example-font@0.2.0");
        let incomplete_uninstall_pkg = testing::make_package_definition("example-font@0.3.0");

        with_db(&app_dirs, &mut lock_file, |db| {
            testing::mark_as_installed(db, &installed_pkg);
            testing::mark_as_incomplete_install(db, &incomplete_install_pkg);
            testing::mark_as_incomplete_uninstall(db, &incomplete_uninstall_pkg);
        });

        with_db(&app_dirs, &mut lock_file, |db| {
            db.simulate_save_failure = true;

            let mut tx = db.transaction();
            tx.begin_uninstall(&installed_pkg.id).unwrap();
            tx.begin_uninstall(&incomplete_install_pkg.id).unwrap();
            tx.begin_uninstall(&incomplete_uninstall_pkg.id).unwrap();

            let err = tx.commit().unwrap_err();
            assert_matches!(err, PackageDatabaseError::SimulatedSaveFailure);

            assert_installation_state(db, &installed_pkg.id, Some(InstallationState::Installed));
            assert_installation_state(
                db,
                &incomplete_install_pkg.id,
                Some(InstallationState::IncompleteInstall),
            );
            assert_installation_state(
                db,
                &incomplete_uninstall_pkg.id,
                Some(InstallationState::IncompleteUninstall),
            );
        });

        let db = load_db(&app_dirs, &mut lock_file);
        assert_installation_state(&db, &installed_pkg.id, Some(InstallationState::Installed));
        assert_installation_state(
            &db,
            &incomplete_install_pkg.id,
            Some(InstallationState::IncompleteInstall),
        );
        assert_installation_state(
            &db,
            &incomplete_uninstall_pkg.id,
            Some(InstallationState::IncompleteUninstall),
        );
    }
}
