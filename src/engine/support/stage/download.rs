use std::{fs::File, io, marker::PhantomData, pin::pin};

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
            ErrorReportExt as _, NeverReport, ReportScope, ScopeResultErrorExt as _, SubReportScope,
        },
    },
    package::{PackageId, PackageSource},
    util::hash::{GenericDigest, GenericHasher},
};

#[derive(Debug, Default)]
struct DownloadScope<S> {
    _base_scope: PhantomData<S>,
}

impl<S> ReportScope for DownloadScope<S>
where
    S: ReportScope,
{
    type NoticeReportValue = NeverReport;
    type WarnReportValue = NeverReport;
    type ErrorReportValue = DownloadErrorReport;
    type Error = S::Error;
}

impl<S> SubReportScope<S> for DownloadScope<S> where S: ReportScope {}

#[derive(Debug, Snafu)]
enum DownloadErrorReport {
    #[snafu(display("failed to download package source file from {url}"))]
    Get { url: Url, source: reqwest::Error },
    #[snafu(display(
        "server-reported file size {reported_size} exceeds maximum allowed size of {max_size}"
    ))]
    ReportedSizeExceedsMax { reported_size: u64, max_size: u64 },
    #[snafu(display(
        "actual downloaded file size {downloaded_size} exceeds maximum allowed size of {max_size}"
    ))]
    DownloadedSizeExceedsMax { downloaded_size: u64, max_size: u64 },
    #[snafu(display("downloaded file hash mismatch for {pkg_id}: expected {expected}, got {got}"))]
    HashMismatch {
        pkg_id: PackageId,
        expected: Box<GenericDigest>,
        got: Box<GenericDigest>,
    },
    #[snafu(display("failed to create temporary file for downloaded file"))]
    CreateTempFile { source: io::Error },
    #[snafu(display("failed to write chunk to temporary file for downloaded file"))]
    WriteTempFile { source: io::Error },
    #[snafu(display("failed to read response body while downloading file"))]
    ReadResponseBody { source: reqwest::Error },
    #[snafu(display("failed to rewind temporary file for downloaded file"))]
    Rewind { source: io::Error },
}

pub(super) async fn download_source_file<S>(
    cx: &ReportContext<S>,
    pkg_id: &PackageId,
    source: &PackageSource,
) -> Result<(File, u64), S::Error>
where
    S: ReportScope,
{
    let cx = DownloadScope::start_with_report(cx, format_args!("Downloading {}...", source.url));

    let response = reqwest::get(source.url.clone())
        .await
        .and_then(Response::error_for_status)
        .with_context(|_| GetSnafu {
            url: source.url.clone(),
        })
        .report_error(&cx)?;

    let len = response.content_length();
    if let Some(len) = len
        && len > cx.config().install.max_source_size_bytes.get()
    {
        return Err(ReportedSizeExceedsMaxSnafu {
            reported_size: len,
            max_size: cx.config().install.max_source_size_bytes.get(),
        }
        .build()
        .report_error(&cx));
    }
    let hasher = source.hash.hasher();
    let (output, digest, total_size) = cx
        .reporter()
        .with_download_progress_bar(len, async |pb| {
            stream_source_file_to_tempfile(response.bytes_stream(), hasher, cx.config(), pb).await
        })
        .await
        .report_error(&cx)?;
    if digest != source.hash {
        return Err(HashMismatchSnafu {
            pkg_id,
            expected: Box::new(source.hash.clone()),
            got: Box::new(digest),
        }
        .build()
        .report_error(&cx));
    }
    Ok((output, total_size))
}

async fn stream_source_file_to_tempfile<S>(
    chunks: S,
    mut hasher: GenericHasher,
    config: &FotonConfig,
    pb: &indicatif::ProgressBar,
) -> Result<(File, GenericDigest, u64), DownloadErrorReport>
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
            total_size <= config.install.max_source_size_bytes.get(),
            DownloadedSizeExceedsMaxSnafu {
                downloaded_size: total_size,
                max_size: config.install.max_source_size_bytes,
            }
        );
        hasher.update(&chunk);
        output.write_all(&chunk).await.context(WriteTempFileSnafu)?;
        pb.inc(chunk_size);
    }
    let digest = hasher.finalize();
    output.rewind().await.context(RewindSnafu)?;
    let output = output.into_std().await;
    Ok((output, digest, total_size))
}

#[cfg(test)]
mod tests {
    use std::{assert_matches, num::NonZero};

    use futures_util::stream;
    use indicatif::ProgressBar;
    use sha2::{Digest as _, Sha256};

    use super::*;
    use crate::cli::config::InstallConfig;

    #[tokio::test]
    async fn stream_source_file_to_tempfile_rejects_download_size_exceeding_limit() {
        let chunks = stream::once(async { Ok(Bytes::copy_from_slice(b"font")) });
        let config = FotonConfig {
            install: InstallConfig {
                max_source_size_bytes: NonZero::new(3).unwrap(),
                ..InstallConfig::default()
            },
            ..FotonConfig::default()
        };
        let pb = ProgressBar::hidden();

        let hasher = Sha256::new().into();
        let err = stream_source_file_to_tempfile(chunks, hasher, &config, &pb)
            .await
            .unwrap_err();

        assert_matches!(err, DownloadErrorReport::DownloadedSizeExceedsMax { .. });
    }
}
