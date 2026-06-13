use std::sync::Arc;

use crate::package::{PackageDefinition, PackageId, PackageSpec};

#[derive(Debug, Clone)]
pub(crate) struct ResolvedInstallTarget {
    pub(in crate::engine) pkg: Arc<PackageDefinition>,
    pub(in crate::engine) should_activate: bool,
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
