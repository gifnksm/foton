use std::{collections::HashMap, sync::Arc};

use crate::engine::{
    ActivateOp, DeactivateOp, ExecutionPlan, InstallOp, PlanStep, PlanStepOp, SkipOp,
    StepCondition, StepId, UninstallOp,
};

#[track_caller]
pub(super) fn assert_plan_eq(actual: &ExecutionPlan, expected: &ExecutionPlan) {
    let ExecutionPlan {
        steps: actual_steps,
    } = actual;
    let ExecutionPlan {
        steps: expected_steps,
    } = expected;
    assert_eq!(
        expected_steps.len(),
        actual_steps.len(),
        "plan step count mismatch:\n  actual: {actual_steps:?}\nexpected: {expected_steps:?}",
    );
    let step_id_map = actual_steps
        .iter()
        .zip(expected_steps)
        .map(|(actual_step, expected_step)| (actual_step.step_id, expected_step.step_id))
        .collect::<HashMap<_, _>>();
    for (actual_step, expected_step) in actual_steps.iter().zip(expected_steps) {
        let PlanStep {
            step_id: _,
            op: actual_op,
            conditions: actual_conditions,
        } = actual_step;
        let PlanStep {
            step_id: _,
            op: expected_op,
            conditions: expected_conditions,
        } = expected_step;
        assert_conditions_eq(&step_id_map, actual_conditions, expected_conditions);
        match (actual_op, expected_op) {
            (
                PlanStepOp::Install(InstallOp {
                    manifest: actual_manifest,
                    reason: actual_reason,
                }),
                PlanStepOp::Install(InstallOp {
                    manifest: expected_manifest,
                    reason: expected_reason,
                }),
            ) => {
                assert!(Arc::ptr_eq(actual_manifest, expected_manifest));
                assert_eq!(actual_reason, expected_reason);
            }
            (
                PlanStepOp::Uninstall(UninstallOp {
                    pkg_id: actual_pkg_id,
                    reason: actual_reason,
                }),
                PlanStepOp::Uninstall(UninstallOp {
                    pkg_id: expected_pkg_id,
                    reason: expected_reason,
                }),
            ) => {
                assert_eq!(actual_pkg_id, expected_pkg_id);
                assert_eq!(actual_reason, expected_reason);
            }
            (
                PlanStepOp::Activate(ActivateOp {
                    pkg_id: actual_pkg_id,
                    reason: actual_reason,
                }),
                PlanStepOp::Activate(ActivateOp {
                    pkg_id: expected_pkg_id,
                    reason: expected_reason,
                }),
            ) => {
                assert_eq!(actual_pkg_id, expected_pkg_id);
                assert_eq!(actual_reason, expected_reason);
            }
            (
                PlanStepOp::Deactivate(DeactivateOp {
                    pkg_id: actual_pkg_id,
                    reason: actual_reason,
                }),
                PlanStepOp::Deactivate(DeactivateOp {
                    pkg_id: expected_pkg_id,
                    reason: expected_reason,
                }),
            ) => {
                assert_eq!(actual_pkg_id, expected_pkg_id);
                assert_eq!(actual_reason, expected_reason);
            }
            (
                PlanStepOp::Skip(SkipOp {
                    pkg_spec: actual_pkg_spec,
                    reason: actual_reason,
                }),
                PlanStepOp::Skip(SkipOp {
                    pkg_spec: expected_pkg_spec,
                    reason: expected_reason,
                }),
            ) => {
                assert_eq!(actual_pkg_spec, expected_pkg_spec);
                assert_eq!(actual_reason, expected_reason);
            }
            (actual_op, expected_op) => {
                panic!("mismatched plan ops\n  actual: {actual_op:?}\nexpected: {expected_op:?}")
            }
        }
    }
}

fn assert_conditions_eq(
    step_id_map: &HashMap<StepId, StepId>,
    actual_conditions: &[StepCondition],
    expected_conditions: &[StepCondition],
) {
    let converted_actual_conditions = actual_conditions
        .iter()
        .map(|condition| match condition {
            StepCondition::AfterSuccess(step_id) => {
                StepCondition::AfterSuccess(step_id_map[step_id])
            }
            StepCondition::Never => StepCondition::Never,
        })
        .collect::<Vec<_>>();
    assert_eq!(converted_actual_conditions, expected_conditions);
}
