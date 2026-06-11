use std::{collections::BTreeSet, marker::PhantomData};

use snafu::{OptionExt as _, ResultExt as _, Snafu};

use crate::{
    cli::{
        context::ReportContext,
        message::BulletList,
        reporter::{
            ErrorReportExt as _, NeverReport, ReportScope, ResultIteratorExt as _,
            ScopeResultErrorExt as _, SubReportScope,
        },
    },
    registry::{self, FetchRegistryError, RegistryId, RegistryIndex, RegistrySpec},
    util::macros::concat_line,
};

#[derive(Debug, Default)]
struct RegistryScope<S> {
    _base_scope: PhantomData<S>,
}

impl<S> ReportScope for RegistryScope<S>
where
    S: ReportScope,
{
    type NoticeReportValue = NeverReport;
    type WarnReportValue = NeverReport;
    type ErrorReportValue = RegistryErrorReport;
    type Error = S::Error;
}

impl<S> SubReportScope<S> for RegistryScope<S> where S: ReportScope {}

#[derive(Debug, Snafu)]
enum RegistryErrorReport {
    #[snafu(display(
        concat_line!(
            "specified registry `{reg_id}` not found in configuration",
            "available registries:",
            "{registry_ids}",
        ),
        reg_id = reg_id,
        registry_ids = BulletList(available_registry_ids),
    ))]
    RegistryNotFound {
        reg_id: RegistryId,
        available_registry_ids: BTreeSet<RegistryId>,
    },
    #[snafu(display("no enabled registries found in configuration"))]
    NoEnabledRegistries,
    #[snafu(display("failed to fetch registry `{id}`"))]
    FetchRegistry {
        id: RegistryId,
        #[snafu(source(from(FetchRegistryError, Box::new)))]
        source: Box<FetchRegistryError>,
    },
}

pub(crate) fn resolve_registries_by_id<S>(
    cx: &ReportContext<S>,
    registry_ids: Option<&[RegistryId]>,
) -> Result<Vec<RegistrySpec>, S::Error>
where
    S: ReportScope,
{
    let config_registries = &cx.config().registries;
    let cx = RegistryScope::start(cx);

    let registries: Vec<_> = match registry_ids {
        Some(registry_ids) => {
            let registry_ids = registry_ids.iter().cloned().collect::<BTreeSet<_>>();
            registry_ids
                .iter()
                .map(|reg_id| {
                    config_registries
                        .get(reg_id)
                        .map(|registry| RegistrySpec::new(reg_id.clone(), registry.source.clone()))
                        .with_context(|| {
                            let available_registry_ids =
                                config_registries.keys().cloned().collect::<BTreeSet<_>>();
                            RegistryNotFoundSnafu {
                                reg_id,
                                available_registry_ids,
                            }
                        })
                        .report_error(&cx)
                })
                .collect_to_end()?
        }
        None => config_registries
            .iter()
            .filter(|(_id, registry)| registry.enabled)
            .map(|(id, registry)| RegistrySpec::new(id.clone(), registry.source.clone()))
            .collect(),
    };
    if registries.is_empty() {
        return Err(NoEnabledRegistriesSnafu.build().report_error(&cx));
    }
    Ok(registries)
}

pub(crate) fn fetch_registries<S>(
    cx: &ReportContext<S>,
    registries: &[RegistrySpec],
) -> Result<Vec<RegistryIndex>, S::Error>
where
    S: ReportScope,
{
    let cx = RegistryScope::start_with_report(cx, format_args!("Fetching package registries..."));

    registries
        .iter()
        .map(|registry| {
            registry::fetch_registry(cx.app_dirs(), registry)
                .context(FetchRegistrySnafu { id: registry.id() })
                .report_error(&cx)
        })
        .collect::<Result<Vec<_>, _>>()
}
