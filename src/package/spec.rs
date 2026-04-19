use reqwest::Url;

use crate::{package::PackageId, util::hash::Sha256Digest};

#[derive(Debug, Clone)]
pub(crate) struct PackageSpec {
    pub(crate) id: PackageId,
    pub(crate) url: Url,
    pub(crate) sha256: Sha256Digest,
}
