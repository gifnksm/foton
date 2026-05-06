use serde_json::value::RawValue;
use snafu::ResultExt as _;

use super::PersistError;
use crate::db::persist::{DeserializePayloadSnafu, SerializePayloadSnafu};

pub(in crate::db::persist) const VERSION: u32 = 1;

pub(in crate::db::persist) fn deserialize_payload(
    s: &str,
) -> Result<types::PersistedPackageDb, PersistError> {
    serde_json::from_str(s).context(DeserializePayloadSnafu {
        schema_version: VERSION,
    })
}

pub(in crate::db::persist) fn serialize_payload(
    payload: &types::PersistedPackageDb,
) -> Result<Box<RawValue>, PersistError> {
    serde_json::value::to_raw_value(payload).context(SerializePayloadSnafu {
        schema_version: VERSION,
    })
}

pub(in crate::db) mod types {
    use std::{collections::BTreeMap, sync::Arc};

    use serde::{Deserialize, Serialize};

    use crate::package::{PackageManifest, PackageQualifiedName, PackageState, PackageVersion};

    #[derive(Debug, Default, Clone, Serialize, Deserialize)]
    pub(in crate::db) struct PersistedPackageDb {
        pub(in crate::db) packages: BTreeMap<PackageQualifiedName, PersistedPackageVersionMap>,
    }

    #[derive(Debug, Clone, Default, Serialize, Deserialize)]
    pub(in crate::db) struct PersistedPackageVersionMap {
        pub(in crate::db) versions: BTreeMap<PackageVersion, PersistedPackageEntry>,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub(in crate::db) struct PersistedPackageEntry {
        pub(in crate::db) state: PackageState,
        pub(in crate::db) manifest: Arc<PackageManifest>,
    }
}
