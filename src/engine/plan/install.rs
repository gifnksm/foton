use std::sync::Arc;

use crate::{
    db::{
        ActivationConflict, PackageDatabase, SimulatedActivationResult,
        SimulatedInstallationResult, SimulatedPackageDbState,
    },
    engine::{
        ActivateOp, ActivateReason, DeactivateOp, DeactivateReason, ExecutionPlan, InstallOp,
        InstallReason, InstallTargetKind, PlanStep, ResolvedInstallTarget, SkipOp, StepCondition,
        StepId, plan::SkipReason,
    },
    package::{PackageDefinition, PackageId},
};

pub(crate) fn plan_install(
    db: &PackageDatabase<'_>,
    targets: &[ResolvedInstallTarget],
) -> ExecutionPlan {
    let mut db = db.simulated_state();
    let mut ops = vec![];
    for target in targets {
        let ResolvedInstallTarget {
            kind,
            detail: _,
            should_activate,
        } = target;
        let install_step_id = plan_install_op(&mut db, kind, &mut ops);
        if *should_activate {
            plan_activate_op(&mut db, kind.pkg_id(), install_step_id, &mut ops);
        }
    }
    ExecutionPlan { steps: ops }
}

fn plan_install_op(
    db: &mut SimulatedPackageDbState,
    kind: &InstallTargetKind,
    ops: &mut Vec<PlanStep>,
) -> Option<StepId> {
    match kind {
        InstallTargetKind::AlreadyInstalled { pkg_id, .. } => {
            ops.push(PlanStep::new(SkipOp {
                pkg_spec: pkg_id.clone().into(),
                reason: SkipReason::AlreadyInstalled,
            }));
            None
        }
        InstallTargetKind::Install { pkg, .. } => {
            let result = db.install(pkg);
            emit_install_steps(pkg, result, ops)
        }
    }
}

fn plan_activate_op(
    db: &mut SimulatedPackageDbState,
    target_id: &PackageId,
    dependency: Option<StepId>,
    ops: &mut Vec<PlanStep>,
) {
    let result = db.activate(target_id);
    emit_activation_steps(target_id, dependency, result, ops);
}

fn emit_install_steps(
    pkg: &Arc<PackageDefinition>,
    result: SimulatedInstallationResult,
    ops: &mut Vec<PlanStep>,
) -> Option<StepId> {
    match result {
        SimulatedInstallationResult::Installed | SimulatedInstallationResult::Reinstalled => {
            let step = PlanStep::new(InstallOp {
                pkg: Arc::clone(pkg),
                reason: InstallReason::RequestedByUser,
            });
            let step_id = step.step_id;
            ops.push(step);
            Some(step_id)
        }
        SimulatedInstallationResult::AlreadyInstalled => {
            ops.push(PlanStep::new(SkipOp {
                pkg_spec: pkg.id.clone().into(),
                reason: SkipReason::AlreadyInstalled,
            }));
            None
        }
    }
}

