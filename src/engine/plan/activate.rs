use crate::{
    db::{ActivationConflict, PackageDatabase, SimulatedActivationResult, SimulatedPackageDbState},
    engine::{
        ActivateOp, ActivateReason, DeactivateOp, DeactivateReason, ExecutionPlan, PlanStep,
        ResolvedActivateTarget, SkipOp, SkipReason, StepCondition,
    },
    package::PackageId,
};

pub(crate) fn plan_activate(
    db: &PackageDatabase<'_>,
    targets: &[ResolvedActivateTarget],
) -> ExecutionPlan {
    let mut db = db.simulated_state();
    let mut steps = vec![];
    for target in targets {
        plan_activate_op(&mut db, &target.pkg_id, &mut steps);
    }
    ExecutionPlan { steps }
}

fn plan_activate_op(
    db: &mut SimulatedPackageDbState,
    target_id: &PackageId,
    steps: &mut Vec<PlanStep>,
) {
    let result = db.activate(target_id);
    emit_activation_steps(target_id, result, steps);
}

fn emit_activation_steps(
    target_id: &PackageId,
    result: SimulatedActivationResult,
    steps: &mut Vec<PlanStep>,
) {
    match result {
        SimulatedActivationResult::Activated { deactivated } => {
            let activate_op = PlanStep::new(ActivateOp {
                pkg_id: target_id.clone(),
                reason: ActivateReason::RequestedByUser,
            });
            let activate_step_id = activate_op.step_id;
            steps.push(activate_op);
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
            steps.extend(
                deactivate_ops
                    .into_iter()
                    .map(|(op, conditions)| PlanStep::with_conditions(op, conditions)),
            );
        }
        SimulatedActivationResult::AlreadyActive => steps.push(PlanStep::new(SkipOp {
            pkg_spec: target_id.clone().into(),
            reason: SkipReason::AlreadyActive,
        })),
        SimulatedActivationResult::NotInstalled => unreachable!(),
    }
}

#[cfg(test)]
mod tests {
    use crate::{engine::plan, util::testing};

    use super::*;

    #[test]
    fn emit_activation_steps_emits_activate_and_cleanup_steps_from_conflicts() {
        let target_pkg = testing::make_package_definition("example-font@0.4.0");
        let incomplete_activation_pkg = testing::make_package_definition("example-font@0.1.0");
        let incomplete_deactivation_pkg = testing::make_package_definition("example-font@0.2.0");
        let active_pkg = testing::make_package_definition("example-font@0.3.0");
        let mut steps = vec![];

        emit_activation_steps(
            &target_pkg.id,
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
            &mut steps,
        );

        let activate_step = PlanStep::new(ActivateOp {
            pkg_id: target_pkg.id.clone(),
            reason: ActivateReason::RequestedByUser,
        });
        let activate_step_id = activate_step.step_id();

        plan::testing::assert_plan_eq(
            &ExecutionPlan { steps },
            &ExecutionPlan::new_for_test([
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
        let mut steps = vec![];

        emit_activation_steps(
            &pkg.id,
            SimulatedActivationResult::AlreadyActive,
            &mut steps,
        );

        plan::testing::assert_plan_eq(
            &ExecutionPlan { steps },
            &ExecutionPlan::new_for_test([PlanStep::new(SkipOp {
                pkg_spec: pkg.id.clone().into(),
                reason: SkipReason::AlreadyActive,
            })]),
        );
    }

    #[test]
    fn plan_activate_uses_simulated_state_to_skip_redundant_second_activation() {
        let pkg = testing::make_package_definition("example-font@0.1.0");
        let targets = vec![
            ResolvedActivateTarget {
                pkg_id: pkg.id.clone(),
            },
            ResolvedActivateTarget {
                pkg_id: pkg.id.clone(),
            },
        ];

        let plan = testing::with_db(|_cx, db| {
            testing::mark_as_installed(db, &pkg);
            plan_activate(db, &targets)
        });

        plan::testing::assert_plan_eq(
            &plan,
            &ExecutionPlan::new_for_test([
                PlanStep::new(ActivateOp {
                    pkg_id: pkg.id.clone(),
                    reason: ActivateReason::RequestedByUser,
                }),
                PlanStep::new(SkipOp {
                    pkg_spec: pkg.id.clone().into(),
                    reason: SkipReason::AlreadyActive,
                }),
            ]),
        );
    }
}
