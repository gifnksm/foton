use crate::{
    db::{PackageDatabase, Uninstallability},
    engine::{
        ExecutionPlan, ResolvedRepairTarget, SkipOp, SkipReason, UninstallOp, UninstallReason,
    },
};

pub(crate) fn plan_repair(
    db: &PackageDatabase<'_>,
    targets: &[ResolvedRepairTarget],
) -> ExecutionPlan {
    let mut ops = vec![];
    for target in targets {
        match target {
            ResolvedRepairTarget::NotBroken { pkg_spec } => ops.push(
                SkipOp {
                    pkg_spec: pkg_spec.clone(),
                    reason: SkipReason::NotBroken,
                }
                .into(),
            ),
            ResolvedRepairTarget::Uninstall { pkg_id } => match db.check_uninstallability(pkg_id) {
                Uninstallability::Uninstallable => ops.push(
                    UninstallOp {
                        pkg_id: pkg_id.clone(),
                        reason: UninstallReason::RequestedByUser,
                        conditions: vec![],
                    }
                    .into(),
                ),
                Uninstallability::AlreadyUninstalled => ops.push(
                    SkipOp {
                        pkg_spec: pkg_id.clone().into(),
                        reason: SkipReason::AlreadyRepaired,
                    }
                    .into(),
                ),
            },
            ResolvedRepairTarget::AlreadyUninstalled { pkg_spec } => {
                ops.push(
                    SkipOp {
                        pkg_spec: pkg_spec.clone(),
                        reason: SkipReason::AlreadyRepaired,
                    }
                    .into(),
                );
            }
        }
    }
    ExecutionPlan { ops }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr as _;

    use super::*;
    use crate::{
        cli::reporter::RootReportScope as _,
        package::{PackageId, PackageSpec},
        util::testing::{self, TempdirContext, TestScope},
    };

    #[test]
    fn plan_repair_uninstalls_pending_entries() {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);

        let pending_install_manifest = testing::make_manifest("example-font@0.1.0");
        let pending_uninstall_manifest = testing::make_manifest("example-font@0.2.0");
        let pending_install_pkg_id = pending_install_manifest.id();
        let pending_uninstall_pkg_id = pending_uninstall_manifest.id();
        let targets = vec![
            ResolvedRepairTarget::Uninstall {
                pkg_id: pending_install_pkg_id.clone(),
            },
            ResolvedRepairTarget::Uninstall {
                pkg_id: pending_uninstall_pkg_id.clone(),
            },
        ];

        let plan = testing::with_db(&cx, |mut db| {
            testing::mark_as_pending_installed(&mut db, &pending_install_manifest);
            testing::mark_as_pending_uninstalled(&mut db, &pending_uninstall_manifest);
            plan_repair(&db, &targets)
        });

        testing::assert_plan_eq(
            &plan,
            &ExecutionPlan::new_for_test([
                UninstallOp {
                    pkg_id: pending_install_pkg_id,
                    reason: UninstallReason::RequestedByUser,
                    conditions: vec![],
                }
                .into(),
                UninstallOp {
                    pkg_id: pending_uninstall_pkg_id,
                    reason: UninstallReason::RequestedByUser,
                    conditions: vec![],
                }
                .into(),
            ]),
        );
    }

    #[test]
    fn plan_repair_skips_not_broken_targets() {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);

        let pkg_spec = PackageSpec::from_str("example-font").unwrap();
        let target = ResolvedRepairTarget::NotBroken {
            pkg_spec: pkg_spec.clone(),
        };

        let plan = testing::with_db(&cx, |db| plan_repair(&db, &[target]));

        testing::assert_plan_eq(
            &plan,
            &ExecutionPlan::new_for_test([SkipOp {
                pkg_spec,
                reason: SkipReason::NotBroken,
            }
            .into()]),
        );
    }

    #[test]
    fn plan_repair_skips_already_repaired_targets() {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);

        let missing_pkg_id = PackageId::from_str("example-font@0.1.0").unwrap();
        let missing_pkg_spec = PackageSpec::from_str("other-font").unwrap();
        let targets = vec![
            ResolvedRepairTarget::Uninstall {
                pkg_id: missing_pkg_id.clone(),
            },
            ResolvedRepairTarget::AlreadyUninstalled {
                pkg_spec: missing_pkg_spec.clone(),
            },
        ];

        let plan = testing::with_db(&cx, |db| plan_repair(&db, &targets));

        testing::assert_plan_eq(
            &plan,
            &ExecutionPlan::new_for_test([
                SkipOp {
                    pkg_spec: missing_pkg_id.into(),
                    reason: SkipReason::AlreadyRepaired,
                }
                .into(),
                SkipOp {
                    pkg_spec: missing_pkg_spec,
                    reason: SkipReason::AlreadyRepaired,
                }
                .into(),
            ]),
        );
    }
}
