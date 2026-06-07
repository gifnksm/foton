use crate::{
    cli::{context::ReportContext, message::BulletList, reporter::ReportScope},
    engine::{ExecutionPlan, PlanStepOp},
};

pub(crate) fn report_plan<S>(cx: &ReportContext<S>, plan: &ExecutionPlan)
where
    S: ReportScope,
{
    let mut install_ops = vec![];
    let mut uninstall_ops = vec![];
    let mut activate_ops = vec![];
    let mut deactivate_ops = vec![];
    let mut skip_ops = vec![];
    for step in plan.steps() {
        match step.op() {
            PlanStepOp::Install(op) => install_ops.push(op),
            PlanStepOp::Uninstall(op) => uninstall_ops.push(op),
            PlanStepOp::Activate(op) => activate_ops.push(op),
            PlanStepOp::Deactivate(op) => deactivate_ops.push(op),
            PlanStepOp::Skip(op) => skip_ops.push(op),
        }
    }
    if !install_ops.is_empty() {
        cx.reporter().report_info(format_args!(
            "Installing the following packages:\n{}",
            BulletList(
                &install_ops
                    .iter()
                    .map(|op| format!("{} ({})", op.manifest.id(), op.reason))
                    .collect::<Vec<_>>()
            )
        ));
    }
    if !uninstall_ops.is_empty() {
        cx.reporter().report_info(format_args!(
            "Uninstalling the following packages:\n{}",
            BulletList(
                &uninstall_ops
                    .iter()
                    .map(|op| format!("{} ({})", op.pkg_id, op.reason))
                    .collect::<Vec<_>>()
            )
        ));
    }
    if !activate_ops.is_empty() {
        cx.reporter().report_info(format_args!(
            "Activating the following packages:\n{}",
            BulletList(
                &activate_ops
                    .iter()
                    .map(|op| format!("{} ({})", op.pkg_id, op.reason))
                    .collect::<Vec<_>>()
            )
        ));
    }
    if !deactivate_ops.is_empty() {
        cx.reporter().report_info(format_args!(
            "Deactivating the following packages:\n{}",
            BulletList(
                &deactivate_ops
                    .iter()
                    .map(|op| format!("{} ({})", op.pkg_id, op.reason))
                    .collect::<Vec<_>>()
            )
        ));
    }
    if !skip_ops.is_empty() {
        cx.reporter().report_info(format_args!(
            "Skipping the following packages:\n{}",
            BulletList(
                &skip_ops
                    .iter()
                    .map(|op| format!("{} ({})", op.pkg_spec, op.reason))
                    .collect::<Vec<_>>()
            )
        ));
    }
}
