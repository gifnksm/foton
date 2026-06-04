use crate::engine::{
    ExecutionPlan, PlanStep, ResolvedUninstallTarget, SkipOp, SkipReason, UninstallOp,
    UninstallReason,
};

pub(crate) fn plan_uninstall(targets: &[ResolvedUninstallTarget]) -> ExecutionPlan {
    let mut ops = vec![];
    for target in targets {
        match target {
            ResolvedUninstallTarget::Uninstall { pkg_id } => {
                ops.push(PlanStep::new(UninstallOp {
                    pkg_id: pkg_id.clone(),
                    reason: UninstallReason::RequestedByUser,
                }));
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
        package::{PackageId, PackageSpec},
        util::testing,
    };

    use super::*;

    #[test]
    fn plan_uninstall_uninstalls_specified_version_only() {
        let uninstall_manifest = testing::make_manifest("example-font@0.2.0");
        let uninstall_pkg_id = uninstall_manifest.id();
        let uninstall_target = testing::make_resolved_uninstall_target(&uninstall_pkg_id);

        let plan = plan_uninstall(&[uninstall_target]);

        testing::assert_plan_eq(
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

        testing::assert_plan_eq(
            &plan,
            &ExecutionPlan::new_for_test([PlanStep::new(SkipOp {
                pkg_spec: uninstall_pkg_id.into(),
                reason: SkipReason::AlreadyUninstalled,
            })]),
        );
    }
}
