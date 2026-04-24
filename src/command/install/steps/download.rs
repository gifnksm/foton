use std::{
    fs::File,
    io::{self, Read, Seek as _, Write as _},
};

use reqwest::{Url, blocking::Response};

use crate::{
    command::install::InstallConfig,
    package::{PackageId, PackageSource},
    util::{
        hash::{GenericDigest, GenericHasher},
        reporter::Reporter,
    },
};

#[derive(Debug, derive_more::Display, derive_more::Error)]
pub(crate) enum DownloadError {
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
        expected: GenericDigest,
        got: GenericDigest,
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
        source: io::Error,
    },
    #[display("failed to rewind temporary file for downloaded archive")]
    Rewind {
        #[error(source)]
        source: io::Error,
    },
}

pub(in crate::command::install) fn download_archive(
    reporter: &Reporter,
    pkg_id: &PackageId,
    source: &PackageSource,
    config: &InstallConfig,
) -> Result<File, Box<DownloadError>> {
    let mut response = reqwest::blocking::get(source.url.clone())
        .and_then(Response::error_for_status)
        .map_err(|err| {
            let url = source.url.clone();
            DownloadError::Get { url, source: err }
        })?;

    let len = response.content_length();
    if let Some(len) = len
        && len > config.max_archive_size_bytes
    {
        let reported_size = len;
        return Err(DownloadError::ReportedSizeExceedsMax {
            reported_size,
            max_size: config.max_archive_size_bytes,
        }
        .into());
    }
    let hasher = source.hash.hasher();
    let (output, digest) = reporter.with_download_progress_bar(len, |pb| {
        stream_archive_to_tempfile(&mut response, hasher, config, pb)
    })?;
    if digest != source.hash {
        let pkg_id = pkg_id.clone();
        let expected = source.hash.clone();
        let got = digest;
        return Err(DownloadError::HashMismatch {
            pkg_id,
            expected,
            got,
        }
        .into());
    }
    Ok(output)
}

fn stream_archive_to_tempfile<R>(
    reader: &mut R,
    mut hasher: GenericHasher,
    config: &InstallConfig,
    pb: &indicatif::ProgressBar,
) -> Result<(File, GenericDigest), Box<DownloadError>>
where
    R: Read,
{
    let mut output =
        tempfile::tempfile().map_err(|source| DownloadError::CreateTempFile { source })?;
    let mut buffer = [0; 8096];
    let mut total_size = 0;
    loop {
        let n = reader
            .read(&mut buffer)
            .map_err(|source| DownloadError::ReadResponseBody { source })?;
        total_size += n as u64;
        if total_size > config.max_archive_size_bytes {
            return Err(DownloadError::DownloadedSizeExceedsMax {
                downloaded_size: total_size,
                max_size: config.max_archive_size_bytes,
            }
            .into());
        }
        if n == 0 {
            break;
        }
        let chunk = &buffer[..n];
        hasher.update(chunk);
        output
            .write_all(chunk)
            .map_err(|source| DownloadError::WriteTempFile { source })?;
        pb.inc(chunk.len() as u64);
    }
    let digest = hasher.finalize();
    output
        .rewind()
        .map_err(|source| DownloadError::Rewind { source })?;
    Ok((output, digest))
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use indicatif::ProgressBar;
    use sha2::{Digest as _, Sha256};

    use super::*;

    #[test]
    fn stream_archive_to_tempfile_rejects_download_size_exceeding_limit() {
        let mut reader = Cursor::new(b"font".to_vec());
        let config = InstallConfig {
            max_archive_size_bytes: 3,
            max_extracted_files: 1000,
            max_extracted_file_size_bytes: 50 * 1024 * 1024, // 50 MiB
        };
        let pb = ProgressBar::hidden();

        let hasher = Sha256::new().into();
        let err = stream_archive_to_tempfile(&mut reader, hasher, &config, &pb).unwrap_err();

        assert!(matches!(
            *err,
            DownloadError::DownloadedSizeExceedsMax { .. }
        ));
    }
}
