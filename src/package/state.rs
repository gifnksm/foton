use serde::{Deserialize, Serialize};

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    derive_more::Display,
    derive_more::IsVariant,
)]
#[serde(rename_all = "kebab-case")]
#[display(rename_all = "kebab-case")]
pub(crate) enum InstallationState {
    Installed,
    IncompleteInstall,
    IncompleteUninstall,
}

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    derive_more::Display,
    derive_more::IsVariant,
)]
#[serde(rename_all = "kebab-case")]
#[display(rename_all = "kebab-case")]
pub(crate) enum ActivationState {
    Active,
    Inactive,
    IncompleteActivate,
    IncompleteDeactivate,
}
