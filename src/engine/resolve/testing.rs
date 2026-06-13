use super::{
    ActivationRepairKind, InstallationRepairKind, ResolvedInstallTarget, ResolvedRepairTarget,
    ResolvedUninstallTarget,
};
use crate::package::{PackageId, PackageSpec};

#[derive(Debug)]
pub(in crate::engine::resolve) enum ExpectedRepairTarget {
    Deactivate(PackageId, ActivationRepairKind),
    Uninstall(PackageId, InstallationRepairKind),
}

pub(in crate::engine::resolve) fn assert_install_target_id(
    target: &ResolvedInstallTarget,
    expected: &str,
) {
    assert_eq!(target.pkg.id.to_string(), expected);
}

pub(in crate::engine::resolve) fn assert_install_target_ids(
    targets: &[ResolvedInstallTarget],
    expected: &[&str],
) {
    let actual = targets
        .iter()
        .map(|target| target.pkg.id.to_string())
        .collect::<Vec<_>>();
    let expected = expected.iter().map(ToString::to_string).collect::<Vec<_>>();
    assert_eq!(actual, expected);
}

pub(in crate::engine::resolve) fn assert_single_install_target_id(
    targets: &[ResolvedInstallTarget],
    expected_id: &str,
) {
    assert_eq!(targets.len(), 1);
    assert_install_target_ids(targets, &[expected_id]);
}

pub(in crate::engine::resolve) fn assert_single_install_target(
    targets: &[ResolvedInstallTarget],
    expected_id: &str,
    should_activate: bool,
) {
    assert_single_install_target_id(targets, expected_id);
    assert_eq!(targets[0].should_activate, should_activate);
}

pub(in crate::engine::resolve) fn assert_uninstall_target(
    target: &ResolvedUninstallTarget,
    expected_pkg_id: &PackageId,
    should_deactivate: bool,
) {
    assert!(matches!(
        target,
        ResolvedUninstallTarget::Uninstall { pkg_id, should_deactivate: actual }
            if pkg_id == expected_pkg_id && *actual == should_deactivate
    ));
}

pub(in crate::engine::resolve) fn assert_already_uninstalled(
    target: &ResolvedUninstallTarget,
    expected_spec: &PackageSpec,
) {
    assert!(matches!(
        target,
        ResolvedUninstallTarget::AlreadyUninstalled { pkg_spec } if pkg_spec == expected_spec
    ));
}

pub(in crate::engine::resolve) fn assert_repair_targets_eq(
    targets: &[ResolvedRepairTarget],
    expected: &[ExpectedRepairTarget],
) {
    assert_eq!(targets.len(), expected.len());

    for (actual, expected) in targets.iter().zip(expected) {
        match (actual, expected) {
            (
                ResolvedRepairTarget::Deactivate { pkg_id, kind },
                ExpectedRepairTarget::Deactivate(expected_pkg_id, expected_kind),
            ) => {
                assert_eq!(pkg_id, expected_pkg_id);
                assert_eq!(kind, expected_kind);
            }
            (
                ResolvedRepairTarget::Uninstall { pkg_id, kind },
                ExpectedRepairTarget::Uninstall(expected_pkg_id, expected_kind),
            ) => {
                assert_eq!(pkg_id, expected_pkg_id);
                assert_eq!(kind, expected_kind);
            }
            (actual, expected) => {
                panic!("unexpected target mismatch: actual={actual:?}, expected={expected:?}")
            }
        }
    }
}
