use super::{
    ActivationRepairKind, InstallationRepairKind, ResolvedRepairTarget, ResolvedUninstallTarget,
};
use crate::package::{PackageId, PackageSpec};

#[derive(Debug)]
pub(in crate::engine::resolve) enum ExpectedRepairTarget {
    Deactivate(PackageId, ActivationRepairKind),
    Uninstall(PackageId, InstallationRepairKind),
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
