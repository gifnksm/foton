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
    steps: Vec<PlanStep>,
}

impl ExecutionPlan {
    pub(crate) fn steps(&self) -> &[PlanStep] {
        &self.steps
    }

    pub(crate) fn has_side_effects(&self) -> bool {
        self.steps.iter().any(|step| !step.op.is_skip())
    }

    #[cfg(test)]
    pub(crate) fn new_for_test<I>(ops: I) -> Self
    where
        I: IntoIterator<Item = PlanStep>,
    {
        Self {
            steps: ops.into_iter().collect(),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct PlanStep {
    step_id: StepId,
    op: PlanStepOp,
    conditions: Vec<StepCondition>,
}

impl PlanStep {
    pub(crate) fn new<O>(op: O) -> Self
    where
        O: Into<PlanStepOp>,
    {
        Self::with_conditions(op, vec![])
    }

    pub(crate) fn with_conditions<O>(op: O, conditions: Vec<StepCondition>) -> Self
    where
        O: Into<PlanStepOp>,
    {
        Self {
            step_id: StepId::new(),
            op: op.into(),
            conditions,
        }
    }

    pub(crate) fn step_id(&self) -> StepId {
        self.step_id
    }

    pub(crate) fn op(&self) -> &PlanStepOp {
        &self.op
    }

    pub(crate) fn can_execute(&self) -> bool {
        self.conditions.is_empty()
    }

    pub(crate) fn conditions(&self) -> &[StepCondition] {
        &self.conditions
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct StepId(u64);

impl StepId {
    pub(crate) fn new() -> Self {
        static ID: AtomicU64 = AtomicU64::new(0);
        let id = ID.fetch_add(1, Ordering::Relaxed);
        Self(id)
    }
}

#[derive(Debug, Clone, derive_more::IsVariant, derive_more::From)]
pub(crate) enum PlanStepOp {
    Install(InstallOp),
    Uninstall(UninstallOp),
    Skip(SkipOp),
}

impl PlanStepOp {
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
pub(crate) enum StepCondition {
    AfterSuccess(StepId),
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
