use std::collections::HashMap;

use crate::engine::{ExecutionPlan, PlanStep, StepCondition, StepId};

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
        assert_eq!(actual_op, expected_op);
        assert_conditions_eq(&step_id_map, actual_conditions, expected_conditions);
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
