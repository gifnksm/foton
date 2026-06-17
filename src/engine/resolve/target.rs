use std::{path::PathBuf, sync::Arc};

use crate::{
    package::{PackageDefinition, PackageId, PackageSpec},
    registry::RegistryId,
};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum InstallTargetKind {
    AlreadyInstalled { pkg_id: PackageId },
    Install { pkg: Arc<PackageDefinition> },
}

impl InstallTargetKind {
    pub(crate) fn pkg_id(&self) -> &PackageId {
        match self {
            Self::AlreadyInstalled { pkg_id } => pkg_id,
            Self::Install { pkg } => &pkg.id,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, derive_more::Display)]
pub(crate) enum InstallResolutionDetail {
    #[display("package registry {reg_id}: {pkg_spec}")]
    Registry {
        reg_id: RegistryId,
        pkg_spec: PackageSpec,
    },
    #[display("installed package: {pkg_spec}")]
    Installed { pkg_spec: PackageSpec },
    #[display("manifest file: {}", path.display())]
    ManifestFile { path: PathBuf },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct ResolvedInstallTarget {
    pub(in crate::engine) kind: InstallTargetKind,
    pub(in crate::engine) detail: InstallResolutionDetail,
    pub(in crate::engine) should_activate: bool,
}

impl ResolvedInstallTarget {
    #[cfg(test)]
    pub(in crate::engine) fn manifest<I, P>(pkg_id: I, path: P, should_activate: bool) -> Self
    where
        I: TryInto<PackageId, Error: std::fmt::Debug>,
        P: Into<PathBuf>,
    {
        Self {
            kind: InstallTargetKind::Install {
                pkg: Arc::new(crate::util::testing::make_package_definition(pkg_id)),
            },
            detail: InstallResolutionDetail::ManifestFile { path: path.into() },
            should_activate,
        }
    }

    #[cfg(test)]
    pub(in crate::engine) fn registry<I, R, S>(
        pkg_id: I,
        reg_id: R,
        pkg_spec: S,
        should_activate: bool,
    ) -> Self
    where
        I: TryInto<PackageId, Error: std::fmt::Debug>,
        R: TryInto<RegistryId, Error: std::fmt::Debug>,
        S: TryInto<PackageSpec, Error: std::fmt::Debug>,
    {
        Self {
            kind: InstallTargetKind::Install {
                pkg: Arc::new(crate::util::testing::make_package_definition(pkg_id)),
            },
            detail: InstallResolutionDetail::Registry {
                reg_id: reg_id.try_into().unwrap(),
                pkg_spec: pkg_spec.try_into().unwrap(),
            },
            should_activate,
        }
    }

    #[cfg(test)]
    pub(in crate::engine) fn already_installed<I, S>(
        pkg_id: I,
        pkg_spec: S,
        should_activate: bool,
    ) -> Self
    where
        I: TryInto<PackageId, Error: std::fmt::Debug>,
        S: TryInto<PackageSpec, Error: std::fmt::Debug>,
    {
        Self {
            kind: InstallTargetKind::AlreadyInstalled {
                pkg_id: pkg_id.try_into().unwrap(),
            },
            detail: InstallResolutionDetail::Installed {
                pkg_spec: pkg_spec.try_into().unwrap(),
            },
            should_activate,
        }
    }
}

#[derive(Debug, Clone, derive_more::IsVariant)]
pub(crate) enum ResolvedUninstallTarget {
    Uninstall {
        pkg_id: PackageId,
        should_deactivate: bool,
    },
    AlreadyUninstalled {
        pkg_spec: PackageSpec,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct ResolvedActivateTarget {
    pub(crate) pkg_id: PackageId,
}

#[derive(Debug, Clone)]
pub(crate) enum ResolvedDeactivateTarget {
    Deactivate { pkg_id: PackageId },
    AlreadyInactive { pkg_spec: PackageSpec },
}
