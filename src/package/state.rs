#[derive(Debug, Clone, Copy, PartialEq, Eq, derive_more::Display, derive_more::IsVariant)]
#[display(rename_all = "kebab-case")]
pub(crate) enum InstallationState {
    Installed,
    IncompleteInstall,
    IncompleteUninstall,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, derive_more::Display, derive_more::IsVariant)]
#[display(rename_all = "kebab-case")]
pub(crate) enum ActivationState {
    Active,
    Inactive,
    IncompleteActivation,
    IncompleteDeactivation,
}
