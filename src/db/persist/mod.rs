use std::io;

use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;
use snafu::{ResultExt as _, Snafu};

mod v1;

use v1 as latest;

use crate::package::{
    ActivationState, FontEntry, InstallationState, PackageDefinition, PackageId, PackageName,
    PackageSource, PackageVersion,
};

pub(in crate::db) use self::latest::types::*;

#[derive(Debug, Serialize, Deserialize)]
struct Envelope {
    schema_version: u32,
    payload: Box<RawValue>,
}

#[derive(Debug, Snafu)]
pub(crate) enum PersistError {
    #[snafu(display("failed to deserialize database envelope"))]
    DeserializeEnvelope { source: serde_json::Error },
    #[snafu(display("unknown database schema version: {}", schema_version))]
    UnknownSchemaVersion { schema_version: u32 },
    #[snafu(display("failed to deserialize database payload (schema version {schema_version})"))]
    DeserializePayload {
        schema_version: u32,
        source: serde_json::Error,
    },
    #[snafu(display("failed to serialize database payload (schema version {schema_version})"))]
    SerializePayload {
        schema_version: u32,
        source: serde_json::Error,
    },
    #[snafu(display("failed to serialize database envelope"))]
    SerializeEnvelope { source: serde_json::Error },
}

pub(in crate::db) fn from_reader<R>(reader: R) -> Result<PersistedPackageDb, PersistError>
where
    R: io::Read,
{
    let Envelope {
        schema_version,
        payload,
    }: Envelope = serde_json::from_reader(reader).context(DeserializeEnvelopeSnafu)?;

    let payload = match schema_version {
        v1::VERSION => v1::deserialize_payload(payload.get())?,
        schema_version => return Err(UnknownSchemaVersionSnafu { schema_version }.build()),
    };

    Ok(payload)
}

pub(in crate::db) fn to_writer<W>(
    writer: W,
    payload: &PersistedPackageDb,
) -> Result<(), PersistError>
where
    W: io::Write,
{
    let envelope = Envelope {
        schema_version: latest::VERSION,
        payload: latest::serialize_payload(payload)?,
    };

    serde_json::to_writer(writer, &envelope).context(SerializeEnvelopeSnafu)?;

    Ok(())
}

impl PersistedPackageDb {
    pub(in crate::db) fn entry<'a>(
        &'a self,
        pkg_id: &PackageId,
    ) -> Option<&'a PersistedPackageEntry> {
        self.packages
            .get(pkg_id.name())?
            .versions
            .get(pkg_id.version())
    }

    pub(in crate::db) fn entry_mut<'a>(
        &'a mut self,
        pkg_id: &PackageId,
    ) -> Option<&'a mut PersistedPackageEntry> {
        self.packages
            .get_mut(pkg_id.name())?
            .versions
            .get_mut(pkg_id.version())
    }

    pub(in crate::db) fn all_entries(
        &self,
    ) -> impl Iterator<Item = (&PackageName, &PackageVersion, &PersistedPackageEntry)> {
        self.packages.iter().flat_map(|(pkg_name, version_map)| {
            version_map
                .versions
                .iter()
                .map(move |(pkg_version, entry)| (pkg_name, pkg_version, entry))
        })
    }

    pub(in crate::db) fn entries_by_name<'a>(
        &'a self,
        pkg_name: &PackageName,
    ) -> impl Iterator<Item = (&'a PackageVersion, &'a PersistedPackageEntry)> {
        self.packages
            .get(pkg_name)
            .into_iter()
            .flat_map(|version_map| version_map.versions.iter())
    }

    pub(in crate::db) fn insert_entry(
        &mut self,
        pkg_id: &PackageId,
        entry: PersistedPackageEntry,
    ) -> Option<PersistedPackageEntry> {
        let pkg_name = pkg_id.name().clone();
        let pkg_version = pkg_id.version().clone();
        self.packages
            .entry(pkg_name)
            .or_default()
            .versions
            .insert(pkg_version, entry)
    }

    pub(in crate::db) fn remove_entry(
        &mut self,
        pkg_id: &PackageId,
    ) -> Option<PersistedPackageEntry> {
        let version_map = self.packages.get_mut(pkg_id.name())?;
        let entry = version_map.versions.remove(pkg_id.version())?;
        if version_map.versions.is_empty() {
            self.packages.remove(pkg_id.name());
        }
        Some(entry)
    }
}

impl PersistedPackageEntry {
    pub(in crate::db) fn new(
        installation_state: InstallationState,
        activation_state: ActivationState,
        definition: &PackageDefinition,
    ) -> Self {
        Self {
            installation_state: installation_state.into(),
            activation_state: activation_state.into(),
            definition: definition.into(),
            font_entries: vec![],
        }
    }

