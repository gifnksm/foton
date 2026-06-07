use std::io;

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
    db::PackageDbEntry,
    engine,
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
    let ListArgs { show_incomplete } = args;

    let cx = ListScope::start(cx);
    let reporter = cx.reporter();

    let mut db_lock_file = engine::open_db_lock_file(&cx)?;
    let db = engine::load_database(&cx, &mut db_lock_file)?;

    let renderer = if *show_incomplete {
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
    I: IntoIterator<Item = PackageDbEntry>,
{
    for entry in entries {
        render.render(writer, &entry)?;
    }
    Ok(())
}

trait EntryRender {
    fn render(&self, writer: &mut dyn io::Write, entry: &PackageDbEntry) -> io::Result<()>;
}

struct AllEntryRender {}

impl EntryRender for AllEntryRender {
    fn render(&self, writer: &mut dyn io::Write, entry: &PackageDbEntry) -> io::Result<()> {
        if entry.installation_state.is_installed() {
            writeln!(
                writer,
                "{} ({}, {})",
                entry.manifest.id(),
                entry.installation_state,
                entry.activation_state,
            )
        } else {
            writeln!(
                writer,
                "{} ({})",
                entry.manifest.id(),
                entry.installation_state,
            )
        }
    }
}

struct InstalledEntryRender {}

impl EntryRender for InstalledEntryRender {
    fn render(&self, writer: &mut dyn io::Write, entry: &PackageDbEntry) -> io::Result<()> {
        if entry.installation_state.is_installed() {
            writeln!(
                writer,
                "{} ({})",
                entry.manifest.id(),
                entry.activation_state,
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        package::{ActivationState, InstallationState},
        util::{macros::concat_line, testing},
    };

    fn make_entries() -> Vec<PackageDbEntry> {
        vec![
            PackageDbEntry {
                installation_state: InstallationState::Installed,
                activation_state: ActivationState::Inactive,
                manifest: testing::make_manifest("installed-font@1.0.0"),
            },
            PackageDbEntry {
                installation_state: InstallationState::Installed,
                activation_state: ActivationState::Active,
                manifest: testing::make_manifest("active-font@1.0.0"),
            },
            PackageDbEntry {
                installation_state: InstallationState::Installed,
                activation_state: ActivationState::IncompleteActivate,
                manifest: testing::make_manifest("incomplete-activate-font@1.0.0"),
            },
            PackageDbEntry {
                installation_state: InstallationState::Installed,
                activation_state: ActivationState::IncompleteDeactivate,
                manifest: testing::make_manifest("incomplete-deactivate-font@1.0.0"),
            },
            PackageDbEntry {
                installation_state: InstallationState::IncompleteInstall,
                activation_state: ActivationState::Inactive,
                manifest: testing::make_manifest("incomplete-install-font@1.1.0"),
            },
            PackageDbEntry {
                installation_state: InstallationState::IncompleteUninstall,
                activation_state: ActivationState::Inactive,
                manifest: testing::make_manifest("incomplete-uninstall-font@1.2.0"),
            },
        ]
    }

    #[test]
    fn render_entries_with_installed_renderer_only_prints_installed_entries() {
        let entries = make_entries();
        let mut output = Vec::new();

        render_entries(&mut output, entries, &InstalledEntryRender {}).unwrap();

        let output = String::from_utf8(output).unwrap();
        assert_eq!(
            output,
            concat_line!(
                "installed-font@1.0.0 (inactive)",
                "active-font@1.0.0 (active)",
                "incomplete-activate-font@1.0.0 (incomplete-activate)",
                "incomplete-deactivate-font@1.0.0 (incomplete-deactivate)",
                "",
            )
        );
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
                "installed-font@1.0.0 (installed, inactive)",
                "active-font@1.0.0 (installed, active)",
                "incomplete-activate-font@1.0.0 (installed, incomplete-activate)",
                "incomplete-deactivate-font@1.0.0 (installed, incomplete-deactivate)",
                "incomplete-install-font@1.1.0 (incomplete-install)",
                "incomplete-uninstall-font@1.2.0 (incomplete-uninstall)",
                "",
            )
        );
    }
}
