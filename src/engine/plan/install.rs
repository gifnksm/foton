use std::sync::Arc;

use crate::{
    db::{Installability, PackageDatabase},
    engine::{
        ExecutionPlan, InstallOp, InstallReason, PlanStep, ResolvedInstallTarget, SkipOp,
        StepCondition, UninstallOp, UninstallReason, plan::SkipReason,
    },
    package::{PackageId, PackageManifest},
};

pub(crate) fn plan_install(
    db: &PackageDatabase<'_>,
    targets: &[ResolvedInstallTarget],
) -> ExecutionPlan {
    let mut ops = vec![];
    for target in targets {
        let manifest = &target.manifest;
        let target_id = manifest.id();
        match db.check_installability(manifest) {
            Installability::Installable {
                installed_other_versions,
                incomplete_installs,
                incomplete_uninstalls,
            } => {
                let install_op = install_target_op(manifest, installed_other_versions.clone());
                let install_step_id = install_op.step_id;
                ops.push(install_op);
                let mut uninstall_ops = vec![];
                for pkg_id in installed_other_versions {
                    uninstall_ops.push((
                        UninstallOp {
                            pkg_id,
                            reason: UninstallReason::ConflictWithInstall {
                                pkg_id: target_id.clone(),
                            },
                        },
                        vec![StepCondition::AfterSuccess(install_step_id)],
                    ));
                }
                for pkg_id in incomplete_uninstalls {
                    uninstall_ops.push((
                        UninstallOp {
                            pkg_id,
                            reason: UninstallReason::CleanupIncompleteUninstall,
                        },
                        vec![],
                    ));
                }
                for pkg_id in incomplete_installs {
                    uninstall_ops.push((
                        UninstallOp {
                            pkg_id,
                            reason: UninstallReason::CleanupIncompleteInstall,
                        },
                        vec![],
                    ));
                }
                uninstall_ops.sort_by(|(a, _), (b, _)| a.pkg_id.cmp(&b.pkg_id));
                ops.extend(
                    uninstall_ops
                        .into_iter()
                        .map(|(op, conditions)| PlanStep::with_conditions(op, conditions)),
                );
            }
            Installability::AlreadyInstalled => ops.push(PlanStep::new(SkipOp {
                pkg_spec: target_id.into(),
                reason: SkipReason::AlreadyInstalled,
            })),
        }
    }
    ExecutionPlan { steps: ops }
}

fn install_target_op(
    manifest: &Arc<PackageManifest>,
    replacing_pkg_ids: Vec<PackageId>,
) -> PlanStep {
    PlanStep::new(InstallOp {
        manifest: Arc::clone(manifest),
        reason: InstallReason::RequestedByUser,
        replacing_pkg_ids,
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::{
        cli::reporter::RootReportScope as _,
        package::InstallationState,
        util::testing::{self, TempdirContext, TestScope},
    };

    #[test]
    fn plan_install_uninstalls_other_versions() {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);

        let incomplete_install_manifest = testing::make_manifest("example-font@0.1.0");
        let incomplete_uninstall_manifest = testing::make_manifest("example-font@0.2.0");
        let installed_manifest = testing::make_manifest("example-font@0.3.0");
        let install_target_manifest = testing::make_manifest("example-font@0.4.0");
        let install_target = testing::make_resolved_install_target(&install_target_manifest);

        let plan = testing::with_db(&cx, |mut db| {
            testing::mark_as_incomplete_install(&mut db, &incomplete_install_manifest);
            testing::mark_as_incomplete_uninstall(&mut db, &incomplete_uninstall_manifest);
            testing::mark_as_installed(&mut db, &installed_manifest);
            plan_install(&db, &[install_target])
        });

        let install_step = PlanStep::new(InstallOp {
            manifest: Arc::clone(&install_target_manifest),
            reason: InstallReason::RequestedByUser,
            replacing_pkg_ids: vec![installed_manifest.id()],
        });
        let install_step_id = install_step.step_id;

        testing::assert_plan_eq(
            &plan,
            &ExecutionPlan::new_for_test([
                install_step,
                PlanStep::new(UninstallOp {
                    pkg_id: incomplete_install_manifest.id(),
                    reason: UninstallReason::CleanupIncompleteInstall,
                }),
                PlanStep::new(UninstallOp {
                    pkg_id: incomplete_uninstall_manifest.id(),
                    reason: UninstallReason::CleanupIncompleteUninstall,
                }),
                PlanStep::with_conditions(
                    UninstallOp {
                        pkg_id: installed_manifest.id(),
                        reason: UninstallReason::ConflictWithInstall {
                            pkg_id: install_target_manifest.id(),
                        },
                    },
                    vec![StepCondition::AfterSuccess(install_step_id)],
                ),
            ]),
        );
    }

    #[test]
    fn plan_install_skips_already_installed() {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);

        let manifest = testing::make_manifest("example-font@0.1.0");
        let install_target = testing::make_resolved_install_target(&manifest);

        let plan = testing::with_db(&cx, |mut db| {
            testing::mark_as_installed(&mut db, &manifest);
            plan_install(&db, &[install_target])
        });

        testing::assert_plan_eq(
            &plan,
            &ExecutionPlan::new_for_test([PlanStep::new(SkipOp {
                pkg_spec: manifest.id().into(),
                reason: SkipReason::AlreadyInstalled,
            })]),
        );
    }

    #[test]
    fn plan_install_overwrites_same_version_incomplete_states() {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);

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
            let install_target = testing::make_resolved_install_target(&manifest);

            let plan = testing::with_db(&cx, |mut db| {
                testing::mark_as_state(&mut db, &manifest, state);
                plan_install(&db, &[install_target])
            });
            testing::assert_plan_eq(
                &plan,
                &ExecutionPlan::new_for_test([PlanStep::new(InstallOp {
                    manifest: Arc::clone(&manifest),
                    reason: InstallReason::RequestedByUser,
                    replacing_pkg_ids: vec![],
                })]),
            );
        }
    }
}
