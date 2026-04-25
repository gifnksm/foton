use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, derive_more::Display)]
#[serde(rename_all = "kebab-case")]
#[display(rename_all = "kebab-case")]
pub(crate) enum PackageState {
    Installed,
    PendingInstall,
    PendingUninstall,
}
