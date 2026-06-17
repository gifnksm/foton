use crate::engine::{
    DeactivateOp, DeactivateReason, ExecutionPlan, PlanStep, ResolvedDeactivateTarget, SkipOp,
    SkipReason,
};

pub(crate) fn plan_deactivate(targets: &[ResolvedDeactivateTarget]) -> ExecutionPlan {
    let mut steps = vec![];
    for target in targets {
        match target {
            ResolvedDeactivateTarget::Deactivate { pkg_id } => {
                steps.push(PlanStep::new(DeactivateOp {
                    pkg_id: pkg_id.clone(),
                    reason: DeactivateReason::RequestedByUser,
                }));
            }
            ResolvedDeactivateTarget::AlreadyInactive { pkg_spec } => {
                steps.push(PlanStep::new(SkipOp {
                    pkg_spec: pkg_spec.clone(),
                    reason: SkipReason::AlreadyInactive,
                }));
            }
        }
    }
    ExecutionPlan { steps }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr as _;

    use crate::{engine::plan, package::PackageSpec, util::testing};

    use super::*;

    #[test]
    fn plan_deactivate_emits_deactivate_step_for_requested_target() {
        let pkg = testing::make_package_definition("example-font@0.2.0");
        let target = ResolvedDeactivateTarget::Deactivate {
            pkg_id: pkg.id.clone(),
        };

        let plan = plan_deactivate(&[target]);

        plan::testing::assert_plan_eq(
            &plan,
            &ExecutionPlan::new_for_test([PlanStep::new(DeactivateOp {
                pkg_id: pkg.id.clone(),
                reason: DeactivateReason::RequestedByUser,
            })]),
        );
    }

    #[test]
    fn plan_deactivate_skips_already_inactive_target() {
        let pkg = testing::make_package_definition("example-font@0.2.0");
        let target = ResolvedDeactivateTarget::AlreadyInactive {
            pkg_spec: pkg.id.clone().into(),
        };

        let plan = plan_deactivate(&[target]);

        plan::testing::assert_plan_eq(
            &plan,
            &ExecutionPlan::new_for_test([PlanStep::new(SkipOp {
                pkg_spec: pkg.id.clone().into(),
                reason: SkipReason::AlreadyInactive,
            })]),
        );
    }

    #[test]
    fn plan_deactivate_skips_missing_target_using_original_spec() {
        let missing_spec = PackageSpec::from_str("example-font").unwrap();
        let target = ResolvedDeactivateTarget::AlreadyInactive {
            pkg_spec: missing_spec.clone(),
        };

        let plan = plan_deactivate(&[target]);

        plan::testing::assert_plan_eq(
            &plan,
            &ExecutionPlan::new_for_test([PlanStep::new(SkipOp {
                pkg_spec: missing_spec,
                reason: SkipReason::AlreadyInactive,
            })]),
        );
    }
}
