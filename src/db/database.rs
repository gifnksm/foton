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
    engine::{ExecutionPlan, ExecutionPlanOp, InstallOp, UninstallOp},
    package::{PackageId, PackageManifest, PackageName, PackageSpec, PackageState},
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
            PackageSpec::Name(name) => Box::new(self.entries_by_name(name)),
        }
    }

    pub(crate) fn entry_by_id(
        &self,
        pkg_id: &PackageId,
    ) -> Option<(PackageState, Arc<PackageManifest>)> {
        self.persist_db
            .packages
            .get(pkg_id.name())
            .and_then(|version_map| version_map.versions.get(pkg_id.version()))
            .map(|entry| (entry.state, Arc::clone(&entry.manifest)))
    }

    pub(crate) fn entries_by_name(
        &'a self,
        pkg_name: &PackageName,
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

    fn entry_mut(&mut self, pkg_id: &PackageId) -> Option<&mut PersistedPackageEntry> {
        let version_map = self.persist_db.packages.get_mut(pkg_id.name())?;
        version_map.versions.get_mut(pkg_id.version())
    }

    fn insert_entry(&mut self, state: PackageState, manifest: Arc<PackageManifest>) {
        let pkg_name = manifest.name.clone();
        let pkg_version = manifest.version.clone();
        self.persist_db
            .packages
            .entry(pkg_name)
            .or_default()
            .versions
            .insert(pkg_version, PersistedPackageEntry { state, manifest });
    }

    fn remove_entry(&mut self, pkg_id: &PackageId) -> Option<PersistedPackageEntry> {
        let version_map = self.persist_db.packages.get_mut(pkg_id.name())?;
        let entry = version_map.versions.remove(pkg_id.version())?;
        if version_map.versions.is_empty() {
            self.persist_db.packages.remove(pkg_id.name());
        }
        Some(entry)
    }

    pub(crate) fn check_installability(&self, manifest: &PackageManifest) -> Installability {
        let pkg_name = manifest.name.clone();
        let pkg_version = manifest.version.clone();
        let mut installed_other_versions = vec![];
        let mut pending_installs = vec![];
        let mut pending_uninstalls = vec![];
        for (state, m) in self.entries_by_name(&pkg_name) {
            if m.version == pkg_version {
                match state {
                    PackageState::Installed => return Installability::AlreadyInstalled,
                    PackageState::PendingInstall | PackageState::PendingUninstall => {}
                }
            } else {
                match state {
                    PackageState::Installed => installed_other_versions.push(m.id()),
                    PackageState::PendingInstall => pending_installs.push(m.id()),
                    PackageState::PendingUninstall => pending_uninstalls.push(m.id()),
                }
            }
        }
        Installability::Installable {
            installed_other_versions,
            pending_installs,
            pending_uninstalls,
        }
    }

    pub(crate) fn check_uninstallability(&self, pkg_id: &PackageId) -> Uninstallability {
        if self.entry_by_id(pkg_id).is_some() {
            Uninstallability::Uninstallable
        } else {
            Uninstallability::AlreadyUninstalled
        }
    }

    pub(crate) fn apply_plan_transaction(
        &mut self,
        plan: &ExecutionPlan,
    ) -> Result<(), PackageDatabaseError> {
        if !plan.has_side_effects() {
            return Ok(());
        }

        for op in plan.ops() {
            match op {
                ExecutionPlanOp::Install(InstallOp { manifest, .. }) => {
                    self.insert_entry(PackageState::PendingInstall, Arc::clone(manifest));
                }
                ExecutionPlanOp::Uninstall(UninstallOp {
                    pkg_id, conditions, ..
                }) if conditions.is_empty() => {
                    let entry = self
                        .entry_mut(pkg_id)
                        .context(EntryNotFoundSnafu { pkg_id })?;
                    entry.state = PackageState::PendingUninstall;
                }
                ExecutionPlanOp::Uninstall(_) | ExecutionPlanOp::Skip(_) => {}
            }
        }
        self.save()?;
        Ok(())
    }

    pub(crate) fn complete_install(
        &mut self,
        pkg_id: &PackageId,
        replacing_pkg_ids: &[PackageId],
    ) -> Result<(), PackageDatabaseError> {
        let entry = self
            .entry_mut(pkg_id)
            .context(EntryNotFoundSnafu { pkg_id })?;
        snafu::ensure!(
            entry.state == PackageState::PendingInstall,
            UnexpectedStateSnafu {
                pkg_id,
                expected: PackageState::PendingInstall,
                actual: entry.state,
            }
        );
        for replacing_pkg_id in replacing_pkg_ids {
            let entry = self
                .entry_mut(replacing_pkg_id)
                .context(EntryNotFoundSnafu {
                    pkg_id: replacing_pkg_id,
                })?;
            snafu::ensure!(
                entry.state == PackageState::Installed,
                UnexpectedStateSnafu {
                    pkg_id: replacing_pkg_id,
                    expected: PackageState::Installed,
                    actual: entry.state,
                }
            );
        }

        self.entry_mut(pkg_id).unwrap().state = PackageState::Installed;
        for replacing_pkg_id in replacing_pkg_ids {
            self.entry_mut(replacing_pkg_id).unwrap().state = PackageState::PendingUninstall;
        }

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
            .get_mut(pkg_id.name())
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
            self.persist_db.packages.remove(pkg_id.name());
        }
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
    Installable {
        installed_other_versions: Vec<PackageId>,
        pending_installs: Vec<PackageId>,
        pending_uninstalls: Vec<PackageId>,
    },
    AlreadyInstalled,
}

