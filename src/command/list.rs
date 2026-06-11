use std::io;

use snafu::{ResultExt as _, Snafu};

use crate::{
    cli::{
        args::ListArgs,
        context::RootContext,
        reporter::{
            NeverReport, OperationError, ReportScope, RootReportScope, ScopeResultErrorExt as _,
        },
    },
    db::PackageDbEntry,
    engine,
};

#[derive(Debug, Default)]
struct ListScope {}

impl ReportScope for ListScope {
    type NoticeReportValue = NeverReport;
    type WarnReportValue = NeverReport;
    type ErrorReportValue = ListErrorReport;
    type Error = ListError;
}

impl RootReportScope for ListScope {}

#[derive(Debug, Snafu)]
enum ListErrorReport {
    #[snafu(display("failed to write entry to stdout"))]
    WriteEntry { source: io::Error },
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

fn render_entries<'a, I>(
    writer: &mut dyn io::Write,
    entries: I,
    render: &dyn EntryRender,
) -> io::Result<()>
where
    I: IntoIterator<Item = PackageDbEntry<'a>>,
{
    for entry in entries {
        render.render(writer, &entry)?;
    }
    Ok(())
}

trait EntryRender {
    fn render(&self, writer: &mut dyn io::Write, entry: &PackageDbEntry<'_>) -> io::Result<()>;
}

struct AllEntryRender {}

impl EntryRender for AllEntryRender {
    fn render(&self, writer: &mut dyn io::Write, entry: &PackageDbEntry<'_>) -> io::Result<()> {
        if entry.installation_state().is_installed() {
            writeln!(
                writer,
                "{} ({}, {})",
                entry.manifest().id(),
                entry.installation_state(),
                entry.activation_state(),
            )
        } else {
            writeln!(
                writer,
                "{} ({})",
                entry.manifest().id(),
                entry.installation_state(),
            )
        }
    }
}

struct InstalledEntryRender {}

impl EntryRender for InstalledEntryRender {
    fn render(&self, writer: &mut dyn io::Write, entry: &PackageDbEntry<'_>) -> io::Result<()> {
        if entry.installation_state().is_installed() {
            writeln!(
                writer,
                "{} ({})",
                entry.manifest().id(),
                entry.activation_state(),
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        db::PackageDatabase,
        util::{macros::concat_line, testing},
    };

    fn make_entries<'db>(
        db: &'db mut PackageDatabase<'_>,
    ) -> impl Iterator<Item = PackageDbEntry<'db>> + 'db {
        testing::mark_as_installed(db, &testing::make_manifest("installed-font@1.0.0"));
        testing::mark_as_active(db, &testing::make_manifest("active-font@1.0.0"));
        testing::mark_as_incomplete_activation(
            db,
            &testing::make_manifest("incomplete-activate-font@1.0.0"),
        );
        testing::mark_as_incomplete_deactivation(
            db,
            &testing::make_manifest("incomplete-deactivate-font@1.0.0"),
        );
        testing::mark_as_incomplete_install(
            db,
            &testing::make_manifest("incomplete-install-font@1.1.0"),
        );
        testing::mark_as_incomplete_uninstall(
            db,
            &testing::make_manifest("incomplete-uninstall-font@1.2.0"),
        );
        db.entries()
    }

    #[test]
    fn render_entries_with_installed_renderer_only_prints_installed_entries() {
        testing::with_db(|_cx, db| {
            let entries = make_entries(db);
            let mut output = Vec::new();

            render_entries(&mut output, entries, &InstalledEntryRender {}).unwrap();

            let output = String::from_utf8(output).unwrap();
            assert_eq!(
                output,
                concat_line!(
                    "active-font@1.0.0 (active)",
                    "incomplete-activate-font@1.0.0 (incomplete-activation)",
                    "incomplete-deactivate-font@1.0.0 (incomplete-deactivation)",
                    "installed-font@1.0.0 (inactive)",
                    "",
                )
            );
        });
    }

    #[test]
    fn render_entries_with_all_renderer_prints_all_entries_with_states() {
        testing::with_db(|_cx, db| {
            let entries = make_entries(db);
            let mut output = Vec::new();

            render_entries(&mut output, entries, &AllEntryRender {}).unwrap();

            let output = String::from_utf8(output).unwrap();
            assert_eq!(
                output,
                concat_line!(
                    "active-font@1.0.0 (installed, active)",
                    "incomplete-activate-font@1.0.0 (installed, incomplete-activation)",
                    "incomplete-deactivate-font@1.0.0 (installed, incomplete-deactivation)",
                    "incomplete-install-font@1.1.0 (incomplete-install)",
                    "incomplete-uninstall-font@1.2.0 (incomplete-uninstall)",
                    "installed-font@1.0.0 (installed, inactive)",
                    "",
                )
            );
        });
    }
}
