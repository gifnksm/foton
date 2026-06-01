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
pub(crate) enum InstallationState {
    #[display("installed")]
    Installed,
    #[display("incomplete-install")]
    IncompleteInstall,
    #[display("incomplete-uninstall")]
    IncompleteUninstall,
}
