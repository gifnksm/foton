use crate::{
    db::{PackageDatabase, Uninstallability},
    engine::{
        ExecutionPlan, PlanStep, ResolvedUninstallTarget, SkipOp, SkipReason, UninstallOp,
        UninstallReason,
    },
};

pub(crate) fn plan_uninstall(
    db: &PackageDatabase<'_>,
    targets: &[ResolvedUninstallTarget],
) -> ExecutionPlan {
    let mut ops = vec![];
    for target in targets {
        match target {
            ResolvedUninstallTarget::Uninstall { pkg_id } => {
                match db.check_uninstallability(pkg_id) {
                    Uninstallability::Uninstallable => ops.push(PlanStep::new(UninstallOp {
                        pkg_id: pkg_id.clone(),
                        reason: UninstallReason::RequestedByUser,
                    })),
                    Uninstallability::AlreadyUninstalled => ops.push(PlanStep::new(SkipOp {
                        pkg_spec: pkg_id.clone().into(),
                        reason: SkipReason::AlreadyUninstalled,
                    })),
                }
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
        cli::reporter::RootReportScope as _,
        package::PackageId,
        util::testing::{self, TempdirContext, TestScope},
    };

    use super::*;

    #[test]
    fn plan_uninstall_uninstalls_specified_version_only() {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);

        let other_manifest = testing::make_manifest("example-font@0.1.0");
        let uninstall_manifest = testing::make_manifest("example-font@0.2.0");
        let uninstall_pkg_id = uninstall_manifest.id();
        let uninstall_target = testing::make_resolved_uninstall_target(&uninstall_pkg_id);

        let plan = testing::with_db(&cx, |mut db| {
            testing::mark_as_installed(&mut db, &other_manifest);
            testing::mark_as_installed(&mut db, &uninstall_manifest);
            plan_uninstall(&db, &[uninstall_target])
        });

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
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);
        let uninstall_pkg_id = PackageId::from_str("example-font@0.1.0").unwrap();
        let uninstall_target = testing::make_resolved_uninstall_target(&uninstall_pkg_id);

        let plan = testing::with_db(&cx, |db| plan_uninstall(&db, &[uninstall_target]));

        testing::assert_plan_eq(
            &plan,
            &ExecutionPlan::new_for_test([PlanStep::new(SkipOp {
                pkg_spec: uninstall_pkg_id.into(),
                reason: SkipReason::AlreadyUninstalled,
            })]),
        );
    }
}
