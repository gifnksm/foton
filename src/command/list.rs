use std::{io, sync::Arc};

use snafu::{ResultExt as _, Snafu};

use crate::{
    cli::{
        args::ListArgs,
        context::RootContext,
        reporter::{
            NeverReport, OperationError, ReportScope, ReportValue, RootReportScope,
            ScopeResultErrorExt as _,
        },
    },
    engine,
    package::{PackageId, PackageManifest, PackageState},
};

#[derive(Debug)]
struct ListScope {}

impl ReportScope for ListScope {
    type NoticeReportValue = NeverReport;
    type WarnReportValue = NeverReport;
    type ErrorReportValue = ListErrorReport;
    type Error = ListError;
}

impl RootReportScope for ListScope {
    fn new() -> Self {
        Self {}
    }
}

#[derive(Debug, Snafu)]
enum ListErrorReport {
    #[snafu(display("failed to write entry to stdout"))]
    WriteEntry { source: io::Error },
}

impl From<ListErrorReport> for ReportValue<'static> {
    fn from(report: ListErrorReport) -> Self {
        ReportValue::BoxedError(report.into())
    }
}

#[derive(Debug, Snafu)]
pub(crate) enum ListError {
    #[snafu(display("failed to list installed packages; see previous messages for details"))]
    Failed,
    #[snafu(display("operation cancelled"))]
    Cancelled,
}

impl OperationError for ListError {
    fn failed() -> Self {
        Self::Failed
    }

    fn cancelled() -> Self {
        Self::Cancelled
    }
}

pub(crate) fn list_package(cx: &RootContext, args: &ListArgs) -> Result<(), ListError> {
    let ListArgs { show_pending } = args;

    let cx = ListScope::start(cx);
    let reporter = cx.reporter();

    let mut db_lock_file = engine::open_db_lock_file(&cx)?;
    let db = engine::load_database(&cx, &mut db_lock_file)?;

    let renderer = if *show_pending {
        (&AllEntryRender {}) as &dyn EntryRender
    } else {
        &InstalledEntryRender {} as &dyn EntryRender
    };

    render_entries(&mut io::stdout().lock(), db.entries(), renderer)
        .context(WriteEntrySnafu)
        .report_error(reporter)?;

    Ok(())
}

fn render_entries<I>(
    writer: &mut dyn io::Write,
    entries: I,
    render: &dyn EntryRender,
) -> io::Result<()>
where
    I: IntoIterator<Item = (PackageState, Arc<PackageManifest>)>,
{
    for (state, manifest) in entries {
        let id = manifest.id();
        render.render(writer, &id, state)?;
    }
    Ok(())
}

trait EntryRender {
    fn render(
        &self,
        writer: &mut dyn io::Write,
        id: &PackageId,
        state: PackageState,
    ) -> io::Result<()>;
}

struct AllEntryRender {}

impl EntryRender for AllEntryRender {
    fn render(
        &self,
        writer: &mut dyn io::Write,
        id: &PackageId,
        state: PackageState,
    ) -> io::Result<()> {
        writeln!(writer, "{id} ({state})")
    }
}

struct InstalledEntryRender {}

impl EntryRender for InstalledEntryRender {
    fn render(
        &self,
        writer: &mut dyn io::Write,
        id: &PackageId,
        state: PackageState,
    ) -> io::Result<()> {
        if state == PackageState::Installed {
            writeln!(writer, "{id}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::{macros::concat_line, testing};

    fn make_entries() -> Vec<(PackageState, Arc<PackageManifest>)> {
        vec![
            (
                PackageState::Installed,
                testing::make_manifest("installed-font@1.0.0"),
            ),
            (
                PackageState::PendingInstall,
                testing::make_manifest("pending-install-font@1.1.0"),
            ),
            (
                PackageState::PendingUninstall,
                testing::make_manifest("pending-uninstall-font@1.2.0"),
            ),
        ]
    }

    #[test]
    fn render_entries_with_installed_renderer_only_prints_installed_entries() {
        let entries = make_entries();
        let mut output = Vec::new();

        render_entries(&mut output, entries, &InstalledEntryRender {}).unwrap();

        let output = String::from_utf8(output).unwrap();
        assert_eq!(output, "installed-font@1.0.0\n");
    }

    #[test]
    fn render_entries_with_all_renderer_prints_all_entries_with_states() {
        let entries = make_entries();
        let mut output = Vec::new();

        render_entries(&mut output, entries, &AllEntryRender {}).unwrap();

        let output = String::from_utf8(output).unwrap();
        assert_eq!(
            output,
            concat_line!(
                "installed-font@1.0.0 (installed)",
                "pending-install-font@1.1.0 (pending-install)",
                "pending-uninstall-font@1.2.0 (pending-uninstall)",
                "",
            )
        );
    }
}
