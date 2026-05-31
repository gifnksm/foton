use std::sync::Arc;

use crate::package::{PackageId, PackageManifest, PackageSpec};

pub(crate) use self::{install::*, uninstall::*};

mod install;
mod uninstall;

#[derive(Debug)]
pub(crate) struct ExecutionPlan {
    ops: Vec<ExecutionPlanOp>,
}

impl ExecutionPlan {
    pub(crate) fn ops(&self) -> &[ExecutionPlanOp] {
        &self.ops
    }

    pub(crate) fn has_side_effects(&self) -> bool {
        self.ops.iter().any(|op| !op.is_skip())
    }

    #[cfg(test)]
    pub(crate) fn new_for_test<I>(ops: I) -> Self
    where
        I: IntoIterator<Item = ExecutionPlanOp>,
    {
        Self {
            ops: ops.into_iter().collect(),
        }
    }
}

#[derive(Debug, Clone, derive_more::IsVariant, derive_more::From)]
pub(crate) enum ExecutionPlanOp {
    Install(InstallOp),
    Uninstall(UninstallOp),
    Skip(SkipOp),
}

impl ExecutionPlanOp {
    pub(crate) fn as_install(&self) -> Option<&InstallOp> {
        match self {
            Self::Install(op) => Some(op),
            _ => None,
        }
    }

    pub(crate) fn as_uninstall(&self) -> Option<&UninstallOp> {
        match self {
            Self::Uninstall(op) => Some(op),
            _ => None,
        }
    }

    pub(crate) fn as_skip(&self) -> Option<&SkipOp> {
        match self {
            Self::Skip(op) => Some(op),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ExecutionCondition {
    AfterSuccess(PackageId),
    Never,
}

#[derive(Debug, Clone)]
pub(crate) struct InstallOp {
    pub(crate) manifest: Arc<PackageManifest>,
    pub(crate) reason: InstallReason,
    pub(crate) replacing_pkg_ids: Vec<PackageId>,
}

#[derive(Debug, Clone)]
pub(crate) struct UninstallOp {
    pub(crate) pkg_id: PackageId,
    pub(crate) reason: UninstallReason,
    pub(crate) conditions: Vec<ExecutionCondition>,
}

#[derive(Debug, Clone)]
pub(crate) struct SkipOp {
    pub(crate) pkg_spec: PackageSpec,
    pub(crate) reason: SkipReason,
}

#[derive(Debug, Clone, derive_more::Display, PartialEq, Eq)]
pub(crate) enum InstallReason {
    #[display("requested by user")]
    RequestedByUser,
}

#[derive(Debug, Clone, derive_more::Display, PartialEq, Eq)]
pub(crate) enum UninstallReason {
    #[display("requested by user")]
    RequestedByUser,
    #[display("conflicts with package {pkg_id}")]
    ConflictWithInstall { pkg_id: PackageId },
    #[display("cleanup after failed install")]
    CleanupPendingInstall,
    #[display("cleanup after failed uninstall")]
    CleanupPendingUninstall,
}

#[derive(Debug, Clone, derive_more::Display, PartialEq, Eq)]
pub(crate) enum SkipReason {
    #[display("already installed")]
    AlreadyInstalled,
    #[display("already uninstalled")]
    AlreadyUninstalled,
}
