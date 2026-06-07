use std::sync::Arc;

use crate::{
    db::{Activatability, Installability, PackageDatabase},
    engine::{
        ActivateOp, ActivateReason, DeactivateOp, DeactivateReason, ExecutionPlan, InstallOp,
        InstallReason, PlanStep, ResolvedInstallTarget, SkipOp, StepCondition, StepId,
        plan::SkipReason,
    },
    package::{PackageId, PackageManifest},
};

pub(crate) fn plan_install(
    db: &PackageDatabase<'_>,
    targets: &[ResolvedInstallTarget],
) -> ExecutionPlan {
    let mut ops = vec![];
    for target in targets {
        let ResolvedInstallTarget {
            source: _,
            manifest,
            should_activate,
        } = target;
        let install_step_id = plan_install_op(db, manifest, &mut ops);
        let target_id = manifest.id();
        if *should_activate {
            plan_activate_op(db, &target_id, install_step_id, &mut ops);
        }
    }
    ExecutionPlan { steps: ops }
}

fn plan_install_op(
    db: &PackageDatabase<'_>,
    manifest: &Arc<PackageManifest>,
    ops: &mut Vec<PlanStep>,
) -> Option<StepId> {
    let target_id = manifest.id();
    match db.check_installability(&target_id) {
        Installability::Installable => {
            let step = PlanStep::new(InstallOp {
                manifest: Arc::clone(manifest),
                reason: InstallReason::RequestedByUser,
            });
            let step_id = step.step_id;
            ops.push(step);
            Some(step_id)
        }
        Installability::AlreadyInstalled => {
            ops.push(PlanStep::new(SkipOp {
                pkg_spec: target_id.clone().into(),
                reason: SkipReason::AlreadyInstalled,
            }));
            None
        }
    }
}

