use crate::{
    registry::{FetchRegistryError, RegistryId, RegistryIndex},
    util::path::AbsolutePath,
};

pub(in crate::registry) fn fetch_registry(
    id: &RegistryId,
    path: &AbsolutePath,
) -> Result<RegistryIndex, FetchRegistryError> {
    RegistryIndex::open(id.clone(), path.as_path().to_path_buf()).map_err(|source| {
        let id = id.clone();
        let path = path.clone();
        let source = Box::new(source);
        FetchRegistryError::OpenIndex { id, path, source }
    })
}
