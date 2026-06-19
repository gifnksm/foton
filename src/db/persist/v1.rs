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

pub(in crate::db::persist) mod types {
    use std::collections::BTreeMap;

    use serde::{Deserialize, Serialize};
    use url::Url;

    use crate::{
        package::{FileRule, IgnoreRule, PackageName, PackageVersion},
        util::{hash::GenericDigest, path::FileName},
    };

    #[derive(Debug, Default, Clone, Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    pub(in crate::db) struct PersistedPackageDb {
        pub(in crate::db::persist) packages: BTreeMap<PackageName, PersistedPackageVersionMap>,
    }

    #[derive(Debug, Clone, Default, Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    pub(in crate::db::persist) struct PersistedPackageVersionMap {
        pub(in crate::db::persist) versions: BTreeMap<PackageVersion, PersistedPackageEntry>,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    pub(in crate::db) struct PersistedPackageEntry {
        pub(in crate::db::persist) installation_state: PersistedInstallationState,
        pub(in crate::db::persist) activation_state: PersistedActivationState,
        pub(in crate::db::persist) definition: PersistedPackageDefinition,
        pub(in crate::db::persist) font_entries: Vec<PersistedFontEntry>,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, derive_more::IsVariant)]
    #[serde(rename_all = "kebab-case")]
    pub(in crate::db::persist) enum PersistedInstallationState {
        Installed,
        IncompleteInstall,
        IncompleteUninstall,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, derive_more::IsVariant)]
    #[serde(rename_all = "kebab-case")]
    pub(in crate::db::persist) enum PersistedActivationState {
        Active,
        Inactive,
        IncompleteActivation,
        IncompleteDeactivation,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    pub(in crate::db::persist) struct PersistedPackageDefinition {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub(in crate::db::persist) display_name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub(in crate::db::persist) description: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        pub(in crate::db::persist) aliases: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub(in crate::db::persist) homepage: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub(in crate::db::persist) repository: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub(in crate::db::persist) license: Option<String>,
        pub(in crate::db::persist) sources: Vec<PersistedPackageSource>,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    pub(in crate::db::persist) struct PersistedPackageSource {
        pub(in crate::db::persist) url: Url,
        pub(in crate::db::persist) hash: GenericDigest,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        pub(in crate::db::persist) files: Vec<FileRule>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        pub(in crate::db::persist) ignore: Vec<IgnoreRule>,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    pub(in crate::db::persist) struct PersistedFontEntry {
        pub(in crate::db::persist) title: String,
        pub(in crate::db::persist) file_name: FileName,
    }
}
