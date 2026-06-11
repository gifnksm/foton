use std::{cmp::Reverse, collections::BinaryHeap, io, sync::Arc};

use snafu::{ResultExt as _, Snafu};

use crate::{
    cli::{
        args::SearchArgs,
        context::RootContext,
        reporter::{
            NeverReport, OperationError, ReportScope, RootReportScope, ScopeResultErrorExt as _,
        },
    },
    engine,
    package::{ManifestMatchResult, PackageManifest},
    registry::{RegistryId, RegistryIndexError},
    util::text::TextMatcher,
};

#[derive(Debug, Default)]
struct SearchScope {}

impl ReportScope for SearchScope {
    type NoticeReportValue = NeverReport;
    type WarnReportValue = NeverReport;
    type ErrorReportValue = SearchErrorReport;
    type Error = SearchError;
}

impl RootReportScope for SearchScope {}

#[derive(Debug, Snafu)]
enum SearchErrorReport {
    #[snafu(display("failed to iterate latest packages from registry {reg_id}"))]
    AllLatestPackages {
        reg_id: RegistryId,
        source: RegistryIndexError,
    },
    #[snafu(display("failed to read a latest package entry from registry {reg_id}"))]
    ReadLatestPackage {
        reg_id: RegistryId,
        #[snafu(source(from(RegistryIndexError, Box::new)))]
        source: Box<RegistryIndexError>,
    },
    #[snafu(display("failed to write search result to stdout"))]
    WriteResult { source: io::Error },
    #[snafu(display("no matching packages found"))]
    NoMatchingPackagesFound,
}

#[derive(Debug, Snafu)]
pub(crate) enum SearchError {
    #[snafu(display("failed to search package(s); see previous messages for details"))]
    Failed,
    #[snafu(display("operation cancelled"))]
    Cancelled,
}

impl OperationError for SearchError {
    fn failed() -> Self {
        Self::Failed
    }

    fn cancelled() -> Self {
        Self::Cancelled
    }
}

pub(crate) fn search_packages(cx: &RootContext, args: &SearchArgs) -> Result<(), SearchError> {
    let SearchArgs {
        registries,
        limit,
        queries,
        pre_release,
    } = args;

    let cx = SearchScope::start(cx);

    let registries = engine::resolve_registries_by_id(&cx, registries.as_deref())?;
    let indexes = engine::fetch_registries(&cx, &registries)?;

    let matcher = TextMatcher::new(queries.clone());
    let manifests =
        collect_search_results(&indexes, &matcher, *limit, *pre_release).report_error(&cx)?;
    render_search_results(&mut io::stdout().lock(), manifests)
        .context(WriteResultSnafu)
        .report_error(&cx)?;

    Ok(())
}

fn collect_search_results(
    indexes: &[crate::registry::RegistryIndex],
    matcher: &TextMatcher,
    limit: usize,
    include_pre_release: bool,
) -> Result<Vec<ScoredManifest>, SearchErrorReport> {
    let mut heap = BinaryHeap::new();

    for index in indexes {
        let Some(manifests) = index
            .all_latest_packages(include_pre_release)
            .with_context(|_| AllLatestPackagesSnafu {
                reg_id: index.id().clone(),
            })?
        else {
            continue;
        };
        for manifest in manifests {
            let manifest = manifest.with_context(|_| ReadLatestPackageSnafu {
                reg_id: index.id().clone(),
            })?;
            if let Some(score) = manifest.match_manifest(matcher) {
                let reg_id = index.id().clone();
                heap.push(Reverse(ScoredManifest {
                    score,
                    reg_id,
                    manifest,
                }));
                if heap.len() > limit {
                    heap.pop();
                }
            }
        }
    }

    if heap.is_empty() {
        return Err(NoMatchingPackagesFoundSnafu.build());
    }

    Ok(heap
        .into_sorted_vec()
        .into_iter()
        .map(|Reverse(manifest)| manifest)
        .collect())
}

