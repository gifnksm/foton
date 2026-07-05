use std::io;

use serde::Serialize;
use snafu::{ResultExt as _, Snafu};

use crate::{
    cli::{
        args::{ListArgs, ListFormat},
        context::{ReportContext, RootContext},
        reporter::{
            NeverReport, OperationError, ReportScope, RootReportScope, ScopeResultErrorExt as _,
        },
    },
    db::PackageDbEntry,
    engine,
    package::{ActivationState, InstallationState, PackageId},
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
    #[snafu(display("failed to serialize package information"))]
    SerializeToJsonl { source: serde_json::Error },
    #[snafu(display("failed to render package information to stdout"))]
    RenderToStdout { source: io::Error },
}

#[derive(Debug, Snafu)]
pub(crate) enum ListError {
    #[snafu(display("failed to list packages; see previous messages for details"))]
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
    let ListArgs { format } = args;

    let cx = ListScope::start(cx);

    let mut db_lock_file = engine::open_db_lock_file(&cx)?;
    let db = engine::load_database(&cx, &mut db_lock_file)?;

    let renderer = match format {
        ListFormat::Text => (&TextRender {}) as &dyn EntryRender,
        ListFormat::Jsonl => &JsonlRender {},
    };

    render_entries(&cx, &mut io::stdout().lock(), db.all_entries(), renderer)?;

    Ok(())
}

fn render_entries<'a, I>(
    cx: &ReportContext<ListScope>,
    writer: &mut dyn io::Write,
    entries: I,
    render: &dyn EntryRender,
) -> Result<(), ListError>
where
    I: IntoIterator<Item = PackageDbEntry<'a>>,
{
    for entry in entries {
        render.render(cx, writer, &entry)?;
    }
    Ok(())
}

trait EntryRender {
    fn render(
        &self,
        cx: &ReportContext<ListScope>,
        writer: &mut dyn io::Write,
        entry: &PackageDbEntry<'_>,
    ) -> Result<(), ListError>;
}

struct TextRender {}

impl EntryRender for TextRender {
    fn render(
        &self,
        cx: &ReportContext<ListScope>,
        writer: &mut dyn io::Write,
        entry: &PackageDbEntry<'_>,
    ) -> Result<(), ListError> {
        writeln!(
            writer,
            "{} ({}, {})",
            entry.id(),
            entry.installation_state(),
            entry.activation_state(),
        )
        .context(RenderToStdoutSnafu)
        .report_error(cx)
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "kebab-case")]
struct JsonlEntry {
    #[serde(rename = "package-id")]
    pkg_id: PackageId,
    installation_state: JsonlInstallationState,
    activation_state: JsonlActivationState,
}

impl From<&PackageDbEntry<'_>> for JsonlEntry {
    fn from(value: &PackageDbEntry<'_>) -> Self {
        Self {
            pkg_id: value.id().clone(),
            installation_state: value.installation_state().into(),
            activation_state: value.activation_state().into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, derive_more::IsVariant)]
#[serde(rename_all = "kebab-case")]
enum JsonlInstallationState {
    Installed,
    IncompleteInstall,
    IncompleteUninstall,
}

impl From<InstallationState> for JsonlInstallationState {
    fn from(value: InstallationState) -> Self {
        match value {
            InstallationState::Installed => Self::Installed,
            InstallationState::IncompleteInstall => Self::IncompleteInstall,
            InstallationState::IncompleteUninstall => Self::IncompleteUninstall,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, derive_more::IsVariant)]
#[serde(rename_all = "kebab-case")]
enum JsonlActivationState {
    Active,
    Inactive,
    IncompleteActivation,
    IncompleteDeactivation,
}

impl From<ActivationState> for JsonlActivationState {
    fn from(value: ActivationState) -> Self {
        match value {
            ActivationState::Active => Self::Active,
            ActivationState::Inactive => Self::Inactive,
            ActivationState::IncompleteActivation => Self::IncompleteActivation,
            ActivationState::IncompleteDeactivation => Self::IncompleteDeactivation,
        }
    }
}

struct JsonlRender {}

impl EntryRender for JsonlRender {
    fn render(
        &self,
        cx: &ReportContext<ListScope>,
        writer: &mut dyn io::Write,
        entry: &PackageDbEntry<'_>,
    ) -> Result<(), ListError> {
        let entry = JsonlEntry::from(entry);
        let line = serde_json::to_string(&entry)
            .context(SerializeToJsonlSnafu)
            .report_error(cx)?;
        writeln!(writer, "{line}")
            .context(RenderToStdoutSnafu)
            .report_error(cx)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{db::PackageDatabase, util::testing};

    fn make_entries<'db>(
        db: &'db mut PackageDatabase<'_>,
    ) -> impl Iterator<Item = PackageDbEntry<'db>> + 'db {
        testing::mark_as_installed(
            db,
            &testing::make_package_definition("installed-font@1.0.0"),
        );
        testing::mark_as_active(db, &testing::make_package_definition("active-font@1.0.0"));
        testing::mark_as_incomplete_activation(
            db,
            &testing::make_package_definition("incomplete-activate-font@1.0.0"),
        );
        testing::mark_as_incomplete_deactivation(
            db,
            &testing::make_package_definition("incomplete-deactivate-font@1.0.0"),
        );
        testing::mark_as_incomplete_install(
            db,
            &testing::make_package_definition("incomplete-install-font@1.1.0"),
        );
        testing::mark_as_incomplete_uninstall(
            db,
            &testing::make_package_definition("incomplete-uninstall-font@1.2.0"),
        );
        db.all_entries()
    }

    #[test]
    fn render_entries_prints_all_entries_with_states() {
        testing::with_root_context(|cx| {
            let cx = ListScope::start(cx);
            testing::with_db_in_context(&cx, |db| {
                let entries = make_entries(db);
                let mut output = Vec::new();

                render_entries(&cx, &mut output, entries, &TextRender {}).unwrap();

                let output = String::from_utf8(output).unwrap();
                let expected = indoc::indoc! {r"
                    active-font@1.0.0 (installed, active)
                    incomplete-activate-font@1.0.0 (installed, incomplete-activation)
                    incomplete-deactivate-font@1.0.0 (installed, incomplete-deactivation)
                    incomplete-install-font@1.1.0 (incomplete-install, inactive)
                    incomplete-uninstall-font@1.2.0 (incomplete-uninstall, inactive)
                    installed-font@1.0.0 (installed, inactive)
                "};
                assert_eq!(output, expected);
            });
        });
    }
}
