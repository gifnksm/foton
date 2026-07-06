use color_eyre::eyre::{self, ensure};
use serde::Deserialize;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, derive_more::Display, derive_more::IsVariant, Deserialize,
)]
#[display(rename_all = "kebab-case")]
#[serde(rename_all = "kebab-case")]
pub(in crate::scenario) enum InstallationState {
    Installed,
    IncompleteInstall,
    IncompleteUninstall,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, derive_more::Display, derive_more::IsVariant, Deserialize,
)]
#[display(rename_all = "kebab-case")]
#[serde(rename_all = "kebab-case")]
pub(in crate::scenario) enum ActivationState {
    Active,
    Inactive,
    IncompleteActivation,
    IncompleteDeactivation,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(in crate::scenario) struct ListPackageEntry {
    #[serde(rename = "package-id")]
    pub(in crate::scenario) pkg_id: String,
    pub(in crate::scenario) installation_state: InstallationState,
    pub(in crate::scenario) activation_state: ActivationState,
}

impl ListPackageEntry {
    pub(in crate::scenario) fn name(&self) -> &str {
        let (name, _version) = self.pkg_id.split_once('@').unwrap();
        name
    }

    pub(in crate::scenario) fn version(&self) -> &str {
        let (_name, version) = self.pkg_id.split_once('@').unwrap();
        version
    }

    #[track_caller]
    pub(in crate::scenario) fn ensure_name<P>(&self, predicate: P) -> eyre::Result<&Self>
    where
        P: FnOnce(&str) -> bool,
    {
        let name = self.name();
        ensure!(
            predicate(name),
            "unexpected package name for `{}`: `{}`",
            self.pkg_id,
            name,
        );
        Ok(self)
    }

    #[track_caller]
    pub(in crate::scenario) fn ensure_version<P>(&self, predicate: P) -> eyre::Result<&Self>
    where
        P: FnOnce(&str) -> bool,
    {
        let version = self.version();
        ensure!(
            predicate(version),
            "unexpected package version for `{}`: `{}`",
            self.pkg_id,
            version,
        );
        Ok(self)
    }

    #[track_caller]
    pub(in crate::scenario) fn ensure_installation_state<P>(
        &self,
        predicate: P,
    ) -> eyre::Result<&Self>
    where
        P: FnOnce(InstallationState) -> bool,
    {
        ensure!(
            predicate(self.installation_state),
            "unexpected installation state for `{}`: `{}`",
            self.pkg_id,
            self.installation_state,
        );
        Ok(self)
    }

    #[track_caller]
    pub(in crate::scenario) fn ensure_activation_state<P>(
        &self,
        predicate: P,
    ) -> eyre::Result<&Self>
    where
        P: FnOnce(ActivationState) -> bool,
    {
        ensure!(
            predicate(self.activation_state),
            "unexpected activation state for `{}`: `{}`",
            self.pkg_id,
            self.activation_state,
        );
        Ok(self)
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(in crate::scenario) struct ListFontEntry {
    pub(in crate::scenario) location: FontLocation,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, derive_more::Display, derive_more::IsVariant, Deserialize,
)]
#[display(rename_all = "kebab-case")]
#[serde(rename_all = "kebab-case", rename_all_fields = "kebab-case")]
pub(in crate::scenario) enum FontLocationType {
    PackageDir,
    SystemDir,
    UserDir,
    Unknown,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(in crate::scenario) struct FontLocation {
    #[serde(rename = "type")]
    pub(in crate::scenario) ty: FontLocationType,
    #[serde(rename = "package-id", default)]
    pub(in crate::scenario) pkg_id: Option<String>,
}
