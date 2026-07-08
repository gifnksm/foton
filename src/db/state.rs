use either::Either;
use snafu::{OptionExt as _, Snafu};

use crate::{
    db::persist::{PersistedPackageDb, PersistedPackageEntry},
    package::{
        ActivationState, InstallationState, PackageDefinition, PackageFont, PackageId, PackageName,
        PackageSpec,
    },
};

#[derive(Debug, Snafu)]
pub(crate) enum PackageDbStateError {
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
}

#[derive(Debug, Clone)]
pub(in crate::db) struct PackageDbState {
    data: PersistedPackageDb,
}

#[derive(Debug)]
pub(crate) struct PackageDbEntry<'a> {
    id: PackageId,
    data: &'a PersistedPackageEntry,
}

#[derive(Debug, Clone, derive_more::IsVariant)]
pub(crate) enum Installability {
    Installable,
    AlreadyInstalled,
    ConflictingDefinition,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ActivationConflict {
    ActiveOtherVersion,
    IncompleteActivation,
    IncompleteDeactivation,
}

#[derive(Debug, Clone, derive_more::IsVariant)]
pub(crate) enum Activatability {
    Activatable {
        conflicts: Vec<(PackageId, ActivationConflict)>,
    },
    AlreadyActive,
    NotInstalled,
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

    pub(crate) fn fonts(&self) -> impl Iterator<Item = PackageFont> + 'a {
        self.data.fonts()
    }
}

impl PackageDbState {
    pub(in crate::db) fn new(data: PersistedPackageDb) -> Self {
        Self { data }
    }

    pub(in crate::db) fn data(&self) -> &PersistedPackageDb {
        &self.data
    }