fn render_search_results<I>(writer: &mut dyn io::Write, manifests: I) -> io::Result<()>
where
    I: IntoIterator<Item = ScoredManifest>,
{
    for manifest in manifests {
        let m = &manifest.manifest;
        writeln!(writer, "{} [{}]", m.id(), manifest.reg_id)?;
        if let Some(description) = m.description.as_deref() {
            writeln!(writer, "  {description}")?;
        }
    }
    Ok(())
}

#[derive(Debug)]
pub(crate) struct ScoredManifest {
    score: ManifestMatchResult,
    reg_id: RegistryId,
    manifest: Arc<PackageManifest>,
}

impl PartialEq for ScoredManifest {
    fn eq(&self, other: &Self) -> bool {
        self.score == other.score
            && self.reg_id == other.reg_id
            && self.manifest.id() == other.manifest.id()
    }
}

impl Eq for ScoredManifest {}

impl PartialOrd for ScoredManifest {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ScoredManifest {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.score
            .cmp(&other.score)
            .then_with(|| self.reg_id.cmp(&other.reg_id))
            .then_with(|| self.manifest.id().cmp(&other.manifest.id()))
    }
}

#[cfg(test)]
mod tests {
    use std::assert_matches;

    use super::*;
    use crate::{
        registry::RegistryId,
        util::{macros::concat_line, testing, text::QueryString},
    };

    fn make_matcher(query: &str) -> TextMatcher {
        TextMatcher::new(vec![QueryString::try_new(query).unwrap()])
    }

    fn make_scored_manifest(manifest: Arc<PackageManifest>) -> ScoredManifest {
        ScoredManifest {
            score: ManifestMatchResult {
                form: crate::util::text::MatchForm::Separated,
                kind: crate::util::text::MatchKind::Exact,
                field: crate::package::ManifestField::Name,
            },
            reg_id: RegistryId::new("foton").unwrap(),
            manifest,
        }
    }

    #[test]
    fn render_search_results_prints_registry_without_description() {
        let manifests = vec![make_scored_manifest(testing::make_manifest(
            "example-font@1.0.0",
        ))];
        let mut output = Vec::new();

        render_search_results(&mut output, manifests).unwrap();

        assert_eq!(
            String::from_utf8(output).unwrap(),
            "example-font@1.0.0 [foton]\n"
        );
    }

    #[test]
    fn render_search_results_prints_description_when_present() {
        let manifest = Arc::new(
            toml::from_str::<PackageManifest>(
                r#"
name = "example-font"
version = "1.0.0"
description = "Example font family for UI and coding"

[[sources]]
url = "https://example.com/example-font-1.0.0.zip"
hash = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
"#,
            )
            .unwrap(),
        );
        let manifests = vec![make_scored_manifest(manifest)];
        let mut output = Vec::new();

        render_search_results(&mut output, manifests).unwrap();

        assert_eq!(
            String::from_utf8(output).unwrap(),
            concat_line!(
                "example-font@1.0.0 [foton]",
                "  Example font family for UI and coding",
                "",
            )
        );
    }

    #[test]
    fn collect_search_results_excludes_pre_release_packages_by_default() {
        let (registry_dir, registry) = testing::make_registry_index("foton");
        testing::write_manifest(registry_dir.path(), "preview-font@1.0.0-rc-1");

        let err =
            collect_search_results(&[registry], &make_matcher("preview"), 10, false).unwrap_err();

        assert_matches!(err, SearchErrorReport::NoMatchingPackagesFound);
    }

    #[test]
    fn collect_search_results_includes_pre_release_packages_when_requested() {
        let (registry_dir, registry) = testing::make_registry_index("foton");
        testing::write_manifest(registry_dir.path(), "preview-font@1.0.0-rc-1");

        let results =
            collect_search_results(&[registry], &make_matcher("preview"), 10, true).unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(
            results[0].manifest.id().to_string(),
            "preview-font@1.0.0-rc-1"
        );
    }
}