fn plan_activate_op(
    db: &PackageDatabase<'_>,
    target_id: &PackageId,
    dependency: Option<StepId>,
    ops: &mut Vec<PlanStep>,
) {
    match db.check_activatability(target_id) {
        Activatability::Activatable {
            active_other_versions,
            incomplete_activations,
            incomplete_deactivations,
        } => {
            let mut conditions = vec![];
            if let Some(step_id) = dependency {
                conditions.push(StepCondition::AfterSuccess(step_id));
            }
            let activate_op = PlanStep::with_conditions(
                ActivateOp {
                    pkg_id: target_id.clone(),
                    reason: ActivateReason::RequestedByUser,
                },
                conditions,
            );
            let activate_step_id = activate_op.step_id;
            ops.push(activate_op);
            let mut deactivate_ops = vec![];
            let deactivates = [
                (
                    DeactivateReason::ConflictWithActive {
                        pkg_id: target_id.clone(),
                    },
                    active_other_versions,
                ),
                (
                    DeactivateReason::CleanupIncompleteActivation,
                    incomplete_activations,
                ),
                (
                    DeactivateReason::CleanupIncompleteDeactivation,
                    incomplete_deactivations,
                ),
            ];
            for (reason, pkg_ids) in deactivates {
                for pkg_id in pkg_ids {
                    let reason = reason.clone();
                    deactivate_ops.push((
                        DeactivateOp { pkg_id, reason },
                        vec![StepCondition::AfterSuccess(activate_step_id)],
                    ));
                }
            }
            deactivate_ops.sort_by(|(a, _), (b, _)| a.pkg_id.cmp(&b.pkg_id));
            ops.extend(
                deactivate_ops
                    .into_iter()
                    .map(|(op, conditions)| PlanStep::with_conditions(op, conditions)),
            );
        }
        Activatability::AlreadyActive => ops.push(PlanStep::new(SkipOp {
            pkg_spec: target_id.clone().into(),
            reason: SkipReason::AlreadyActive,
        })),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::{
        engine::{ActivateOp, ActivateReason, DeactivateOp, DeactivateReason, plan},
        package::InstallationState,
        util::testing,
    };

    #[test]
    fn plan_install_activates_target_and_deactivates_other_versions() {
        let incomplete_activation_manifest = testing::make_manifest("example-font@0.1.0");
        let incomplete_deactivation_manifest = testing::make_manifest("example-font@0.2.0");
        let active_manifest = testing::make_manifest("example-font@0.3.0");
        let install_target_manifest = testing::make_manifest("example-font@0.4.0");
        let install_target = testing::make_resolved_install_target(&install_target_manifest);

        let plan = testing::with_db(|_cx, db| {
            testing::mark_as_installed(db, &incomplete_activation_manifest);
            testing::mark_as_active(db, &incomplete_deactivation_manifest);
            testing::mark_as_active(db, &active_manifest);

            let mut tx = db.transaction();
            tx.begin_activate(&incomplete_activation_manifest.id())
                .unwrap();
            tx.commit().unwrap();

            let mut tx = db.transaction();
            tx.begin_deactivate(&incomplete_deactivation_manifest.id())
                .unwrap();
            tx.commit().unwrap();

            plan_install(db, &[install_target])
        });

        let install_step = PlanStep::new(InstallOp {
            manifest: Arc::clone(&install_target_manifest),
            reason: InstallReason::RequestedByUser,
        });
        let install_step_id = install_step.step_id;
        let activate_step = PlanStep::with_conditions(
            ActivateOp {
                pkg_id: install_target_manifest.id(),
                reason: ActivateReason::RequestedByUser,
            },
            vec![StepCondition::AfterSuccess(install_step_id)],
        );
        let activate_step_id = activate_step.step_id;

        plan::testing::assert_plan_eq(
            &plan,
            &ExecutionPlan::new_for_test([
                install_step,
                activate_step,
                PlanStep::with_conditions(
                    DeactivateOp {
                        pkg_id: incomplete_activation_manifest.id(),
                        reason: DeactivateReason::CleanupIncompleteActivation,
                    },
                    vec![StepCondition::AfterSuccess(activate_step_id)],
                ),
                PlanStep::with_conditions(
                    DeactivateOp {
                        pkg_id: incomplete_deactivation_manifest.id(),
                        reason: DeactivateReason::CleanupIncompleteDeactivation,
                    },
                    vec![StepCondition::AfterSuccess(activate_step_id)],
                ),
                PlanStep::with_conditions(
                    DeactivateOp {
                        pkg_id: active_manifest.id(),
                        reason: DeactivateReason::ConflictWithActive {
                            pkg_id: install_target_manifest.id(),
                        },
                    },
                    vec![StepCondition::AfterSuccess(activate_step_id)],
                ),
            ]),
        );
    }

    #[test]
    fn plan_install_activates_already_installed_inactive_package_without_reinstall() {
        let manifest = testing::make_manifest("example-font@0.1.0");
        let install_target = testing::make_resolved_install_target(&manifest);

        let plan = testing::with_db(|_cx, db| {
            testing::mark_as_installed(db, &manifest);
            plan_install(db, &[install_target])
        });

        plan::testing::assert_plan_eq(
            &plan,
            &ExecutionPlan::new_for_test([
                PlanStep::new(SkipOp {
                    pkg_spec: manifest.id().into(),
                    reason: SkipReason::AlreadyInstalled,
                }),
                PlanStep::new(ActivateOp {
                    pkg_id: manifest.id(),
                    reason: ActivateReason::RequestedByUser,
                }),
            ]),
        );
    }

    #[test]
    fn plan_install_skips_already_installed_when_activation_is_not_requested() {
        let manifest = testing::make_manifest("example-font@0.1.0");
        let install_target =
            testing::make_resolved_install_target_with_activation(&manifest, false);

        let plan = testing::with_db(|_cx, db| {
            testing::mark_as_installed(db, &manifest);
            plan_install(db, &[install_target])
        });

        plan::testing::assert_plan_eq(
            &plan,
            &ExecutionPlan::new_for_test([PlanStep::new(SkipOp {
                pkg_spec: manifest.id().into(),
                reason: SkipReason::AlreadyInstalled,
            })]),
        );
    }

    #[test]
    fn plan_install_reinstalls_same_version_incomplete_states_without_activation() {
        let pairs = [
            (
                InstallationState::IncompleteUninstall,
                "example-font-pu@0.1.0",
            ),
            (
                InstallationState::IncompleteInstall,
                "example-font-pi@0.1.0",
            ),
        ];
        for (state, pkg_id) in pairs {
            let manifest = testing::make_manifest(pkg_id);
            let install_target =
                testing::make_resolved_install_target_with_activation(&manifest, false);

            let plan = testing::with_db(|_cx, db| {
                testing::mark_as_state(db, &manifest, state);
                plan_install(db, &[install_target])
            });

            plan::testing::assert_plan_eq(
                &plan,
                &ExecutionPlan::new_for_test([PlanStep::new(InstallOp {
                    manifest: Arc::clone(&manifest),
                    reason: InstallReason::RequestedByUser,
                })]),
            );
        }
    }
}
