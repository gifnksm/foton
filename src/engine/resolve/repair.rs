use std::{collections::BTreeSet, marker::PhantomData};

use crate::{
    cli::{
        context::ReportContext,
        reporter::{NeverReport, ReportScope, SubReportScope},
    },
    db::PackageDatabase,
    package::{InstallationState, PackageId, PackageSpec},
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
pub(crate) enum RepairKind {
    IncompleteInstall,
    IncompleteUninstall,
}

#[derive(Debug, Clone, derive_more::IsVariant)]
pub(crate) enum ResolvedRepairTarget {
    NotBroken { pkg_spec: PackageSpec },
    Uninstall { pkg_id: PackageId, kind: RepairKind },
    AlreadyUninstalled { pkg_spec: PackageSpec },
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
        .filter_map(|entry| match entry.installation_state {
            InstallationState::Installed => None,
            InstallationState::IncompleteInstall => Some(ResolvedRepairTarget::Uninstall {
                pkg_id: entry.manifest.id(),
                kind: RepairKind::IncompleteInstall,
            }),
            InstallationState::IncompleteUninstall => Some(ResolvedRepairTarget::Uninstall {
                pkg_id: entry.manifest.id(),
                kind: RepairKind::IncompleteUninstall,
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
        match entry.installation_state {
            InstallationState::Installed => {}
            InstallationState::IncompleteInstall => targets.push(ResolvedRepairTarget::Uninstall {
                pkg_id: entry.manifest.id(),
                kind: RepairKind::IncompleteInstall,
            }),
            InstallationState::IncompleteUninstall => {
                targets.push(ResolvedRepairTarget::Uninstall {
                    pkg_id: entry.manifest.id(),
                    kind: RepairKind::IncompleteUninstall,
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
        ResolvedRepairTarget::Uninstall { pkg_id, .. } => seen_pkg_id.insert(pkg_id.clone()),
        ResolvedRepairTarget::AlreadyUninstalled { pkg_spec } => {
            seen_pkg_spec.insert(pkg_spec.clone())
        }
    });
}

#[cfg(test)]
mod tests {
    use std::{assert_matches, str::FromStr as _};

    use super::*;
    use crate::{
        cli::reporter::RootReportScope as _,
        util::testing::{self, TempdirContext, TestScope},
    };

    fn assert_uninstall_targets_eq(
        targets: &[ResolvedRepairTarget],
        expected: &[(PackageId, RepairKind)],
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

    #[test]
    fn resolve_all_repair_targets_returns_all_incomplete_entries_only() {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);

        let incomplete_install_manifest = testing::make_manifest("example-font@0.1.0");
        let incomplete_uninstall_manifest = testing::make_manifest("example-font@0.2.0");
        let installed_manifest = testing::make_manifest("other-font@1.0.0");

        testing::with_db(&cx, |mut db| {
            testing::mark_as_incomplete_install(&mut db, &incomplete_install_manifest);
            testing::mark_as_incomplete_uninstall(&mut db, &incomplete_uninstall_manifest);
            testing::mark_as_installed(&mut db, &installed_manifest);

            let targets = resolve_all_repair_targets(&cx, &db);

            assert_eq!(targets.len(), 2);
            assert_uninstall_targets_eq(
                &targets,
                &[
                    (
                        incomplete_install_manifest.id(),
                        RepairKind::IncompleteInstall,
                    ),
                    (
                        incomplete_uninstall_manifest.id(),
                        RepairKind::IncompleteUninstall,
                    ),
                ],
            );
        });
    }

    #[test]
    fn resolve_spec_returns_already_uninstalled_for_missing_specs() {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);

        testing::with_db(&cx, |db| {
            let pkg_specs = [
                PackageSpec::from_str("example-font@0.1.0").unwrap(),
                PackageSpec::from_str("example-font").unwrap(),
            ];

            for spec in pkg_specs {
                let targets = resolve_spec(&db, &spec);
                assert_eq!(targets.len(), 1);
                assert_matches!(
                    &targets[0],
                    ResolvedRepairTarget::AlreadyUninstalled { pkg_spec } if pkg_spec == &spec
                );
            }
        });
    }

    #[test]
    fn resolve_spec_returns_not_broken_for_installed_only_matches() {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);

        let manifest = testing::make_manifest("example-font@0.1.0");

        testing::with_db(&cx, |mut db| {
            testing::mark_as_installed(&mut db, &manifest);

            for spec in [
                PackageSpec::from_str("example-font@0.1.0").unwrap(),
                PackageSpec::from_str("example-font").unwrap(),
            ] {
                let targets = resolve_spec(&db, &spec);
                assert_eq!(targets.len(), 1);
                assert_matches!(
                    &targets[0],
                    ResolvedRepairTarget::NotBroken { pkg_spec } if pkg_spec == &spec
                );
            }
        });
    }

    #[test]
    fn resolve_repair_targets_returns_all_incomplete_matches_for_name_and_dedups() {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);

        let incomplete_install_manifest = testing::make_manifest("example-font@0.1.0");
        let incomplete_uninstall_manifest = testing::make_manifest("example-font@0.2.0");
        let installed_manifest = testing::make_manifest("example-font@1.0.0");
        let pkg_specs = vec![
            PackageSpec::from_str("example-font").unwrap(),
            PackageSpec::from_str("example-font@0.1.0").unwrap(),
            PackageSpec::from_str("example-font").unwrap(),
        ];

        testing::with_db(&cx, |mut db| {
            testing::mark_as_incomplete_install(&mut db, &incomplete_install_manifest);
            testing::mark_as_incomplete_uninstall(&mut db, &incomplete_uninstall_manifest);
            testing::mark_as_installed(&mut db, &installed_manifest);

            let targets = resolve_repair_targets(&cx, &db, &pkg_specs);

            assert_eq!(targets.len(), 2);
            assert_uninstall_targets_eq(
                &targets,
                &[
                    (
                        incomplete_install_manifest.id(),
                        RepairKind::IncompleteInstall,
                    ),
                    (
                        incomplete_uninstall_manifest.id(),
                        RepairKind::IncompleteUninstall,
                    ),
                ],
            );
        });
    }
}