#[derive(Debug, Clone, derive_more::IsVariant)]
pub(crate) enum Uninstallability {
    Uninstallable,
    AlreadyUninstalled,
}

#[cfg(test)]
mod tests {
    use std::assert_matches;

    use super::*;
    use crate::{db::DbLockFile, engine::UninstallReason, util::testing};

    fn get_entry_state(db: &PackageDatabase<'_>, id: &PackageId) -> Option<PackageState> {
        let (state, manifest) = db.entry_by_id(id)?;
        assert_eq!(manifest.id(), *id);
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
    fn complete_install_persists_installed_state_and_marks_replaced_versions_pending_uninstall() {
        let (_tempdir, app_dirs) = testing::make_app_dirs();
        let mut lock_file = DbLockFile::open(&app_dirs).unwrap();

        let installed_manifest = testing::make_manifest("example-font@0.1.0");
        let installed_pkg_id = installed_manifest.id();
        let manifest = testing::make_manifest("example-font@0.2.0");
        let pkg_id = manifest.id();
        let replacing_pkg_ids = vec![installed_pkg_id.clone()];

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
                    Some(PackageState::PendingUninstall),
                ),
                (&pkg_id, None, Some(PackageState::Installed)),
            ],
            |db| {
                let plan = testing::make_install_plan(&manifest, replacing_pkg_ids.clone());
                db.apply_plan_transaction(&plan).unwrap();
                db.complete_install(&pkg_id, &replacing_pkg_ids).unwrap();
            },
        );
    }

    #[test]
    fn apply_plan_transaction_keeps_same_version_pending_install() {
        let (_tempdir, app_dirs) = testing::make_app_dirs();
        let mut lock_file = DbLockFile::open(&app_dirs).unwrap();

        let manifest = testing::make_manifest("example-font@0.1.0");
        let pkg_id = manifest.id();

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
                let plan = testing::make_install_plan(&manifest, vec![]);
                db.apply_plan_transaction(&plan).unwrap();
            },
        );
    }

    #[test]
    fn apply_plan_transaction_save_failure_rolls_back_previous_states() {
        let (_tempdir, app_dirs) = testing::make_app_dirs();
        let mut lock_file = DbLockFile::open(&app_dirs).unwrap();

        let installed_manifest = testing::make_manifest("example-font@0.1.0");
        let pending_install_manifest = testing::make_manifest("example-font@0.2.0");
        let pending_uninstall_manifest = testing::make_manifest("example-font@0.3.0");

        with_db(&app_dirs, &mut lock_file, |db| {
            testing::mark_as_installed(db, &installed_manifest);
            testing::mark_as_pending_installed(db, &pending_install_manifest);
            testing::mark_as_pending_uninstalled(db, &pending_uninstall_manifest);
        });
        assert_statuses_change(
            &app_dirs,
            &mut lock_file,
            &[
                (
                    &installed_manifest.id(),
                    Some(PackageState::Installed),
                    Some(PackageState::Installed),
                ),
                (
                    &pending_install_manifest.id(),
                    Some(PackageState::PendingInstall),
                    Some(PackageState::PendingInstall),
                ),
                (
                    &pending_uninstall_manifest.id(),
                    Some(PackageState::PendingUninstall),
                    Some(PackageState::PendingUninstall),
                ),
            ],
            |db| {
                let plan = ExecutionPlan::new_for_test([
                    UninstallOp {
                        pkg_id: installed_manifest.id(),
                        reason: UninstallReason::RequestedByUser,
                        conditions: vec![],
                    }
                    .into(),
                    UninstallOp {
                        pkg_id: pending_install_manifest.id(),
                        reason: UninstallReason::RequestedByUser,
                        conditions: vec![],
                    }
                    .into(),
                    UninstallOp {
                        pkg_id: pending_uninstall_manifest.id(),
                        reason: UninstallReason::RequestedByUser,
                        conditions: vec![],
                    }
                    .into(),
                ]);
                db.simulate_save_failure = true;
                let err = db.apply_plan_transaction(&plan).unwrap_err();
                assert_matches!(err, PackageDatabaseError::SimulatedSaveFailure);
            },
        );
    }

    #[test]
    fn cancel_install_removes_pending_entry() {
        let (_tempdir, app_dirs) = testing::make_app_dirs();
        let mut lock_file = DbLockFile::open(&app_dirs).unwrap();

        let manifest = testing::make_manifest("example-font@0.1.0");
        let pkg_id = manifest.id();

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

        let manifest = testing::make_manifest("example-font@0.1.0");
        let pkg_id = manifest.id();

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
                let plan = testing::make_uninstall_plan(&pkg_id, vec![]);
                db.apply_plan_transaction(&plan).unwrap();
                db.complete_uninstall(&pkg_id).unwrap();
            },
        );
    }

    #[test]
    fn complete_install_save_failure_restores_previous_states() {
        let (_tempdir, app_dirs) = testing::make_app_dirs();
        let mut lock_file = DbLockFile::open(&app_dirs).unwrap();

        let installed_manifest = testing::make_manifest("example-font@0.1.0");
        let installed_pkg_id = installed_manifest.id();
        let manifest = testing::make_manifest("example-font@0.2.0");
        let pkg_id = manifest.id();
        let replacing_pkg_ids = vec![installed_pkg_id.clone()];

        with_db(&app_dirs, &mut lock_file, |db| {
            testing::mark_as_installed(db, &installed_manifest);
            let plan = testing::make_install_plan(&manifest, replacing_pkg_ids.clone());
            db.apply_plan_transaction(&plan).unwrap();
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
                (
                    &pkg_id,
                    Some(PackageState::PendingInstall),
                    Some(PackageState::PendingInstall),
                ),
            ],
            |db| {
                db.simulate_save_failure = true;
                let err = db
                    .complete_install(&pkg_id, &replacing_pkg_ids)
                    .unwrap_err();
                assert_matches!(err, PackageDatabaseError::SimulatedSaveFailure);
            },
        );
    }

    #[test]
    fn cancel_install_save_failure_restores_pending_install_state() {
        let (_tempdir, app_dirs) = testing::make_app_dirs();
        let mut lock_file = DbLockFile::open(&app_dirs).unwrap();

        let manifest = testing::make_manifest("example-font@0.1.0");
        let pkg_id = manifest.id();

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
                assert_matches!(err, PackageDatabaseError::SimulatedSaveFailure);
            },
        );
    }

    #[test]
    fn complete_uninstall_save_failure_restores_pending_uninstall_state() {
        let (_tempdir, app_dirs) = testing::make_app_dirs();
        let mut lock_file = DbLockFile::open(&app_dirs).unwrap();

        let manifest = testing::make_manifest("example-font@0.1.0");
        let pkg_id = manifest.id();

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
                assert_matches!(err, PackageDatabaseError::SimulatedSaveFailure);
            },
        );
    }
}
