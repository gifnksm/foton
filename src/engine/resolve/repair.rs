use std::{collections::BTreeSet, marker::PhantomData};

use crate::{
    cli::{
        context::ReportContext,
        reporter::{NeverReport, ReportScope, SubReportScope},
    },
    db::PackageDatabase,
    package::{ActivationState, InstallationState, PackageId, PackageSpec},
};

#[derive(Debug, Default)]
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

impl<S> SubReportScope<S> for RepairResolveScope<S> where S: ReportScope {}

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
        .all_entries()
        .filter_map(|entry| match entry.installation_state() {
            InstallationState::Installed => match entry.activation_state() {
                ActivationState::Active | ActivationState::Inactive => None,
                ActivationState::IncompleteActivation => Some(ResolvedRepairTarget::Deactivate {
                    pkg_id: entry.id().clone(),
                    kind: ActivationRepairKind::IncompleteActivation,
                }),
                ActivationState::IncompleteDeactivation => Some(ResolvedRepairTarget::Deactivate {
                    pkg_id: entry.id().clone(),
                    kind: ActivationRepairKind::IncompleteDeactivation,
                }),
            },
            InstallationState::IncompleteInstall => Some(ResolvedRepairTarget::Uninstall {
                pkg_id: entry.id().clone(),
                kind: InstallationRepairKind::IncompleteInstall,
            }),
            InstallationState::IncompleteUninstall => Some(ResolvedRepairTarget::Uninstall {
                pkg_id: entry.id().clone(),
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
                        pkg_id: entry.id().clone(),
                        kind: ActivationRepairKind::IncompleteActivation,
                    });
                }
                ActivationState::IncompleteDeactivation => {
                    targets.push(ResolvedRepairTarget::Deactivate {
                        pkg_id: entry.id().clone(),
                        kind: ActivationRepairKind::IncompleteDeactivation,
                    });
                }
            },
            InstallationState::IncompleteInstall => targets.push(ResolvedRepairTarget::Uninstall {
                pkg_id: entry.id().clone(),
                kind: InstallationRepairKind::IncompleteInstall,
            }),
            InstallationState::IncompleteUninstall => {
                targets.push(ResolvedRepairTarget::Uninstall {
                    pkg_id: entry.id().clone(),
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
    use std::{assert_matches, str::FromStr as _, sync::Arc};

    use super::*;
    use crate::{
        engine::resolve::{self, testing::ExpectedRepairTarget},
        package::PackageDefinition,
        util::testing,
    };

    #[expect(clippy::struct_field_names)]
    #[derive(Debug)]
    struct BrokenRepairFixture {
        incomplete_activation_pkg: Arc<PackageDefinition>,
        incomplete_deactivation_pkg: Arc<PackageDefinition>,
        incomplete_install_pkg: Arc<PackageDefinition>,
        incomplete_uninstall_pkg: Arc<PackageDefinition>,
        installed_pkg: Arc<PackageDefinition>,
    }

    impl BrokenRepairFixture {
        fn mixed_names() -> Self {
            Self {
                incomplete_activation_pkg: testing::make_package_definition("example-font@0.1.0"),
                incomplete_deactivation_pkg: testing::make_package_definition("example-font@0.2.0"),
                incomplete_install_pkg: testing::make_package_definition("other-font@0.1.0"),
                incomplete_uninstall_pkg: testing::make_package_definition("other-font@0.2.0"),
                installed_pkg: testing::make_package_definition("clean-font@1.0.0"),
            }
        }

        fn same_name(name: &str) -> Self {
            Self {
                incomplete_activation_pkg: testing::make_package_definition(format!(
                    "{name}@0.1.0"
                )),
                incomplete_deactivation_pkg: testing::make_package_definition(format!(
                    "{name}@0.2.0"
                )),
                incomplete_install_pkg: testing::make_package_definition(format!("{name}@0.3.0")),
                incomplete_uninstall_pkg: testing::make_package_definition(format!("{name}@0.4.0")),
                installed_pkg: testing::make_package_definition(format!("{name}@1.0.0")),
            }
        }
    }

    fn seed_broken_repair_entries(db: &mut PackageDatabase<'_>, fixture: &BrokenRepairFixture) {
        testing::mark_as_installed(db, &fixture.incomplete_activation_pkg);
        testing::mark_as_active(db, &fixture.incomplete_deactivation_pkg);
        testing::mark_as_incomplete_install(db, &fixture.incomplete_install_pkg);
        testing::mark_as_incomplete_uninstall(db, &fixture.incomplete_uninstall_pkg);
        testing::mark_as_installed(db, &fixture.installed_pkg);

        begin_incomplete_activation(db, &fixture.incomplete_activation_pkg.id);
        begin_incomplete_deactivation(db, &fixture.incomplete_deactivation_pkg.id);
    }

    fn begin_incomplete_activation(db: &mut PackageDatabase<'_>, pkg_id: &PackageId) {
        let mut tx = db.transaction();
        tx.begin_activate(pkg_id).unwrap();
        tx.commit().unwrap();
    }

    fn begin_incomplete_deactivation(db: &mut PackageDatabase<'_>, pkg_id: &PackageId) {
        let mut tx = db.transaction();
        tx.begin_deactivate(pkg_id).unwrap();
        tx.commit().unwrap();
    }

    #[test]
    fn resolve_all_repair_targets_returns_all_broken_entries_only() {
        let fixture = BrokenRepairFixture::mixed_names();

        testing::with_db(|cx, db| {
            seed_broken_repair_entries(db, &fixture);

            let targets = resolve_all_repair_targets(cx, db);

            resolve::testing::assert_repair_targets_eq(
                &targets,
                &[
                    ExpectedRepairTarget::Deactivate(
                        fixture.incomplete_activation_pkg.id.clone(),
                        ActivationRepairKind::IncompleteActivation,
                    ),
                    ExpectedRepairTarget::Deactivate(
                        fixture.incomplete_deactivation_pkg.id.clone(),
                        ActivationRepairKind::IncompleteDeactivation,
                    ),
                    ExpectedRepairTarget::Uninstall(
                        fixture.incomplete_install_pkg.id.clone(),
                        InstallationRepairKind::IncompleteInstall,
                    ),
                    ExpectedRepairTarget::Uninstall(
                        fixture.incomplete_uninstall_pkg.id.clone(),
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
        let pkg = testing::make_package_definition("example-font@0.1.0");

        testing::with_db(|_cx, db| {
            testing::mark_as_installed(db, &pkg);

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
        let fixture = BrokenRepairFixture::same_name("example-font");
        let spec = PackageSpec::from_str("example-font").unwrap();

        testing::with_db(|_cx, db| {
            testing::mark_as_installed(db, &fixture.incomplete_activation_pkg);
            testing::mark_as_active(db, &fixture.incomplete_deactivation_pkg);
            begin_incomplete_activation(db, &fixture.incomplete_activation_pkg.id);
            begin_incomplete_deactivation(db, &fixture.incomplete_deactivation_pkg.id);

            let targets = resolve_spec(db, &spec);
            resolve::testing::assert_repair_targets_eq(
                &targets,
                &[
                    ExpectedRepairTarget::Deactivate(
                        fixture.incomplete_activation_pkg.id.clone(),
                        ActivationRepairKind::IncompleteActivation,
                    ),
                    ExpectedRepairTarget::Deactivate(
                        fixture.incomplete_deactivation_pkg.id.clone(),
                        ActivationRepairKind::IncompleteDeactivation,
                    ),
                ],
            );
        });
    }

    #[test]
    fn resolve_repair_targets_returns_all_broken_matches_for_name_and_dedups() {
        let fixture = BrokenRepairFixture::same_name("example-font");
        let pkg_specs = vec![
            PackageSpec::from_str("example-font").unwrap(),
            PackageSpec::from_str("example-font@0.1.0").unwrap(),
            PackageSpec::from_str("example-font").unwrap(),
        ];

        testing::with_db(|cx, db| {
            seed_broken_repair_entries(db, &fixture);

            let targets = resolve_repair_targets(cx, db, &pkg_specs);

            resolve::testing::assert_repair_targets_eq(
                &targets,
                &[
                    ExpectedRepairTarget::Deactivate(
                        fixture.incomplete_activation_pkg.id.clone(),
                        ActivationRepairKind::IncompleteActivation,
                    ),
                    ExpectedRepairTarget::Deactivate(
                        fixture.incomplete_deactivation_pkg.id.clone(),
                        ActivationRepairKind::IncompleteDeactivation,
                    ),
                    ExpectedRepairTarget::Uninstall(
                        fixture.incomplete_install_pkg.id.clone(),
                        InstallationRepairKind::IncompleteInstall,
                    ),
                    ExpectedRepairTarget::Uninstall(
                        fixture.incomplete_uninstall_pkg.id.clone(),
                        InstallationRepairKind::IncompleteUninstall,
                    ),
                ],
            );
        });
    }
}