fn emit_activation_steps(
    target_id: &PackageId,
    dependency: Option<StepId>,
    result: SimulatedActivationResult,
    ops: &mut Vec<PlanStep>,
) {
    match result {
        SimulatedActivationResult::Activated { deactivated } => {
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
            for (pkg_id, conflict) in deactivated {
                let reason = match conflict {
                    ActivationConflict::ActiveOtherVersion => {
                        DeactivateReason::ConflictWithActivation {
                            pkg_id: target_id.clone(),
                        }
                    }
                    ActivationConflict::IncompleteActivation => {
                        DeactivateReason::CleanupIncompleteActivation
                    }
                    ActivationConflict::IncompleteDeactivation => {
                        DeactivateReason::CleanupIncompleteDeactivation
                    }
                };
                deactivate_ops.push((
                    DeactivateOp { pkg_id, reason },
                    vec![StepCondition::AfterSuccess(activate_step_id)],
                ));
            }
            deactivate_ops.sort_by(|(a, _), (b, _)| a.pkg_id.cmp(&b.pkg_id));
            ops.extend(
                deactivate_ops
                    .into_iter()
                    .map(|(op, conditions)| PlanStep::with_conditions(op, conditions)),
            );
        }
        SimulatedActivationResult::AlreadyActive => ops.push(PlanStep::new(SkipOp {
            pkg_spec: target_id.clone().into(),
            reason: SkipReason::AlreadyActive,
        })),
        SimulatedActivationResult::NotInstalled => unreachable!(),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::{
        engine::{ActivateOp, ActivateReason, DeactivateOp, DeactivateReason, plan},
        util::testing,
    };

    #[test]
    fn emit_install_steps_maps_simulated_results_to_plan_steps() {
        let pkg = Arc::new(testing::make_package_definition("example-font@0.1.0"));

        for (result, expected_step_count, expected_step_id_present, expected_plan) in [
            (
                SimulatedInstallationResult::Installed,
                1,
                true,
                ExecutionPlan::new_for_test([PlanStep::new(InstallOp {
                    pkg: Arc::clone(&pkg),
                    reason: InstallReason::RequestedByUser,
                })]),
            ),
            (
                SimulatedInstallationResult::Reinstalled,
                1,
                true,
                ExecutionPlan::new_for_test([PlanStep::new(InstallOp {
                    pkg: Arc::clone(&pkg),
                    reason: InstallReason::RequestedByUser,
                })]),
            ),
            (
                SimulatedInstallationResult::AlreadyInstalled,
                1,
                false,
                ExecutionPlan::new_for_test([PlanStep::new(SkipOp {
                    pkg_spec: pkg.id.clone().into(),
                    reason: SkipReason::AlreadyInstalled,
                })]),
            ),
        ] {
            let mut ops = vec![];
            let step_id = emit_install_steps(&pkg, result, &mut ops);

            assert_eq!(ops.len(), expected_step_count);
            assert_eq!(step_id.is_some(), expected_step_id_present);
            if let Some(step_id) = step_id {
                assert_eq!(step_id, ops[0].step_id());
            }
            plan::testing::assert_plan_eq(&ExecutionPlan { steps: ops }, &expected_plan);
        }
    }

    #[test]
    fn emit_activation_steps_emits_activate_and_deactivate_steps_from_conflicts() {
        let target_pkg = testing::make_package_definition("example-font@0.4.0");
        let install_step = PlanStep::new(InstallOp {
            pkg: Arc::new(target_pkg.clone()),
            reason: InstallReason::RequestedByUser,
        });
        let dependency = Some(install_step.step_id());
        let incomplete_activation_pkg = testing::make_package_definition("example-font@0.1.0");
        let incomplete_deactivation_pkg = testing::make_package_definition("example-font@0.2.0");
        let active_pkg = testing::make_package_definition("example-font@0.3.0");
        let mut ops = vec![install_step.clone()];

        emit_activation_steps(
            &target_pkg.id,
            dependency,
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
            &mut ops,
        );

        let activate_step = PlanStep::with_conditions(
            ActivateOp {
                pkg_id: target_pkg.id.clone(),
                reason: ActivateReason::RequestedByUser,
            },
            vec![StepCondition::AfterSuccess(install_step.step_id())],
        );
        let activate_step_id = activate_step.step_id();

        plan::testing::assert_plan_eq(
            &ExecutionPlan { steps: ops },
            &ExecutionPlan::new_for_test([
                install_step,
                activate_step,
                PlanStep::with_conditions(
                    DeactivateOp {
                        pkg_id: incomplete_activation_pkg.id.clone(),
                        reason: DeactivateReason::CleanupIncompleteActivation,
                    },
                    vec![StepCondition::AfterSuccess(activate_step_id)],
                ),
                PlanStep::with_conditions(
                    DeactivateOp {
                        pkg_id: incomplete_deactivation_pkg.id.clone(),
                        reason: DeactivateReason::CleanupIncompleteDeactivation,
                    },
                    vec![StepCondition::AfterSuccess(activate_step_id)],
                ),
                PlanStep::with_conditions(
                    DeactivateOp {
                        pkg_id: active_pkg.id.clone(),
                        reason: DeactivateReason::ConflictWithActivation {
                            pkg_id: target_pkg.id.clone(),
                        },
                    },
                    vec![StepCondition::AfterSuccess(activate_step_id)],
                ),
            ]),
        );
    }

    #[test]
    fn emit_activation_steps_emits_skip_for_already_active_result() {
        let pkg = testing::make_package_definition("example-font@0.1.0");
        let mut ops = vec![];

        emit_activation_steps(
            &pkg.id,
            None,
            SimulatedActivationResult::AlreadyActive,
            &mut ops,
        );

        plan::testing::assert_plan_eq(
            &ExecutionPlan { steps: ops },
            &ExecutionPlan::new_for_test([PlanStep::new(SkipOp {
                pkg_spec: pkg.id.clone().into(),
                reason: SkipReason::AlreadyActive,
            })]),
        );
    }

    #[test]
    fn plan_install_activates_already_installed_inactive_package_without_reinstall() {
        let pkg = testing::make_package_definition("example-font@0.1.0");
        let install_target = ResolvedInstallTarget::manifest(pkg.id.clone(), "dummy", true);

        let plan = testing::with_db(|_cx, db| {
            testing::mark_as_installed(db, &pkg);
            plan_install(db, &[install_target])
        });

        plan::testing::assert_plan_eq(
            &plan,
            &ExecutionPlan::new_for_test([
                PlanStep::new(SkipOp {
                    pkg_spec: pkg.id.clone().into(),
                    reason: SkipReason::AlreadyInstalled,
                }),
                PlanStep::new(ActivateOp {
                    pkg_id: pkg.id.clone(),
                    reason: ActivateReason::RequestedByUser,
                }),
            ]),
        );
    }

    #[test]
    fn plan_install_installs_without_activating_when_activation_is_not_requested() {
        let pkg = testing::make_package_definition("example-font@0.1.0");
        let install_target = ResolvedInstallTarget::manifest(pkg.id.clone(), "dummy", false);

        let plan = testing::with_db(|_cx, db| plan_install(db, &[install_target]));

        plan::testing::assert_plan_eq(
            &plan,
            &ExecutionPlan::new_for_test([PlanStep::new(InstallOp {
                pkg: Arc::new(pkg.clone()),
                reason: InstallReason::RequestedByUser,
            })]),
        );
    }

    #[test]
    fn plan_install_activates_newly_installed_package_after_install() {
        let pkg = testing::make_package_definition("example-font@0.1.0");
        let install_target = ResolvedInstallTarget::manifest(pkg.id.clone(), "dummy", true);

        let plan = testing::with_db(|_cx, db| plan_install(db, &[install_target]));

        let install_step = PlanStep::new(InstallOp {
            pkg: Arc::new(pkg.clone()),
            reason: InstallReason::RequestedByUser,
        });
        let install_step_id = install_step.step_id();

        plan::testing::assert_plan_eq(
            &plan,
            &ExecutionPlan::new_for_test([
                install_step,
                PlanStep::with_conditions(
                    ActivateOp {
                        pkg_id: pkg.id.clone(),
                        reason: ActivateReason::RequestedByUser,
                    },
                    vec![StepCondition::AfterSuccess(install_step_id)],
                ),
            ]),
        );
    }
}
