use std::{collections::BTreeSet, marker::PhantomData};

use crate::{
    cli::{
        context::ReportContext,
        reporter::{NeverReport, ReportScope, SubReportScope},
    },
    db::PackageDatabase,
    package::{ActivationState, InstallationState, PackageId, PackageSpec},
};

#[derive(Debug)]
struct RepairResolveScope<S> {
    _base_scope: PhantomData<S>,
}

impl<S> ReportScope for RepairResolveScope<S>
where
    S: ReportScope,
{
    type NoticeReportValue = NeverReport;
    type WarnReportValue = NeverReport;
    type ErrorReportValue = NeverReport;
    type Error = S::Error;
}

impl<S> SubReportScope<S> for RepairResolveScope<S>
where
    S: ReportScope,
{
    fn new() -> Self {
        Self {
            _base_scope: PhantomData,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InstallationRepairKind {
    IncompleteInstall,
    IncompleteUninstall,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActivationRepairKind {
    IncompleteActivation,
    IncompleteDeactivation,
}

#[derive(Debug, Clone, derive_more::IsVariant)]
pub(crate) enum ResolvedRepairTarget {
    NotBroken {
        pkg_spec: PackageSpec,
    },
    Deactivate {
        pkg_id: PackageId,
        kind: ActivationRepairKind,
    },
    Uninstall {
        pkg_id: PackageId,
        kind: InstallationRepairKind,
    },
    AlreadyUninstalled {
        pkg_spec: PackageSpec,
    },
}

pub(crate) fn resolve_all_repair_targets<S>(
    cx: &ReportContext<S>,
    db: &PackageDatabase<'_>,
) -> Vec<ResolvedRepairTarget>
where
    S: ReportScope,
{
    let _cx =
        RepairResolveScope::start_with_report(cx, format_args!("Resolving repair targets..."));
    let mut targets = db
        .entries()
        .filter_map(|entry| match entry.installation_state() {
            InstallationState::Installed => match entry.activation_state() {
                ActivationState::Active | ActivationState::Inactive => None,
                ActivationState::IncompleteActivation => Some(ResolvedRepairTarget::Deactivate {
                    pkg_id: entry.manifest().id(),
                    kind: ActivationRepairKind::IncompleteActivation,
                }),
                ActivationState::IncompleteDeactivation => Some(ResolvedRepairTarget::Deactivate {
                    pkg_id: entry.manifest().id(),
                    kind: ActivationRepairKind::IncompleteDeactivation,
                }),
            },
            InstallationState::IncompleteInstall => Some(ResolvedRepairTarget::Uninstall {
                pkg_id: entry.manifest().id(),
                kind: InstallationRepairKind::IncompleteInstall,
            }),
            InstallationState::IncompleteUninstall => Some(ResolvedRepairTarget::Uninstall {
                pkg_id: entry.manifest().id(),
                kind: InstallationRepairKind::IncompleteUninstall,
            }),
        })
        .collect::<Vec<_>>();
    dedup_targets(&mut targets);
    targets
}

pub(crate) fn resolve_repair_targets<S>(
    cx: &ReportContext<S>,
    db: &PackageDatabase<'_>,
    pkg_specs: &[PackageSpec],
) -> Vec<ResolvedRepairTarget>
where
    S: ReportScope,
{
    let _cx = RepairResolveScope::start_with_report(cx, format_args!("Resolving repair target..."));
    let mut targets = pkg_specs
        .iter()
        .flat_map(|pkg_spec| resolve_spec(db, pkg_spec))
        .collect::<Vec<_>>();
    dedup_targets(&mut targets);
    targets
}

fn resolve_spec(db: &PackageDatabase<'_>, pkg_spec: &PackageSpec) -> Vec<ResolvedRepairTarget> {
    let candidates = db.entries_by_spec(pkg_spec).collect::<Vec<_>>();
    if candidates.is_empty() {
        return vec![ResolvedRepairTarget::AlreadyUninstalled {
            pkg_spec: pkg_spec.clone(),
        }];
    }

    let mut targets = vec![];
    for entry in candidates {
        match entry.installation_state() {
            InstallationState::Installed => match entry.activation_state() {
                ActivationState::Active | ActivationState::Inactive => {}
                ActivationState::IncompleteActivation => {
                    targets.push(ResolvedRepairTarget::Deactivate {
                        pkg_id: entry.manifest().id(),
                        kind: ActivationRepairKind::IncompleteActivation,
                    });
                }
                ActivationState::IncompleteDeactivation => {
                    targets.push(ResolvedRepairTarget::Deactivate {
                        pkg_id: entry.manifest().id(),
                        kind: ActivationRepairKind::IncompleteDeactivation,
                    });
                }
            },
            InstallationState::IncompleteInstall => targets.push(ResolvedRepairTarget::Uninstall {
                pkg_id: entry.manifest().id(),
                kind: InstallationRepairKind::IncompleteInstall,
            }),
            InstallationState::IncompleteUninstall => {
                targets.push(ResolvedRepairTarget::Uninstall {
                    pkg_id: entry.manifest().id(),
                    kind: InstallationRepairKind::IncompleteUninstall,
                });
            }
        }
    }
    if targets.is_empty() {
        return vec![ResolvedRepairTarget::NotBroken {
            pkg_spec: pkg_spec.clone(),
        }];
    }
    targets
}

fn dedup_targets(targets: &mut Vec<ResolvedRepairTarget>) {
    let mut seen_pkg_id = BTreeSet::new();
    let mut seen_pkg_spec = BTreeSet::new();
    targets.retain(|target| match target {
        ResolvedRepairTarget::NotBroken { pkg_spec } => seen_pkg_spec.insert(pkg_spec.clone()),
        ResolvedRepairTarget::Deactivate { pkg_id, .. }
        | ResolvedRepairTarget::Uninstall { pkg_id, .. } => seen_pkg_id.insert(pkg_id.clone()),
        ResolvedRepairTarget::AlreadyUninstalled { pkg_spec } => {
            seen_pkg_spec.insert(pkg_spec.clone())
        }
    });
}

#[cfg(test)]
mod tests {
    use std::{assert_matches, str::FromStr as _};

    use super::*;
    use crate::util::testing;

    fn assert_uninstall_targets_eq(
        targets: &[ResolvedRepairTarget],
        expected: &[(PackageId, InstallationRepairKind)],
    ) {
        let mut actual = targets
            .iter()
            .map(|target| match target {
                ResolvedRepairTarget::Uninstall { pkg_id, kind } => (pkg_id.to_string(), *kind),
                other => panic!("unexpected target: {other:?}"),
            })
            .collect::<Vec<_>>();
        actual.sort_by(|a, b| a.0.cmp(&b.0));

        let mut expected = expected
            .iter()
            .map(|(pkg_id, kind)| (pkg_id.to_string(), *kind))
            .collect::<Vec<_>>();
        expected.sort_by(|a, b| a.0.cmp(&b.0));

        assert_eq!(actual, expected);
    }

    fn assert_deactivate_targets_eq(
        targets: &[ResolvedRepairTarget],
        expected: &[(PackageId, ActivationRepairKind)],
    ) {
        let mut actual = targets
            .iter()
            .map(|target| match target {
                ResolvedRepairTarget::Deactivate { pkg_id, kind } => (pkg_id.to_string(), *kind),
                other => panic!("unexpected target: {other:?}"),
            })
            .collect::<Vec<_>>();
        actual.sort_by(|a, b| a.0.cmp(&b.0));

        let mut expected = expected
            .iter()
            .map(|(pkg_id, kind)| (pkg_id.to_string(), *kind))
            .collect::<Vec<_>>();
        expected.sort_by(|a, b| a.0.cmp(&b.0));

        assert_eq!(actual, expected);
    }

    #[test]
    fn resolve_all_repair_targets_returns_all_broken_entries_only() {
        let incomplete_activation_manifest = testing::make_manifest("example-font@0.1.0");
        let incomplete_deactivation_manifest = testing::make_manifest("example-font@0.2.0");
        let incomplete_install_manifest = testing::make_manifest("other-font@0.1.0");
        let incomplete_uninstall_manifest = testing::make_manifest("other-font@0.2.0");
        let installed_manifest = testing::make_manifest("clean-font@1.0.0");

        testing::with_db(|cx, db| {
            testing::mark_as_installed(db, &incomplete_activation_manifest);
            testing::mark_as_active(db, &incomplete_deactivation_manifest);
            testing::mark_as_incomplete_install(db, &incomplete_install_manifest);
            testing::mark_as_incomplete_uninstall(db, &incomplete_uninstall_manifest);
            testing::mark_as_installed(db, &installed_manifest);

            let mut tx = db.transaction();
            tx.begin_activate(&incomplete_activation_manifest.id())
                .unwrap();
            tx.commit().unwrap();

            let mut tx = db.transaction();
            tx.begin_deactivate(&incomplete_deactivation_manifest.id())
                .unwrap();
            tx.commit().unwrap();

            let targets = resolve_all_repair_targets(cx, db);

            assert_eq!(targets.len(), 4);
            assert_deactivate_targets_eq(
                &targets[..2],
                &[
                    (
                        incomplete_activation_manifest.id(),
                        ActivationRepairKind::IncompleteActivation,
                    ),
                    (
                        incomplete_deactivation_manifest.id(),
                        ActivationRepairKind::IncompleteDeactivation,
                    ),
                ],
            );
            assert_uninstall_targets_eq(
                &targets[2..],
                &[
                    (
                        incomplete_install_manifest.id(),
                        InstallationRepairKind::IncompleteInstall,
                    ),
                    (
                        incomplete_uninstall_manifest.id(),
                        InstallationRepairKind::IncompleteUninstall,
                    ),
                ],
            );
        });
    }

    #[test]
    fn resolve_spec_returns_already_uninstalled_for_missing_specs() {
        let pkg_specs = [
            PackageSpec::from_str("example-font@0.1.0").unwrap(),
            PackageSpec::from_str("example-font").unwrap(),
        ];

        testing::with_db(|_cx, db| {
            for spec in pkg_specs {
                let targets = resolve_spec(db, &spec);
                assert_eq!(targets.len(), 1);
                assert_matches!(
                    &targets[0],
                    ResolvedRepairTarget::AlreadyUninstalled { pkg_spec } if pkg_spec == &spec
                );
            }
        });
    }

    #[test]
    fn resolve_spec_returns_not_broken_for_clean_installed_matches() {
        let manifest = testing::make_manifest("example-font@0.1.0");

        testing::with_db(|_cx, db| {
            testing::mark_as_installed(db, &manifest);

            for spec in [
                PackageSpec::from_str("example-font@0.1.0").unwrap(),
                PackageSpec::from_str("example-font").unwrap(),
            ] {
                let targets = resolve_spec(db, &spec);
                assert_eq!(targets.len(), 1);
                assert_matches!(
                    &targets[0],
                    ResolvedRepairTarget::NotBroken { pkg_spec } if pkg_spec == &spec
                );
            }
        });
    }

    #[test]
    fn resolve_spec_returns_deactivate_targets_for_incomplete_activation_states() {
        let incomplete_activation_manifest = testing::make_manifest("example-font@0.1.0");
        let incomplete_deactivation_manifest = testing::make_manifest("example-font@0.2.0");
        let spec = PackageSpec::from_str("example-font").unwrap();

        testing::with_db(|_cx, db| {
            testing::mark_as_installed(db, &incomplete_activation_manifest);
            testing::mark_as_active(db, &incomplete_deactivation_manifest);

            let mut tx = db.transaction();
            tx.begin_activate(&incomplete_activation_manifest.id())
                .unwrap();
            tx.commit().unwrap();

            let mut tx = db.transaction();
            tx.begin_deactivate(&incomplete_deactivation_manifest.id())
                .unwrap();
            tx.commit().unwrap();

            let targets = resolve_spec(db, &spec);
            assert_eq!(targets.len(), 2);
            assert_deactivate_targets_eq(
                &targets,
                &[
                    (
                        incomplete_activation_manifest.id(),
                        ActivationRepairKind::IncompleteActivation,
                    ),
                    (
                        incomplete_deactivation_manifest.id(),
                        ActivationRepairKind::IncompleteDeactivation,
                    ),
                ],
            );
        });
    }

    #[test]
    fn resolve_repair_targets_returns_all_broken_matches_for_name_and_dedups() {
        let incomplete_activation_manifest = testing::make_manifest("example-font@0.1.0");
        let incomplete_deactivation_manifest = testing::make_manifest("example-font@0.2.0");
        let incomplete_install_manifest = testing::make_manifest("example-font@0.3.0");
        let incomplete_uninstall_manifest = testing::make_manifest("example-font@0.4.0");
        let installed_manifest = testing::make_manifest("example-font@1.0.0");
        let pkg_specs = vec![
            PackageSpec::from_str("example-font").unwrap(),
            PackageSpec::from_str("example-font@0.1.0").unwrap(),
            PackageSpec::from_str("example-font").unwrap(),
        ];

        testing::with_db(|cx, db| {
            testing::mark_as_installed(db, &incomplete_activation_manifest);
            testing::mark_as_active(db, &incomplete_deactivation_manifest);
            testing::mark_as_incomplete_install(db, &incomplete_install_manifest);
            testing::mark_as_incomplete_uninstall(db, &incomplete_uninstall_manifest);
            testing::mark_as_installed(db, &installed_manifest);

            let mut tx = db.transaction();
            tx.begin_activate(&incomplete_activation_manifest.id())
                .unwrap();
            tx.commit().unwrap();

            let mut tx = db.transaction();
            tx.begin_deactivate(&incomplete_deactivation_manifest.id())
                .unwrap();
            tx.commit().unwrap();

            let targets = resolve_repair_targets(cx, db, &pkg_specs);

            assert_eq!(targets.len(), 4);
            assert_deactivate_targets_eq(
                &targets[..2],
                &[
                    (
                        incomplete_activation_manifest.id(),
                        ActivationRepairKind::IncompleteActivation,
                    ),
                    (
                        incomplete_deactivation_manifest.id(),
                        ActivationRepairKind::IncompleteDeactivation,
                    ),
                ],
            );
            assert_uninstall_targets_eq(
                &targets[2..],
                &[
                    (
                        incomplete_install_manifest.id(),
                        InstallationRepairKind::IncompleteInstall,
                    ),
                    (
                        incomplete_uninstall_manifest.id(),
                        InstallationRepairKind::IncompleteUninstall,
                    ),
                ],
            );
        });
    }
}
