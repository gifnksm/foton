use crate::engine::{
    DeactivateOp, DeactivateReason, ExecutionPlan, PlanStep, ResolvedUninstallTarget, SkipOp,
    SkipReason, StepCondition, UninstallOp, UninstallReason,
};

pub(crate) fn plan_uninstall(targets: &[ResolvedUninstallTarget]) -> ExecutionPlan {
    let mut ops = vec![];
    for target in targets {
        match target {
            ResolvedUninstallTarget::Uninstall {
                pkg_id,
                should_deactivate,
            } => {
                let mut conditions = vec![];
                if *should_deactivate {
                    let step = PlanStep::new(DeactivateOp {
                        pkg_id: pkg_id.clone(),
                        reason: DeactivateReason::RequestedByUser,
                    });
                    let step_id = step.step_id;
                    ops.push(step);
                    conditions.push(StepCondition::AfterSuccess(step_id));
                }
                let uninstall_op = UninstallOp {
                    pkg_id: pkg_id.clone(),
                    reason: UninstallReason::RequestedByUser,
                };
                ops.push(PlanStep::with_conditions(uninstall_op, conditions));
            }
            ResolvedUninstallTarget::AlreadyUninstalled { pkg_spec } => {
                ops.push(PlanStep::new(SkipOp {
                    pkg_spec: pkg_spec.clone(),
                    reason: SkipReason::AlreadyUninstalled,
                }));
            }
        }
    }
    ExecutionPlan { steps: ops }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr as _;

    use crate::{
        engine::plan,
        package::{PackageId, PackageSpec},
        util::testing,
    };

    use super::*;

    #[test]
    fn plan_uninstall_deactivates_then_uninstalls_when_requested() {
        let uninstall_manifest = testing::make_manifest("example-font@0.2.0");
        let uninstall_pkg_id = uninstall_manifest.id();
        let uninstall_target = testing::make_resolved_uninstall_target(&uninstall_pkg_id);

        let plan = plan_uninstall(&[uninstall_target]);

        let deactivate_step = PlanStep::new(DeactivateOp {
            pkg_id: uninstall_pkg_id.clone(),
            reason: DeactivateReason::RequestedByUser,
        });
        let deactivate_step_id = deactivate_step.step_id();

        plan::testing::assert_plan_eq(
            &plan,
            &ExecutionPlan::new_for_test([
                deactivate_step,
                PlanStep::with_conditions(
                    UninstallOp {
                        pkg_id: uninstall_pkg_id,
                        reason: UninstallReason::RequestedByUser,
                    },
                    vec![StepCondition::AfterSuccess(deactivate_step_id)],
                ),
            ]),
        );
    }

    #[test]
    fn plan_uninstall_uninstalls_without_deactivating_when_not_requested() {
        let uninstall_manifest = testing::make_manifest("example-font@0.2.0");
        let uninstall_pkg_id = uninstall_manifest.id();
        let uninstall_target =
            testing::make_resolved_uninstall_target_with_deactivation(&uninstall_pkg_id, false);

        let plan = plan_uninstall(&[uninstall_target]);

        plan::testing::assert_plan_eq(
            &plan,
            &ExecutionPlan::new_for_test([PlanStep::new(UninstallOp {
                pkg_id: uninstall_pkg_id,
                reason: UninstallReason::RequestedByUser,
            })]),
        );
    }

    #[test]
    fn plan_uninstall_skips_non_existing() {
        let uninstall_pkg_id = PackageId::from_str("example-font@0.1.0").unwrap();
        let uninstall_target = ResolvedUninstallTarget::AlreadyUninstalled {
            pkg_spec: PackageSpec::from(uninstall_pkg_id.clone()),
        };

        let plan = plan_uninstall(&[uninstall_target]);

        plan::testing::assert_plan_eq(
            &plan,
            &ExecutionPlan::new_for_test([PlanStep::new(SkipOp {
                pkg_spec: uninstall_pkg_id.into(),
                reason: SkipReason::AlreadyUninstalled,
            })]),
        );
    }
}
