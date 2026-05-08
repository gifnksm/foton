use crate::{
    db::{PackageDatabase, Uninstallability},
    engine::{
        ExecutionPlan, ResolvedUninstallTarget, SkipOp, SkipReason, UninstallOp, UninstallReason,
    },
};

pub(crate) fn plan_uninstall(
    db: &PackageDatabase<'_>,
    targets: &[ResolvedUninstallTarget],
) -> ExecutionPlan {
    let mut ops = vec![];
    for target in targets {
        let pkg_id = &target.pkg_id;
        match db.check_uninstallability(pkg_id) {
            Uninstallability::Uninstallable => ops.push(
                UninstallOp {
                    pkg_id: pkg_id.clone(),
                    reason: UninstallReason::RequestedByUser,
                }
                .into(),
            ),
            Uninstallability::AlreadyUninstalled => ops.push(
                SkipOp {
                    pkg_id: pkg_id.clone(),
                    reason: SkipReason::AlreadyUninstalled,
                }
                .into(),
            ),
        }
    }
    ExecutionPlan { ops }
}

#[cfg(test)]
mod tests {
    use crate::{
        cli::reporter::RootReportScope as _,
        util::testing::{self, TempdirContext, TestScope},
    };

    use super::*;

    #[test]
    fn plan_uninstall_uninstalls_specified_version_only() {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);

        let other_manifest = testing::make_manifest("example-namespace/example-font@0.1.0");
        let uninstall_manifest = testing::make_manifest("example-namespace/example-font@0.2.0");
        let uninstall_pkg_id = uninstall_manifest.metadata.id();
        let uninstall_target = testing::make_resolved_uninstall_target(&uninstall_pkg_id);

        let plan = testing::with_db(&cx, |mut db| {
            testing::mark_as_installed(&mut db, &other_manifest);
            testing::mark_as_installed(&mut db, &uninstall_manifest);
            plan_uninstall(&db, &[uninstall_target])
        });

        testing::assert_plan_eq(
            &plan,
            &ExecutionPlan::new_for_test([UninstallOp {
                pkg_id: uninstall_pkg_id,
                reason: UninstallReason::RequestedByUser,
            }
            .into()]),
        );
    }

    #[test]
    fn plan_uninstall_skips_non_existing() {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);
        let uninstall_target =
            testing::make_resolved_uninstall_target("example-namespace/example-font@0.1.0");
        let uninstall_pkg_id = uninstall_target.pkg_id.clone();

        let plan = testing::with_db(&cx, |db| plan_uninstall(&db, &[uninstall_target]));

        testing::assert_plan_eq(
            &plan,
            &ExecutionPlan::new_for_test([SkipOp {
                pkg_id: uninstall_pkg_id.clone(),
                reason: SkipReason::AlreadyUninstalled,
            }
            .into()]),
        );
    }
}
