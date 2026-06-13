use std::{path::PathBuf, sync::Arc};

use crate::{
    package::{PackageDefinition, PackageId, PackageSpec},
    registry::RegistryId,
};

#[derive(Debug, Clone, derive_more::Display)]
pub(crate) enum InstallTargetSource {
    #[display("package registry {_0}")]
    Registry(RegistryId),
    #[display("installed package")]
    Installed,
    #[display("manifest file: {}", _0.display())]
    File(PathBuf),
}

#[derive(Debug, Clone)]
pub(crate) struct ResolvedInstallTarget {
    pub(in crate::engine) source: InstallTargetSource,
    pub(in crate::engine) pkg: Arc<PackageDefinition>,
    pub(in crate::engine) should_activate: bool,
}

impl ResolvedInstallTarget {
    #[cfg(test)]
    pub(crate) fn installed(pkg: &Arc<PackageDefinition>, should_activate: bool) -> Self {
        Self {
            source: InstallTargetSource::Installed,
            pkg: Arc::clone(pkg),
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

impl ResolvedUninstallTarget {
    #[cfg(test)]
    pub(crate) fn uninstall(pkg_id: PackageId, should_deactivate: bool) -> Self {
        Self::Uninstall {
            pkg_id,
            should_deactivate,
        }
    }

    #[cfg(test)]
    pub(crate) fn already_uninstalled(pkg_spec: PackageSpec) -> Self {
        Self::AlreadyUninstalled { pkg_spec }
    }
}
