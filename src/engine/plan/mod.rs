use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use crate::package::{PackageId, PackageManifest, PackageSpec};

pub(crate) use self::{install::*, repair::*, uninstall::*};

mod install;
mod repair;
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

    #[cfg(test)]
    pub(crate) fn exec_id(&self) -> Option<ExecutionId> {
        match self {
            ExecutionPlanOp::Install(op) => Some(op.exec_id),
            ExecutionPlanOp::Uninstall(op) => Some(op.exec_id),
            ExecutionPlanOp::Skip(_) => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct ExecutionId(u64);

impl ExecutionId {
    pub(crate) fn new() -> Self {
        static ID: AtomicU64 = AtomicU64::new(0);
        let id = ID.fetch_add(1, Ordering::Relaxed);
        Self(id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ExecutionCondition {
    AfterSuccess(ExecutionId),
    Never,
}

#[derive(Debug, Clone)]
pub(crate) struct InstallOp {
    pub(crate) exec_id: ExecutionId,
    pub(crate) manifest: Arc<PackageManifest>,
    pub(crate) reason: InstallReason,
    pub(crate) replacing_pkg_ids: Vec<PackageId>,
}

#[derive(Debug, Clone)]
pub(crate) struct UninstallOp {
    pub(crate) exec_id: ExecutionId,
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
    #[display("cleanup after incomplete install")]
    CleanupIncompleteInstall,
    #[display("cleanup after incomplete uninstall")]
    CleanupIncompleteUninstall,
}

#[derive(Debug, Clone, derive_more::Display, PartialEq, Eq)]
pub(crate) enum SkipReason {
    #[display("already installed")]
    AlreadyInstalled,
    #[display("already uninstalled")]
    AlreadyUninstalled,
    #[display("not broken")]
    NotBroken,
    #[display("already repaired")]
    AlreadyRepaired,
}
