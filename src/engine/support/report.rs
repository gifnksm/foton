use crate::{
    cli::{context::ReportContext, message::BulletList, reporter::ReportScope},
    engine::ExecutionPlan,
};

pub(crate) fn report_plan<S>(cx: &ReportContext<S>, plan: &ExecutionPlan)
where
    S: ReportScope,
{
    let mut install_ops = plan
        .ops()
        .iter()
        .filter_map(|op| op.as_install())
        .peekable();
    let mut uninstall_ops = plan
        .ops()
        .iter()
        .filter_map(|op| op.as_uninstall())
        .peekable();
    let mut skip_ops = plan.ops().iter().filter_map(|op| op.as_skip()).peekable();
    if install_ops.peek().is_some() {
        cx.reporter().report_info(format_args!(
            "Installing the following packages:\n{}",
            BulletList(
                &install_ops
                    .map(|op| format!("{} ({})", op.manifest.metadata.id(), op.reason))
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
                    .map(|op| format!("{} ({})", op.pkg_id, op.reason))
                    .collect::<Vec<_>>()
            )
        ));
    }
}
