use std::{fs::File, io, pin::pin, sync::Arc};

use bytes::Bytes;
use futures_core::Stream;
use futures_util::StreamExt as _;
use reqwest::{Response, Url};
use tokio::io::{AsyncSeekExt as _, AsyncWriteExt as _};

use crate::{
    cli::{config::Config, context::StepContext},
    package::{PackageId, PackageSource},
    util::{
        hash::{GenericDigest, GenericHasher},
        reporter::{NeverReport, ReportValue, Step, StepResultErrorExt as _},
    },
};

#[derive(Debug)]
struct DownloadStep<S> {
    step: Arc<S>,
}

impl<S> Step for DownloadStep<S>
where
    S: Step,
{
    type WarnReportValue = NeverReport;
    type ErrorReportValue = DownloadErrorReport;
    type Error = S::Error;

    fn make_failed(&self) -> Self::Error {
        self.step.make_failed()
    }
}

#[derive(Debug, derive_more::Display, derive_more::Error)]
enum DownloadErrorReport {
    #[display("failed to download font archive from {url}")]
    Get {
        url: Url,
        #[error(source)]
        source: reqwest::Error,
    },
    #[display(
        "server-reported archive size {reported_size} exceeds maximum allowed size of {max_size}"
    )]
    ReportedSizeExceedsMax { reported_size: u64, max_size: u64 },
    #[display(
        "actual downloaded archive size {downloaded_size} exceeds maximum allowed size of {max_size}"
    )]
    DownloadedSizeExceedsMax { downloaded_size: u64, max_size: u64 },
    #[display("downloaded archive hash mismatch for {pkg_id}: expected {expected}, got {got}")]
    HashMismatch {
        pkg_id: PackageId,
        expected: Box<GenericDigest>,
        got: Box<GenericDigest>,
    },
    #[display("failed to create temporary file for downloaded archive")]
    CreateTempFile {
        #[error(source)]
        source: io::Error,
    },
    #[display("failed to write chunk to temporary file for downloaded archive")]
    WriteTempFile {
        #[error(source)]
        source: io::Error,
    },
    #[display("failed to read response body while downloading archive")]
    ReadResponseBody {
        #[error(source)]
        source: reqwest::Error,
    },
    #[display("failed to rewind temporary file for downloaded archive")]
    Rewind {
        #[error(source)]
        source: io::Error,
    },
}

impl From<DownloadErrorReport> for ReportValue<'static> {
    fn from(report: DownloadErrorReport) -> Self {
        Self::BoxedError(report.into())
    }
}

pub(in crate::command::install) async fn download_archive<S>(
    cx: &StepContext<S>,
    pkg_id: &PackageId,
    source: &PackageSource,
) -> Result<File, S::Error>
where
    S: Step,
{
    let cx = cx.with_step(DownloadStep {
        step: Arc::clone(cx.step()),
    });
    let reporter = cx.reporter();
    reporter.report_step(format_args!("Downloading {}...", source.url));

    let response = reqwest::get(source.url.clone())
        .await
        .and_then(Response::error_for_status)
        .map_err(|err| {
            let url = source.url.clone();
            DownloadErrorReport::Get { url, source: err }
        })
        .report_error(reporter)?;

    let len = response.content_length();
    if let Some(len) = len
        && len > cx.config().install.max_archive_size_bytes
    {
        let reported_size = len;
        return Err(
            reporter.report_error(DownloadErrorReport::ReportedSizeExceedsMax {
                reported_size,
                max_size: cx.config().install.max_archive_size_bytes,
            }),
        );
    }
    let hasher = source.hash.hasher();
    let (output, digest) = reporter
        .with_download_progress_bar(len, async |pb| {
            stream_archive_to_tempfile(response.bytes_stream(), hasher, cx.config(), pb).await
        })
        .await
        .report_error(reporter)?;
    if digest != source.hash {
        let pkg_id = pkg_id.clone();
        let expected = Box::new(source.hash.clone());
        let got = Box::new(digest);
        let err = reporter.report_error(DownloadErrorReport::HashMismatch {
            pkg_id,
            expected,
            got,
        });
        return Err(err);
    }
    Ok(output)
}

async fn stream_archive_to_tempfile<S>(
    chunks: S,
    mut hasher: GenericHasher,
    config: &Config,
    pb: &indicatif::ProgressBar,
) -> Result<(File, GenericDigest), DownloadErrorReport>
where
    S: Stream<Item = Result<Bytes, reqwest::Error>>,
{
    let mut chunks = pin!(chunks);

    let output =
        tempfile::tempfile().map_err(|source| DownloadErrorReport::CreateTempFile { source })?;
    let mut output = tokio::fs::File::from_std(output);
    let mut total_size = 0;
    while let Some(chunk) = chunks.next().await {
        let chunk = chunk.map_err(|source| DownloadErrorReport::ReadResponseBody { source })?;
        let chunk_size = chunk.len() as u64;
        total_size += chunk_size;
        if total_size > config.install.max_archive_size_bytes {
            return Err(DownloadErrorReport::DownloadedSizeExceedsMax {
                downloaded_size: total_size,
                max_size: config.install.max_archive_size_bytes,
            });
        }
        hasher.update(&chunk);
        output
            .write_all(&chunk)
            .await
            .map_err(|source| DownloadErrorReport::WriteTempFile { source })?;
        pb.inc(chunk_size);
    }
    let digest = hasher.finalize();
    output
        .rewind()
        .await
        .map_err(|source| DownloadErrorReport::Rewind { source })?;
    let output = output.into_std().await;
    Ok((output, digest))
}

#[cfg(test)]
mod tests {
    use futures_util::stream;
    use indicatif::ProgressBar;
    use sha2::{Digest as _, Sha256};

    use crate::cli::config::InstallConfig;

    use super::*;

    #[tokio::test]
    async fn stream_archive_to_tempfile_rejects_download_size_exceeding_limit() {
        let chunks = stream::once(async { Ok(Bytes::copy_from_slice(b"font")) });
        let config = Config {
            install: InstallConfig {
                max_archive_size_bytes: 3,
                max_extracted_files: 1000,
                max_extracted_file_size_bytes: 50 * 1024 * 1024, // 50 MiB
            },
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
