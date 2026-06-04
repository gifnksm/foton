use crate::engine::{
    ExecutionPlan, PlanStep, RepairKind, ResolvedRepairTarget, SkipOp, SkipReason, UninstallOp,
    UninstallReason,
};

pub(crate) fn plan_repair(targets: &[ResolvedRepairTarget]) -> ExecutionPlan {
    let mut ops = vec![];
    for target in targets {
        match target {
            ResolvedRepairTarget::NotBroken { pkg_spec } => ops.push(PlanStep::new(SkipOp {
                pkg_spec: pkg_spec.clone(),
                reason: SkipReason::NotBroken,
            })),
            ResolvedRepairTarget::Uninstall { pkg_id, kind } => {
                let reason = match kind {
                    RepairKind::IncompleteInstall => UninstallReason::RepairIncompleteInstall,
                    RepairKind::IncompleteUninstall => UninstallReason::RepairIncompleteUninstall,
                };
                ops.push(PlanStep::new(UninstallOp {
                    pkg_id: pkg_id.clone(),
                    reason,
                }));
            }
            ResolvedRepairTarget::AlreadyUninstalled { pkg_spec } => {
                ops.push(PlanStep::new(SkipOp {
                    pkg_spec: pkg_spec.clone(),
                    reason: SkipReason::NothingToRepair,
                }));
            }
        }
    }
    ExecutionPlan { steps: ops }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr as _;

    use super::*;
    use crate::{
        package::{PackageId, PackageSpec},
        util::testing,
    };

    #[test]
    fn plan_repair_uninstalls_incomplete_entries() {
        let incomplete_install_manifest = testing::make_manifest("example-font@0.1.0");
        let incomplete_uninstall_manifest = testing::make_manifest("example-font@0.2.0");
        let incomplete_install_pkg_id = incomplete_install_manifest.id();
        let incomplete_uninstall_pkg_id = incomplete_uninstall_manifest.id();
        let targets = vec![
            ResolvedRepairTarget::Uninstall {
                pkg_id: incomplete_install_pkg_id.clone(),
                kind: RepairKind::IncompleteInstall,
            },
            ResolvedRepairTarget::Uninstall {
                pkg_id: incomplete_uninstall_pkg_id.clone(),
                kind: RepairKind::IncompleteUninstall,
            },
        ];

        let plan = plan_repair(&targets);

        testing::assert_plan_eq(
            &plan,
            &ExecutionPlan::new_for_test([
                PlanStep::new(UninstallOp {
                    pkg_id: incomplete_install_pkg_id,
                    reason: UninstallReason::RepairIncompleteInstall,
                }),
                PlanStep::new(UninstallOp {
                    pkg_id: incomplete_uninstall_pkg_id,
                    reason: UninstallReason::RepairIncompleteUninstall,
                }),
            ]),
        );
    }

    #[test]
    fn plan_repair_skips_not_broken_targets() {
        let pkg_spec = PackageSpec::from_str("example-font").unwrap();
        let target = ResolvedRepairTarget::NotBroken {
            pkg_spec: pkg_spec.clone(),
        };

        let plan = plan_repair(&[target]);

        testing::assert_plan_eq(
            &plan,
            &ExecutionPlan::new_for_test([PlanStep::new(SkipOp {
                pkg_spec,
                reason: SkipReason::NotBroken,
            })]),
        );
    }

    #[test]
    fn plan_repair_skips_targets_with_nothing_to_repair() {
        let missing_pkg_id = PackageId::from_str("example-font@0.1.0").unwrap();
        let missing_pkg_spec = PackageSpec::from_str("other-font").unwrap();
        let targets = vec![
            ResolvedRepairTarget::AlreadyUninstalled {
                pkg_spec: missing_pkg_id.clone().into(),
            },
            ResolvedRepairTarget::AlreadyUninstalled {
                pkg_spec: missing_pkg_spec.clone(),
            },
        ];

        let plan = plan_repair(&targets);

        testing::assert_plan_eq(
            &plan,
            &ExecutionPlan::new_for_test([
                PlanStep::new(SkipOp {
                    pkg_spec: missing_pkg_id.into(),
                    reason: SkipReason::NothingToRepair,
                }),
                PlanStep::new(SkipOp {
                    pkg_spec: missing_pkg_spec,
                    reason: SkipReason::NothingToRepair,
                }),
            ]),
        );
    }
}
