use std::sync::Arc;

use crate::{
    package::PackageManifest,
    registry::{RegistryId, RegistryIndex},
};

pub(in crate::engine::resolve) fn collect_registry_matches<E, F>(
    indexes: &[RegistryIndex],
    mut find: F,
) -> Result<Vec<(RegistryId, Arc<PackageManifest>)>, E>
where
    F: FnMut(&RegistryIndex) -> Result<Option<Arc<PackageManifest>>, E>,
{
    let mut matches = vec![];
    for index in indexes {
        if let Some(manifest) = find(index)? {
            matches.push((index.id().clone(), manifest));
        }
    }
    Ok(matches)
}

pub(in crate::engine::resolve) fn into_unique_match<T>(
    mut matches: Vec<T>,
) -> Result<Option<T>, Vec<T>> {
    match matches.len() {
        0 => Ok(None),
        1 => Ok(matches.pop()),
        _ => Err(matches),
    }
}