    pub(in crate::db) fn installation_state(&self) -> InstallationState {
        self.installation_state.into()
    }

    pub(in crate::db) fn set_installation_state(&mut self, state: InstallationState) {
        self.installation_state = state.into();
    }

    pub(in crate::db) fn activation_state(&self) -> ActivationState {
        self.activation_state.into()
    }

    pub(in crate::db) fn set_activation_state(&mut self, state: ActivationState) {
        self.activation_state = state.into();
    }

    pub(in crate::db) fn make_definition(&self, pkg_id: PackageId) -> PackageDefinition {
        self.definition.make_definition(pkg_id)
    }

    pub(in crate::db) fn font_entries(&self) -> impl Iterator<Item = FontEntry> + '_ {
        self.font_entries.iter().map(Into::into)
    }

    pub(in crate::db) fn set_font_entries(&mut self, entries: &[FontEntry]) {
        self.font_entries = entries.iter().map(Into::into).collect();
    }
}

impl From<PersistedInstallationState> for InstallationState {
    fn from(state: PersistedInstallationState) -> Self {
        match state {
            PersistedInstallationState::Installed => Self::Installed,
            PersistedInstallationState::IncompleteInstall => Self::IncompleteInstall,
            PersistedInstallationState::IncompleteUninstall => Self::IncompleteUninstall,
        }
    }
}

impl From<InstallationState> for PersistedInstallationState {
    fn from(state: InstallationState) -> Self {
        match state {
            InstallationState::Installed => Self::Installed,
            InstallationState::IncompleteInstall => Self::IncompleteInstall,
            InstallationState::IncompleteUninstall => Self::IncompleteUninstall,
        }
    }
}

impl From<PersistedActivationState> for ActivationState {
    fn from(state: PersistedActivationState) -> Self {
        match state {
            PersistedActivationState::Active => Self::Active,
            PersistedActivationState::Inactive => Self::Inactive,
            PersistedActivationState::IncompleteActivation => Self::IncompleteActivation,
            PersistedActivationState::IncompleteDeactivation => Self::IncompleteDeactivation,
        }
    }
}

impl From<ActivationState> for PersistedActivationState {
    fn from(state: ActivationState) -> Self {
        match state {
            ActivationState::Active => Self::Active,
            ActivationState::IncompleteActivation => Self::IncompleteActivation,
            ActivationState::IncompleteDeactivation => Self::IncompleteDeactivation,
            ActivationState::Inactive => Self::Inactive,
        }
    }
}

impl From<&PackageDefinition> for PersistedPackageDefinition {
    fn from(value: &PackageDefinition) -> Self {
        Self {
            display_name: value.display_name.clone(),
            description: value.description.clone(),
            aliases: value.aliases.clone(),
            faces: value.faces.clone(),
            homepage: value.homepage.clone(),
            repository: value.repository.clone(),
            license: value.license.clone(),
            sources: value.sources.iter().map(Into::into).collect(),
        }
    }
}

impl PersistedPackageDefinition {
    pub(in crate::db::persist) fn make_definition(&self, pkg_id: PackageId) -> PackageDefinition {
        PackageDefinition {
            id: pkg_id,
            display_name: self.display_name.clone(),
            description: self.description.clone(),
            aliases: self.aliases.clone(),
            faces: self.faces.clone(),
            homepage: self.homepage.clone(),
            repository: self.repository.clone(),
            license: self.license.clone(),
            sources: self.sources.iter().map(Into::into).collect(),
        }
    }
}

impl From<&PackageSource> for PersistedPackageSource {
    fn from(value: &PackageSource) -> Self {
        Self {
            url: value.url.clone(),
            hash: value.hash.clone(),
            include: value.include.clone(),
            exclude: value.exclude.clone(),
        }
    }
}

impl From<&PersistedPackageSource> for PackageSource {
    fn from(value: &PersistedPackageSource) -> Self {
        Self {
            url: value.url.clone(),
            hash: value.hash.clone(),
            include: value.include.clone(),
            exclude: value.exclude.clone(),
        }
    }
}

impl From<&FontEntry> for PersistedFontEntry {
    fn from(value: &FontEntry) -> Self {
        Self {
            title: value.title().to_owned(),
            file_name: value.file_name().clone(),
        }
    }
}

impl From<&PersistedFontEntry> for FontEntry {
    fn from(value: &PersistedFontEntry) -> Self {
        Self::new(value.title.clone(), value.file_name.clone())
    }
}
