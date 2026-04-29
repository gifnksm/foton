use std::io;

use crate::{
    cli::{args::ListArgs, context::RootContext},
    command::common,
    package::{PackageId, PackageManifest, PackageState},
    util::reporter::{NeverReport, ReportValue, Step, StepResultErrorExt as _},
};

#[derive(Debug)]
struct ListStep {}

impl Step for ListStep {
    type WarnReportValue = NeverReport;
    type ErrorReportValue = ListErrorReport;
    type Error = ListError;

    fn make_failed(&self) -> Self::Error {
        ListError::Failed
    }
}

#[derive(Debug, derive_more::Display, derive_more::Error)]
pub(crate) enum ListErrorReport {
    #[display("failed to write entry to stdout")]
    WriteEntry {
        #[error(source)]
        source: io::Error,
    },
}

impl From<ListErrorReport> for ReportValue<'static> {
    fn from(report: ListErrorReport) -> Self {
        ReportValue::BoxedError(report.into())
    }
}

#[derive(Debug, derive_more::Display, derive_more::Error)]
pub(crate) enum ListError {
    #[display("failed to list installed packages")]
    Failed,
}

pub(crate) fn list_package(cx: &RootContext, args: &ListArgs) -> Result<(), ListError> {
    let ListArgs { show_pending } = args;

    let cx = cx.with_step(ListStep {});
    let reporter = cx.reporter();

    let mut db_lock_file = common::steps::open_db_lock_file(&cx)?;
    let db = common::steps::load_database(&cx, &mut db_lock_file)?;

    let renderer = if *show_pending {
        (&AllEntryRender {}) as &dyn EntryRender
    } else {
        &InstalledEntryRender {} as &dyn EntryRender
    };

    render_entries(&mut io::stdout().lock(), db.entries(), renderer)
        .map_err(|source| ListErrorReport::WriteEntry { source })
        .report_error(reporter)?;

    Ok(())
}

fn render_entries<'a, I>(
    writer: &mut dyn io::Write,
    entries: I,
    render: &dyn EntryRender,
) -> io::Result<()>
where
    I: IntoIterator<Item = (PackageState, &'a PackageManifest)>,
{
    for (state, manifest) in entries {
        let id = manifest.metadata.id();
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
    use crate::util::testing;

    use super::*;

    fn make_entries() -> Vec<(PackageState, PackageManifest)> {
        vec![
            (
                PackageState::Installed,
                testing::make_manifest("example-namespace", "installed-font", "1.0.0"),
            ),
            (
                PackageState::PendingInstall,
                testing::make_manifest("example-namespace", "pending-install-font", "1.1.0"),
            ),
            (
                PackageState::PendingUninstall,
                testing::make_manifest("example-namespace", "pending-uninstall-font", "1.2.0"),
            ),
        ]
    }

    #[test]
    fn render_entries_with_installed_renderer_only_prints_installed_entries() {
        let entries = make_entries();
        let mut output = Vec::new();

        render_entries(
            &mut output,
            entries.iter().map(|(state, manifest)| (*state, manifest)),
            &InstalledEntryRender {},
        )
        .unwrap();

        let output = String::from_utf8(output).unwrap();
        assert_eq!(output, "example-namespace/installed-font@1.0.0\n");
    }

    #[test]
    fn render_entries_with_all_renderer_prints_all_entries_with_states() {
        let entries = make_entries();
        let mut output = Vec::new();

        render_entries(
            &mut output,
            entries.iter().map(|(state, manifest)| (*state, manifest)),
            &AllEntryRender {},
        )
        .unwrap();

        let output = String::from_utf8(output).unwrap();
        assert_eq!(
            output,
            concat!(
                "example-namespace/installed-font@1.0.0 (installed)\n",
                "example-namespace/pending-install-font@1.1.0 (pending-install)\n",
                "example-namespace/pending-uninstall-font@1.2.0 (pending-uninstall)\n",
            )
        );
    }
}
