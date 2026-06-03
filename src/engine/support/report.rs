use crate::{
    cli::{context::ReportContext, message::BulletList, reporter::ReportScope},
    engine::ExecutionPlan,
};

pub(crate) fn report_plan<S>(cx: &ReportContext<S>, plan: &ExecutionPlan)
where
    S: ReportScope,
{
    let mut install_ops = plan
        .steps()
        .iter()
        .filter_map(|step| step.op().as_install())
        .peekable();
    let mut uninstall_ops = plan
        .steps()
        .iter()
        .filter_map(|step| step.op().as_uninstall())
        .peekable();
    let mut skip_ops = plan
        .steps()
        .iter()
        .filter_map(|step| step.op().as_skip())
        .peekable();
    if install_ops.peek().is_some() {
        cx.reporter().report_info(format_args!(
            "Installing the following packages:\n{}",
            BulletList(
                &install_ops
                    .map(|op| format!("{} ({})", op.manifest.id(), op.reason))
                    .collect::<Vec<_>>()
            )
        ));
    }
    if uninstall_ops.peek().is_some() {
        cx.reporter().report_info(format_args!(
            "Uninstalling the following packages:\n{}",
            BulletList(
                &uninstall_ops
                    .map(|op| format!("{} ({})", op.pkg_id, op.reason))
                    .collect::<Vec<_>>()
            )
        ));
    }
    if skip_ops.peek().is_some() {
        cx.reporter().report_info(format_args!(
            "Skipping the following packages:\n{}",
            BulletList(
                &skip_ops
                    .map(|op| format!("{} ({})", op.pkg_spec, op.reason))
                    .collect::<Vec<_>>()
            )
        ));
    }
}
