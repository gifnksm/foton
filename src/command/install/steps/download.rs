use std::{fs::File, io, pin::pin, sync::Arc};

use bytes::Bytes;
use futures_core::Stream;
use futures_util::StreamExt as _;
use reqwest::{Response, Url};
use snafu::{ResultExt as _, Snafu};
use tokio::io::{AsyncSeekExt as _, AsyncWriteExt as _};

use crate::{
    cli::{
        config::FotonConfig,
        context::ReportContext,
        reporter::{
            NeverReport, ReportScope, ReportValue, ScopeResultErrorExt as _, SubReportScope,
        },
    },
    package::{PackageId, PackageSource},
    util::hash::{GenericDigest, GenericHasher},
};

#[derive(Debug)]
struct DownloadScope<S> {
    base_scope: Arc<S>,
}

impl<S> ReportScope for DownloadScope<S>
where
    S: ReportScope,
{
    type WarnReportValue = NeverReport;
    type ErrorReportValue = DownloadErrorReport;
    type Error = S::Error;

    fn make_failed(&self) -> Self::Error {
        self.base_scope.make_failed()
    }
}

impl<S> SubReportScope<S> for DownloadScope<S>
where
    S: ReportScope,
{
    fn new(base_scope: Arc<S>) -> Self {
        Self { base_scope }
    }
}

#[derive(Debug, Snafu)]
enum DownloadErrorReport {
    #[snafu(display("failed to download font archive from {url}"))]
    Get { url: Url, source: reqwest::Error },
    #[snafu(display(
        "server-reported archive size {reported_size} exceeds maximum allowed size of {max_size}"
    ))]
    ReportedSizeExceedsMax { reported_size: u64, max_size: u64 },
    #[snafu(display(
        "actual downloaded archive size {downloaded_size} exceeds maximum allowed size of {max_size}"
    ))]
    DownloadedSizeExceedsMax { downloaded_size: u64, max_size: u64 },
    #[snafu(display(
        "downloaded archive hash mismatch for {pkg_id}: expected {expected}, got {got}"
    ))]
    HashMismatch {
        pkg_id: PackageId,
        expected: Box<GenericDigest>,
        got: Box<GenericDigest>,
    },
    #[snafu(display("failed to create temporary file for downloaded archive"))]
    CreateTempFile { source: io::Error },
    #[snafu(display("failed to write chunk to temporary file for downloaded archive"))]
    WriteTempFile { source: io::Error },
    #[snafu(display("failed to read response body while downloading archive"))]
    ReadResponseBody { source: reqwest::Error },
    #[snafu(display("failed to rewind temporary file for downloaded archive"))]
    Rewind { source: io::Error },
}

impl From<DownloadErrorReport> for ReportValue<'static> {
    fn from(report: DownloadErrorReport) -> Self {
        Self::BoxedError(report.into())
    }
}

pub(in crate::command::install) async fn download_archive<S>(
    cx: &ReportContext<S>,
    pkg_id: &PackageId,
    source: &PackageSource,
) -> Result<File, S::Error>
where
    S: ReportScope,
{
    let cx = DownloadScope::start_with_report(cx, format_args!("Downloading {}...", source.url));
    let reporter = cx.reporter();

    let response = reqwest::get(source.url.clone())
        .await
        .and_then(Response::error_for_status)
        .with_context(|_| GetSnafu {
            url: source.url.clone(),
        })
        .report_error(reporter)?;

    let len = response.content_length();
    if let Some(len) = len
        && len > cx.config().install.max_archive_size_bytes
    {
        return Err(reporter.report_error(
            ReportedSizeExceedsMaxSnafu {
                reported_size: len,
                max_size: cx.config().install.max_archive_size_bytes,
            }
            .build(),
        ));
    }
    let hasher = source.hash.hasher();
    let (output, digest) = reporter
        .with_download_progress_bar(len, async |pb| {
            stream_archive_to_tempfile(response.bytes_stream(), hasher, cx.config(), pb).await
        })
        .await
        .report_error(reporter)?;
    if digest != source.hash {
        let err = reporter.report_error(
            HashMismatchSnafu {
                pkg_id,
                expected: Box::new(source.hash.clone()),
                got: Box::new(digest),
            }
            .build(),
        );
        return Err(err);
    }
    Ok(output)
}

async fn stream_archive_to_tempfile<S>(
    chunks: S,
    mut hasher: GenericHasher,
    config: &FotonConfig,
    pb: &indicatif::ProgressBar,
) -> Result<(File, GenericDigest), DownloadErrorReport>
where
    S: Stream<Item = Result<Bytes, reqwest::Error>>,
{
    let mut chunks = pin!(chunks);

    let output = tempfile::tempfile().context(CreateTempFileSnafu)?;
    let mut output = tokio::fs::File::from_std(output);
    let mut total_size = 0;
    while let Some(chunk) = chunks.next().await {
        let chunk = chunk.context(ReadResponseBodySnafu)?;
        let chunk_size = chunk.len() as u64;
        total_size += chunk_size;
        snafu::ensure!(
            total_size <= config.install.max_archive_size_bytes,
            DownloadedSizeExceedsMaxSnafu {
                downloaded_size: total_size,
                max_size: config.install.max_archive_size_bytes,
            }
        );
        hasher.update(&chunk);
        output.write_all(&chunk).await.context(WriteTempFileSnafu)?;
        pb.inc(chunk_size);
    }
    let digest = hasher.finalize();
    output.rewind().await.context(RewindSnafu)?;
    let output = output.into_std().await;
    Ok((output, digest))
}

#[cfg(test)]
mod tests {
    use futures_util::stream;
    use indicatif::ProgressBar;
    use sha2::{Digest as _, Sha256};

    use super::*;
    use crate::cli::config::InstallConfig;

    #[tokio::test]
    async fn stream_archive_to_tempfile_rejects_download_size_exceeding_limit() {
        let chunks = stream::once(async { Ok(Bytes::copy_from_slice(b"font")) });
        let config = FotonConfig {
            install: InstallConfig {
                max_archive_size_bytes: 3,
                ..InstallConfig::default()
            },
            ..FotonConfig::default()
        };
        let pb = ProgressBar::hidden();

        let hasher = Sha256::new().into();
        let err = stream_archive_to_tempfile(chunks, hasher, &config, &pb)
            .await
            .unwrap_err();

        assert!(matches!(
            err,
            DownloadErrorReport::DownloadedSizeExceedsMax { .. }
        ));
    }
}
