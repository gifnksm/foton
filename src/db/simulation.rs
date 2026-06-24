use crate::{
    db::{Activatability, ActivationConflict, Installability, PackageDbState},
    package::{PackageDefinition, PackageId},
};

#[derive(Debug, Clone)]
pub(crate) struct SimulatedPackageDbState {
    state: PackageDbState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SimulatedInstallationResult {
    Installed,
    Reinstalled,
    AlreadyInstalled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SimulatedActivationResult {
    Activated {
        deactivated: Vec<(PackageId, ActivationConflict)>,
    },
    AlreadyActive,
    NotInstalled,
}

impl SimulatedPackageDbState {
    pub(in crate::db) fn new(state: PackageDbState) -> Self {
        Self { state }
    }

    pub(crate) fn install(&mut self, pkg: &PackageDefinition) -> SimulatedInstallationResult {
        let result = match self.state.check_installability(pkg) {
            Installability::Installable => SimulatedInstallationResult::Installed,
            Installability::ConflictingDefinition => SimulatedInstallationResult::Reinstalled,
            Installability::AlreadyInstalled => {
                return SimulatedInstallationResult::AlreadyInstalled;
            }
        };
        self.state.begin_install(pkg);
        self.state.complete_install(&pkg.id, &[]).unwrap();
        result
    }

    pub(crate) fn activate(&mut self, pkg_id: &PackageId) -> SimulatedActivationResult {
        let conflicts = match self.state.check_activatability(pkg_id) {
            Activatability::Activatable { conflicts } => conflicts,
            Activatability::AlreadyActive => return SimulatedActivationResult::AlreadyActive,
            Activatability::NotInstalled => return SimulatedActivationResult::NotInstalled,
        };

        self.state.begin_activate(pkg_id).unwrap();
        self.state.complete_activate(pkg_id).unwrap();

        for (pkg_id, _) in &conflicts {
            self.state.begin_deactivate(pkg_id).unwrap();
            self.state.complete_deactivate(pkg_id).unwrap();
        }

        SimulatedActivationResult::Activated {
            deactivated: conflicts,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        package::{ActivationState, InstallationState},
        util::testing,
    };

    fn get_entry_state(
        state: &SimulatedPackageDbState,
        pkg_id: &PackageId,
    ) -> Option<(InstallationState, ActivationState)> {
        let entry = state.state.entry_by_id(pkg_id)?;
        Some((entry.installation_state(), entry.activation_state()))
    }

    #[track_caller]
    fn assert_entry_state(
        state: &SimulatedPackageDbState,
        pkg_id: &PackageId,
        expected: Option<(InstallationState, ActivationState)>,
    ) {
        assert_eq!(get_entry_state(state, pkg_id), expected);
    }

    #[test]
    fn install_returns_already_installed_without_changing_state() {
        let pkg = testing::make_package_definition("example-font@0.1.0");

        testing::with_db(|_cx, db| {
            testing::mark_as_active(db, &pkg);
            let mut state = db.simulated_state();

            assert_eq!(
                state.install(&pkg),
                SimulatedInstallationResult::AlreadyInstalled
            );
            assert_entry_state(
                &state,
                &pkg.id,
                Some((InstallationState::Installed, ActivationState::Active)),
            );
        });
    }

    #[test]
    fn install_reinstalls_same_version_with_different_definition_and_resets_activation() {
        let installed_pkg = testing::make_package_definition_with_display_name(
            "example-font@0.1.0",
            "Installed Definition",
        );
        let manifest_pkg = testing::make_package_definition_with_display_name(
            "example-font@0.1.0",
            "Manifest Definition",
        );

        testing::with_db(|_cx, db| {
            testing::mark_as_active(db, &installed_pkg);
            let mut state = db.simulated_state();

            assert_eq!(
                state.install(&manifest_pkg),
                SimulatedInstallationResult::Reinstalled
            );
            assert_entry_state(
                &state,
                &manifest_pkg.id,
                Some((InstallationState::Installed, ActivationState::Inactive)),
            );
            assert_eq!(
                state
                    .state
                    .entry_by_id(&manifest_pkg.id)
                    .unwrap()
                    .make_definition(),
                manifest_pkg,
            );
        });
    }

    #[test]
    fn install_from_incomplete_states_marks_package_installed_and_inactive() {
        let pkg = testing::make_package_definition("example-font@0.1.0");

        for state in [
            InstallationState::IncompleteInstall,
            InstallationState::IncompleteUninstall,
        ] {
            testing::with_db(|_cx, db| {
                testing::mark_as_state(db, &pkg, state);
                let mut state = db.simulated_state();

                assert_eq!(state.install(&pkg), SimulatedInstallationResult::Installed);
                assert_entry_state(
                    &state,
                    &pkg.id,
                    Some((InstallationState::Installed, ActivationState::Inactive)),
                );
            });
        }
    }

    #[test]
    fn activate_reports_conflicts_and_applies_state_changes() {
        let incomplete_activation_pkg = testing::make_package_definition("example-font@0.1.0");
        let incomplete_deactivation_pkg = testing::make_package_definition("example-font@0.2.0");
        let active_pkg = testing::make_package_definition("example-font@0.3.0");
        let target_pkg = testing::make_package_definition("example-font@0.4.0");

        testing::with_db(|_cx, db| {
            testing::mark_as_installed(db, &incomplete_activation_pkg);
            testing::mark_as_active(db, &incomplete_deactivation_pkg);
            testing::mark_as_active(db, &active_pkg);
            testing::mark_as_installed(db, &target_pkg);

            let mut tx = db.transaction();
            tx.begin_activate(&incomplete_activation_pkg.id).unwrap();
            tx.commit().unwrap();

            let mut tx = db.transaction();
            tx.begin_deactivate(&incomplete_deactivation_pkg.id)
                .unwrap();
            tx.commit().unwrap();

            let mut state = db.simulated_state();
            let result = state.activate(&target_pkg.id);

            assert_eq!(
                result,
                SimulatedActivationResult::Activated {
                    deactivated: vec![
                        (
                            incomplete_activation_pkg.id.clone(),
                            ActivationConflict::IncompleteActivation,
                        ),
                        (
                            incomplete_deactivation_pkg.id.clone(),
                            ActivationConflict::IncompleteDeactivation,
                        ),
                        (
                            active_pkg.id.clone(),
                            ActivationConflict::ActiveOtherVersion,
                        ),
                    ],
                },
            );
            assert_entry_state(
                &state,
                &target_pkg.id,
                Some((InstallationState::Installed, ActivationState::Active)),
            );
            assert_entry_state(
                &state,
                &incomplete_activation_pkg.id,
                Some((InstallationState::Installed, ActivationState::Inactive)),
            );
            assert_entry_state(
                &state,
                &incomplete_deactivation_pkg.id,
                Some((InstallationState::Installed, ActivationState::Inactive)),
            );
            assert_entry_state(
                &state,
                &active_pkg.id,
                Some((InstallationState::Installed, ActivationState::Inactive)),
            );
        });
    }

    #[test]
    fn activate_returns_already_active_for_active_package() {
        let pkg = testing::make_package_definition("example-font@0.1.0");

        testing::with_db(|_cx, db| {
            testing::mark_as_active(db, &pkg);
            let mut state = db.simulated_state();

            assert_eq!(
                state.activate(&pkg.id),
                SimulatedActivationResult::AlreadyActive
            );
            assert_entry_state(
                &state,
                &pkg.id,
                Some((InstallationState::Installed, ActivationState::Active)),
            );
        });
    }

    #[test]
    fn activate_returns_not_installed_for_missing_or_incomplete_packages() {
        let pkg = testing::make_package_definition("example-font@0.1.0");

        testing::with_db(|_cx, db| {
            let mut state = db.simulated_state();
            assert_eq!(
                state.activate(&pkg.id),
                SimulatedActivationResult::NotInstalled
            );
            assert_entry_state(&state, &pkg.id, None);
        });

        for install_state in [
            InstallationState::IncompleteInstall,
            InstallationState::IncompleteUninstall,
        ] {
            testing::with_db(|_cx, db| {
                testing::mark_as_state(db, &pkg, install_state);
                let mut state = db.simulated_state();

                assert_eq!(
                    state.activate(&pkg.id),
                    SimulatedActivationResult::NotInstalled
                );
                assert_entry_state(
                    &state,
                    &pkg.id,
                    Some((install_state, ActivationState::Inactive)),
                );
            });
        }
    }
}
