use snafu::ResultExt as _;

use crate::{
    registry::{FetchRegistryError, RegistryId, RegistryIndex, fetch::OpenIndexSnafu},
    util::path::AbsolutePath,
};

pub(in crate::registry) fn fetch_registry(
    id: &RegistryId,
    path: &AbsolutePath,
) -> Result<RegistryIndex, FetchRegistryError> {
    RegistryIndex::open(id.clone(), path.as_path().to_path_buf())
        .context(OpenIndexSnafu { id, path })
}