    pub(in crate::db) fn all_entries(&self) -> impl Iterator<Item = PackageDbEntry<'_>> {
        self.data
            .all_entries()
            .map(|(pkg_name, pkg_version, entry)| {
                let pkg_id = PackageId::new(pkg_name.clone(), pkg_version.clone());
                PackageDbEntry::new(pkg_id, entry)
            })
    }

    pub(in crate::db) fn entries_by_spec<'a>(
        &'a self,
        pkg_spec: &PackageSpec,
    ) -> impl Iterator<Item = PackageDbEntry<'a>> {
        match pkg_spec {
            PackageSpec::Id(id) => Either::Left(self.entry_by_id(id).into_iter()),
            PackageSpec::Name(name) => Either::Right(self.entries_by_name(name)),
        }
    }

    pub(in crate::db) fn entry_by_id<'a>(
        &'a self,
        pkg_id: &PackageId,
    ) -> Option<PackageDbEntry<'a>> {
        self.data
            .entry(pkg_id)
            .map(|entry| PackageDbEntry::new(pkg_id.clone(), entry))
    }

    pub(in crate::db) fn entries_by_name<'a>(
        &'a self,
        pkg_name: &PackageName,
    ) -> impl Iterator<Item = PackageDbEntry<'a>> {
        self.data
            .entries_by_name(pkg_name)
            .map(|(pkg_version, entry)| {
                let pkg_id = PackageId::new(pkg_name.clone(), pkg_version.clone());
                PackageDbEntry::new(pkg_id, entry)
            })
    }

    pub(in crate::db) fn check_installability(&self, pkg: &PackageDefinition) -> Installability {
        if let Some(entry) = self.data.entry(&pkg.id)
            && entry.installation_state().is_installed()
        {
            if entry.has_same_definition_as(pkg) {
                Installability::AlreadyInstalled
            } else {
                Installability::ConflictingDefinition
            }
        } else {
            Installability::Installable
        }
    }

    pub(in crate::db) fn check_activatability(&self, pkg_id: &PackageId) -> Activatability {
        let pkg_name = pkg_id.name();
        let pkg_version = pkg_id.version();
        let mut conflicts = vec![];
        let mut activatable = false;
        for (entry_version, entry) in self.data.entries_by_name(pkg_name) {
            if entry_version == pkg_version {
                if !entry.installation_state().is_installed() {
                    return Activatability::NotInstalled;
                }
                activatable = true;
                match entry.activation_state() {
                    ActivationState::Active => return Activatability::AlreadyActive,
                    ActivationState::Inactive
                    | ActivationState::IncompleteActivation
                    | ActivationState::IncompleteDeactivation => {}
                }
            } else {
                let conflict = match entry.activation_state() {
                    ActivationState::Active => Some(ActivationConflict::ActiveOtherVersion),
                    ActivationState::Inactive => None,
                    ActivationState::IncompleteActivation => {
                        Some(ActivationConflict::IncompleteActivation)
                    }
                    ActivationState::IncompleteDeactivation => {
                        Some(ActivationConflict::IncompleteDeactivation)
                    }
                };
                if let Some(conflict) = conflict {
                    conflicts.push((
                        PackageId::new(pkg_name.clone(), entry_version.clone()),
                        conflict,
                    ));
                }
            }
        }
        if !activatable {
            return Activatability::NotInstalled;
        }
        Activatability::Activatable { conflicts }
    }

    fn check_installation_state(
        &self,
        pkg_id: &PackageId,
        expected: InstallationState,
    ) -> Result<(), PackageDbStateError> {
        let entry = self
            .data
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
    ) -> Result<(), PackageDbStateError> {
        let entry = self
            .data
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
    ) -> Result<(), PackageDbStateError> {
        let entry = self
            .data
            .entry_mut(pkg_id)
            .context(EntryNotFoundSnafu { pkg_id })?;
        entry.set_installation_state(new_state);
        Ok(())
    }

    fn update_activation_state(
        &mut self,
        pkg_id: &PackageId,
        new_state: ActivationState,
    ) -> Result<(), PackageDbStateError> {
        let entry = self
            .data
            .entry_mut(pkg_id)
            .context(EntryNotFoundSnafu { pkg_id })?;
        entry.set_activation_state(new_state);
        Ok(())
    }

    pub(in crate::db) fn begin_install(&mut self, pkg: &PackageDefinition) {
        let entry = PersistedPackageEntry::new(
            InstallationState::IncompleteInstall,
            ActivationState::Inactive,
            pkg,
        );
        self.data.insert_entry(&pkg.id, entry);
    }

    pub(in crate::db) fn complete_install(
        &mut self,
        pkg_id: &PackageId,
        pkg_fonts: &[PackageFont],
    ) -> Result<(), PackageDbStateError> {
        self.check_installation_state(pkg_id, InstallationState::IncompleteInstall)?;

        let db_entry = self.data.entry_mut(pkg_id).unwrap();
        db_entry.set_installation_state(InstallationState::Installed);
        db_entry.set_fonts(pkg_fonts);
        Ok(())
    }

    pub(in crate::db) fn cancel_install(
        &mut self,
        pkg_id: &PackageId,
        cleanup_required: bool,
    ) -> Result<(), PackageDbStateError> {
        self.check_installation_state(pkg_id, InstallationState::IncompleteInstall)?;
        if cleanup_required {
            self.update_installation_state(pkg_id, InstallationState::IncompleteUninstall)?;
        } else {
            self.data.remove_entry(pkg_id);
        }
        Ok(())
    }

    pub(in crate::db) fn begin_uninstall(
        &mut self,
        pkg_id: &PackageId,
    ) -> Result<(), PackageDbStateError> {
        self.check_activation_state(pkg_id, ActivationState::Inactive)?;
        self.update_installation_state(pkg_id, InstallationState::IncompleteUninstall)?;
        Ok(())
    }

    pub(in crate::db) fn complete_uninstall(
        &mut self,
        pkg_id: &PackageId,
    ) -> Result<(), PackageDbStateError> {
        self.check_installation_state(pkg_id, InstallationState::IncompleteUninstall)?;
        self.data.remove_entry(pkg_id);
        Ok(())
    }

    pub(in crate::db) fn begin_activate(
        &mut self,
        pkg_id: &PackageId,
    ) -> Result<(), PackageDbStateError> {
        self.check_installation_state(pkg_id, InstallationState::Installed)?;
        self.update_activation_state(pkg_id, ActivationState::IncompleteActivation)?;
        Ok(())
    }

    pub(in crate::db) fn complete_activate(
        &mut self,
        pkg_id: &PackageId,
    ) -> Result<(), PackageDbStateError> {
        self.check_activation_state(pkg_id, ActivationState::IncompleteActivation)?;
        self.update_activation_state(pkg_id, ActivationState::Active)?;
        Ok(())
    }

    pub(in crate::db) fn cancel_activate(
        &mut self,
        pkg_id: &PackageId,
        cleanup_required: bool,
    ) -> Result<(), PackageDbStateError> {
        self.check_activation_state(pkg_id, ActivationState::IncompleteActivation)?;
        if cleanup_required {
            self.update_activation_state(pkg_id, ActivationState::IncompleteDeactivation)?;
        } else {
            self.update_activation_state(pkg_id, ActivationState::Inactive)?;
        }
        Ok(())
    }

    pub(in crate::db) fn begin_deactivate(
        &mut self,
        pkg_id: &PackageId,
    ) -> Result<(), PackageDbStateError> {
        self.update_activation_state(pkg_id, ActivationState::IncompleteDeactivation)?;
        Ok(())
    }

    pub(in crate::db) fn complete_deactivate(
        &mut self,
        pkg_id: &PackageId,
    ) -> Result<(), PackageDbStateError> {
        self.check_activation_state(pkg_id, ActivationState::IncompleteDeactivation)?;
        self.update_activation_state(pkg_id, ActivationState::Inactive)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::assert_matches;

    use super::*;
    use crate::{db::persist::PersistedPackageDb, util::testing};

    fn new_state() -> PackageDbState {
        PackageDbState::new(PersistedPackageDb::default())
    }

    fn get_entry_state(
        state: &PackageDbState,
        pkg_id: &PackageId,
    ) -> Option<(InstallationState, ActivationState)> {
        let entry = state.entry_by_id(pkg_id)?;
        Some((entry.installation_state(), entry.activation_state()))
    }

    #[track_caller]
    fn assert_entry_state(
        state: &PackageDbState,
        pkg_id: &PackageId,
        expected: Option<(InstallationState, ActivationState)>,
    ) {
        assert_eq!(get_entry_state(state, pkg_id), expected);
    }

    fn mark_as_incomplete_install(state: &mut PackageDbState, pkg: &PackageDefinition) {
        state.begin_install(pkg);
    }

    fn mark_as_installed(state: &mut PackageDbState, pkg: &PackageDefinition) {
        state.begin_install(pkg);
        state.complete_install(&pkg.id, &[]).unwrap();
    }

    fn mark_as_active(state: &mut PackageDbState, pkg: &PackageDefinition) {
        mark_as_installed(state, pkg);
        state.begin_activate(&pkg.id).unwrap();
        state.complete_activate(&pkg.id).unwrap();
    }

    fn mark_as_incomplete_uninstall(state: &mut PackageDbState, pkg: &PackageDefinition) {
        mark_as_installed(state, pkg);
        state.begin_uninstall(&pkg.id).unwrap();
    }

    #[test]
    fn check_installability_returns_conflicting_definition_for_same_version_with_different_definition()
     {
        let mut state = new_state();
        let installed_pkg = testing::make_package_definition_with_description(
            "example-font@0.1.0",
            "Installed Definition",
        );
        let manifest_pkg = testing::make_package_definition_with_description(
            "example-font@0.1.0",
            "Manifest Definition",
        );

        mark_as_installed(&mut state, &installed_pkg);

        assert!(
            state
                .check_installability(&manifest_pkg)
                .is_conflicting_definition()
        );
    }

    #[test]
    fn begin_install_keeps_same_version_incomplete_install_state() {
        let mut state = new_state();
        let pkg = testing::make_package_definition("example-font@0.1.0");

        mark_as_incomplete_install(&mut state, &pkg);
        state.begin_install(&pkg);

        assert_entry_state(
            &state,
            &pkg.id,
            Some((
                InstallationState::IncompleteInstall,
                ActivationState::Inactive,
            )),
        );
    }

    #[test]
    fn complete_install_marks_target_installed_without_touching_other_versions() {
        let mut state = new_state();
        let installed_pkg = testing::make_package_definition("example-font@0.1.0");
        let pkg = testing::make_package_definition("example-font@0.2.0");

        mark_as_installed(&mut state, &installed_pkg);
        mark_as_incomplete_install(&mut state, &pkg);

        state.complete_install(&pkg.id, &[]).unwrap();

        assert_entry_state(
            &state,
            &installed_pkg.id,
            Some((InstallationState::Installed, ActivationState::Inactive)),
        );
        assert_entry_state(
            &state,
            &pkg.id,
            Some((InstallationState::Installed, ActivationState::Inactive)),
        );
    }

    #[test]
    fn cancel_install_removes_incomplete_entry_without_cleanup() {
        let mut state = new_state();
        let pkg = testing::make_package_definition("example-font@0.1.0");

        mark_as_incomplete_install(&mut state, &pkg);
        state.cancel_install(&pkg.id, false).unwrap();

        assert_entry_state(&state, &pkg.id, None);
    }

    #[test]
    fn cancel_install_marks_entry_incomplete_uninstall_when_cleanup_required() {
        let mut state = new_state();
        let pkg = testing::make_package_definition("example-font@0.1.0");

        mark_as_incomplete_install(&mut state, &pkg);
        state.cancel_install(&pkg.id, true).unwrap();

        assert_entry_state(
            &state,
            &pkg.id,
            Some((
                InstallationState::IncompleteUninstall,
                ActivationState::Inactive,
            )),
        );
    }

    #[test]
    fn begin_activate_and_complete_activate_mark_entry_active() {
        let mut state = new_state();
        let pkg = testing::make_package_definition("example-font@0.1.0");

        mark_as_installed(&mut state, &pkg);
        state.begin_activate(&pkg.id).unwrap();
        assert_entry_state(
            &state,
            &pkg.id,
            Some((
                InstallationState::Installed,
                ActivationState::IncompleteActivation,
            )),
        );

        state.complete_activate(&pkg.id).unwrap();
        assert_entry_state(
            &state,
            &pkg.id,
            Some((InstallationState::Installed, ActivationState::Active)),
        );
    }

    #[test]
    fn cancel_activate_handles_cleanup_and_non_cleanup_transitions() {
        let mut state = new_state();
        let inactive_pkg = testing::make_package_definition("example-font-inactive@0.1.0");
        let cleanup_pkg = testing::make_package_definition("example-font-cleanup@0.1.0");

        mark_as_installed(&mut state, &inactive_pkg);
        mark_as_installed(&mut state, &cleanup_pkg);
        state.begin_activate(&inactive_pkg.id).unwrap();
        state.begin_activate(&cleanup_pkg.id).unwrap();

        state.cancel_activate(&inactive_pkg.id, false).unwrap();
        state.cancel_activate(&cleanup_pkg.id, true).unwrap();

        assert_entry_state(
            &state,
            &inactive_pkg.id,
            Some((InstallationState::Installed, ActivationState::Inactive)),
        );
        assert_entry_state(
            &state,
            &cleanup_pkg.id,
            Some((
                InstallationState::Installed,
                ActivationState::IncompleteDeactivation,
            )),
        );
    }

    #[test]
    fn begin_deactivate_and_complete_deactivate_mark_entry_inactive() {
        let mut state = new_state();
        let pkg = testing::make_package_definition("example-font@0.1.0");

        mark_as_active(&mut state, &pkg);
        state.begin_deactivate(&pkg.id).unwrap();
        assert_entry_state(
            &state,
            &pkg.id,
            Some((
                InstallationState::Installed,
                ActivationState::IncompleteDeactivation,
            )),
        );

        state.complete_deactivate(&pkg.id).unwrap();
        assert_entry_state(
            &state,
            &pkg.id,
            Some((InstallationState::Installed, ActivationState::Inactive)),
        );
    }

    #[test]
    fn begin_uninstall_rejects_active_entries() {
        let mut state = new_state();
        let pkg = testing::make_package_definition("example-font@0.1.0");

        mark_as_active(&mut state, &pkg);

        let err = state.begin_uninstall(&pkg.id).unwrap_err();
        assert_matches!(
            err,
            PackageDbStateError::UnexpectedActivationState {
                pkg_id: actual_pkg_id,
                expected: ActivationState::Inactive,
                actual: ActivationState::Active,
            } if actual_pkg_id == pkg.id
        );
    }

    #[test]
    fn complete_uninstall_removes_last_version_entry() {
        let mut state = new_state();
        let pkg = testing::make_package_definition("example-font@0.1.0");

        mark_as_incomplete_uninstall(&mut state, &pkg);
        state.complete_uninstall(&pkg.id).unwrap();

        assert_entry_state(&state, &pkg.id, None);
    }
}
