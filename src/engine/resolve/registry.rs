use std::{
    collections::{BTreeMap, BTreeSet},
    marker::PhantomData,
};

use snafu::{OptionExt as _, ResultExt as _, Snafu};

use crate::{
    cli::{
        config::RegistryConfig,
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

pub(crate) fn resolve_registries<S>(
    cx: &ReportContext<S>,
    registry_ids: Option<&[RegistryId]>,
) -> Result<Vec<RegistrySpec>, S::Error>
where
    S: ReportScope,
{
    let config_registries = &cx.config().registries;
    let registries = match registry_ids {
        Some(registry_ids) => resolve_registries_by_id(cx, config_registries, registry_ids)?,
        None => resolve_all_enabled_registries(config_registries),
    };
    Ok(registries)
}

fn resolve_registries_by_id<S>(
    cx: &ReportContext<S>,
    config_registries: &BTreeMap<RegistryId, RegistryConfig>,
    registry_ids: &[RegistryId],
) -> Result<Vec<RegistrySpec>, S::Error>
where
    S: ReportScope,
{
    let cx = RegistryScope::start(cx);

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
        .collect_to_end()
}

fn resolve_all_enabled_registries(
    config_registries: &BTreeMap<RegistryId, RegistryConfig>,
) -> Vec<RegistrySpec> {
    config_registries
        .iter()
        .filter(|(_id, registry)| registry.enabled)
        .map(|(id, registry)| RegistrySpec::new(id.clone(), registry.source.clone()))
        .collect()
}

pub(crate) fn ensure_non_empty_registries<S>(
    cx: &ReportContext<S>,
    registries: &[RegistrySpec],
) -> Result<(), S::Error>
where
    S: ReportScope,
{
    let cx = RegistryScope::start(cx);
    if registries.is_empty() {
        return Err(NoEnabledRegistriesSnafu.build().report_error(&cx));
    }
    Ok(())
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

#[cfg(test)]
mod tests {

    use std::{slice, str::FromStr as _};

    use super::*;
    use crate::{cli::config::FotonConfig, util::testing};

    fn make_config(reg_ids: &[(&str, bool)]) -> FotonConfig {
        let registries = reg_ids
            .iter()
            .map(|&(reg_id, enabled)| {
                let reg_id = RegistryId::from_str(reg_id).unwrap();
                (
                    reg_id.clone(),
                    RegistryConfig {
                        source: format!("git+https://example.com/registry-{reg_id}.git")
                            .parse()
                            .unwrap(),
                        enabled,
                    },
                )
            })
            .collect();
        FotonConfig {
            registries,
            ..Default::default()
        }
    }

    #[test]
    fn resolve_registries_reports_unknown_explicit_registry() {
        let config = FotonConfig::default();
        testing::with_configured_context(config, |cx| {
            let err =
                resolve_registries(cx, Some(&[RegistryId::new("unknown").unwrap()])).unwrap_err();
            assert!(err.is_failed());
        });
    }

    #[test]
    fn resolve_registries_returns_explicit_registry_even_when_disabled() {
        let reg_id = RegistryId::from_str("disabled").unwrap();
        let config = make_config(&[("disabled", false)]);
        testing::with_configured_context(config, |cx| {
            let registries = resolve_registries(cx, Some(slice::from_ref(&reg_id))).unwrap();
            assert_eq!(registries.len(), 1);
            assert_eq!(registries[0].id(), "disabled");
        });
    }

    #[test]
    fn resolve_registries_returns_enabled_registries_when_ids_are_omitted() {
        let config = make_config(&[("enabled", true), ("disabled", false)]);
        testing::with_configured_context(config, |cx| {
            let registries = resolve_registries(cx, None).unwrap();
            assert_eq!(registries.len(), 1);
            assert_eq!(registries[0].id(), "enabled");
        });
    }
}
