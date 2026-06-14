use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use crate::package::{PackageDefinition, PackageId, PackageSpec};

pub(crate) use self::{install::*, repair::*, uninstall::*};

mod install;
mod repair;
#[cfg(test)]
mod testing;
mod uninstall;

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ExecutionPlan {
    steps: Vec<PlanStep>,
}

impl ExecutionPlan {
    pub(in crate::engine) fn steps(&self) -> &[PlanStep] {
        &self.steps
    }

    pub(in crate::engine) fn into_steps(self) -> Vec<PlanStep> {
        self.steps
    }

    pub(crate) fn has_side_effects(&self) -> bool {
        self.steps.iter().any(|step| !step.op.is_skip())
    }

    #[cfg(test)]
    pub(in crate::engine) fn new_for_test<I>(ops: I) -> Self
    where
        I: IntoIterator<Item = PlanStep>,
    {
        Self {
            steps: ops.into_iter().collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::engine) struct PlanStep {
    step_id: StepId,
    op: PlanStepOp,
    conditions: Vec<StepCondition>,
}

impl PlanStep {
    pub(in crate::engine) fn new<O>(op: O) -> Self
    where
        O: Into<PlanStepOp>,
    {
        Self::with_conditions(op, vec![])
    }

    pub(in crate::engine) fn with_conditions<O>(op: O, conditions: Vec<StepCondition>) -> Self
    where
        O: Into<PlanStepOp>,
    {
        Self {
            step_id: StepId::new(),
            op: op.into(),
            conditions,
        }
    }

    pub(in crate::engine) fn step_id(&self) -> StepId {
        self.step_id
    }

    pub(in crate::engine) fn into_op(self) -> PlanStepOp {
        self.op
    }

    pub(in crate::engine) fn op(&self) -> &PlanStepOp {
        &self.op
    }

    pub(in crate::engine) fn can_execute(&self) -> bool {
        self.conditions.is_empty()
    }

    pub(in crate::engine) fn notify_result(&mut self, step_id: StepId, result: StepResult) {
        self.conditions.retain_mut(|condition| match condition {
            StepCondition::AfterSuccess(cond_id) if *cond_id == step_id => match result {
                StepResult::Success => false,
                StepResult::Failure => {
                    *condition = StepCondition::Never;
                    true
                }
            },
            StepCondition::AfterSuccess(_) | StepCondition::Never => true,
        });
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(in crate::engine) struct StepId(u64);

impl StepId {
    fn new() -> Self {
        static ID: AtomicU64 = AtomicU64::new(0);
        let id = ID.fetch_add(1, Ordering::Relaxed);
        Self(id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, derive_more::IsVariant, derive_more::From)]
pub(in crate::engine) enum PlanStepOp {
    Install(InstallOp),
    Uninstall(UninstallOp),
    Activate(ActivateOp),
    Deactivate(DeactivateOp),
    Skip(SkipOp),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, derive_more::IsVariant)]
pub(in crate::engine) enum StepResult {
    Success,
    Failure,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::engine) enum StepCondition {
    AfterSuccess(StepId),
    Never,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::engine) struct InstallOp {
    pub(in crate::engine) pkg: Arc<PackageDefinition>,
    pub(in crate::engine) reason: InstallReason,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::engine) struct UninstallOp {
    pub(in crate::engine) pkg_id: PackageId,
    pub(in crate::engine) reason: UninstallReason,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::engine) struct ActivateOp {
    pub(in crate::engine) pkg_id: PackageId,
    pub(in crate::engine) reason: ActivateReason,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::engine) struct DeactivateOp {
    pub(in crate::engine) pkg_id: PackageId,
    pub(in crate::engine) reason: DeactivateReason,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::engine) struct SkipOp {
    pub(in crate::engine) pkg_spec: PackageSpec,
    pub(in crate::engine) reason: SkipReason,
}

#[derive(Debug, Clone, PartialEq, Eq, derive_more::Display)]
pub(in crate::engine) enum InstallReason {
    #[display("requested by user")]
    RequestedByUser,
}

#[derive(Debug, Clone, PartialEq, Eq, derive_more::Display)]
pub(in crate::engine) enum UninstallReason {
    #[display("requested by user")]
    RequestedByUser,
    #[display("repairing incomplete install")]
    RepairIncompleteInstall,
    #[display("repairing incomplete uninstall")]
    RepairIncompleteUninstall,
}

#[derive(Debug, Clone, PartialEq, Eq, derive_more::Display)]
pub(in crate::engine) enum ActivateReason {
    #[display("requested by user")]
    RequestedByUser,
}

#[derive(Debug, Clone, PartialEq, Eq, derive_more::Display)]
pub(in crate::engine) enum DeactivateReason {
    #[display("requested by user")]
    RequestedByUser,
    #[display("conflicts with package {pkg_id} activation")]
    ConflictWithActivation { pkg_id: PackageId },
    #[display("cleanup after incomplete activation")]
    CleanupIncompleteActivation,
    #[display("cleanup after incomplete deactivation")]
    CleanupIncompleteDeactivation,
    #[display("repairing incomplete activation")]
    RepairIncompleteActivation,
    #[display("repairing incomplete deactivation")]
    RepairIncompleteDeactivation,
}

#[derive(Debug, Clone, PartialEq, Eq, derive_more::Display)]
pub(in crate::engine) enum SkipReason {
    #[display("already installed")]
    AlreadyInstalled,
    #[display("already uninstalled")]
    AlreadyUninstalled,
    #[display("already active")]
    AlreadyActive,
    #[display("not broken")]
    NotBroken,
    #[display("nothing to repair")]
    NothingToRepair,
}
