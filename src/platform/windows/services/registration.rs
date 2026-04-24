use crate::{
    package::{Package, PackageId},
    platform::windows::primitives::{
        registry::{self, RegisteredFont, RegistryError},
        session::{self, SessionError},
    },
    util::{
        path::AbsolutePath,
        reporter::{ReportErrorExt as _, Reporter},
    },
};

#[derive(Debug, derive_more::Display, derive_more::Error)]
pub(crate) enum RegistrationWarning {
    #[display(
        "failed to load font into current session: {path}; the font was registered persistently but may not be available until next logon",
        path = path.display()
    )]
    LoadFont {
        path: AbsolutePath,
        #[error(source)]
        source: SessionError,
    },
    #[display("failed to list registered package fonts for the package")]
    ListInstalledFonts {
        #[error(source)]
        source: RegistryError,
    },
    #[display(
        "failed to broadcast font change after install; applications may not see the new font immediately"
    )]
    BroadcastFontAfterInstall {
        #[error(source)]
        source: SessionError,
    },
    #[display(
        "failed to broadcast font change after uninstall; applications may continue to use stale font information until refresh"
    )]
    BroadcastFontAfterUninstall {
        #[error(source)]
        source: SessionError,
    },
}

#[derive(Debug, derive_more::Display, derive_more::Error)]
pub(crate) enum RegistrationError {
    #[display("failed to register package fonts in the registry")]
    RegisterFontsInRegistry {
        #[error(source)]
        source: RegistryError,
    },
    #[display("failed to unregister package fonts from the registry")]
    UnregisterFontsFromRegistry {
        #[error(source)]
        source: RegistryError,
    },
}

pub(crate) fn register_package_fonts(
    reporter: &Reporter,
    app_id: &str,
    package: &Package,
) -> Result<(), RegistrationError> {
    let fonts_dir = package.dirs().fonts_dir();
    let registered_fonts = package
        .entries()
        .iter()
        .map(|entry| RegisteredFont::new(entry.title(), fonts_dir.join(entry.file_name())))
        .collect::<Vec<_>>();

    // Report fatal errors at the point of failure so they stay ordered relative to
    // warnings emitted by later best-effort steps in this function. Callers should
    // return the error without reporting it again.
    registry::register_package_fonts(app_id, package.id(), &registered_fonts)
        .map_err(|source| RegistrationError::RegisterFontsInRegistry { source })
        .report_err_as_error(reporter)?;

    for entry in &registered_fonts {
        let _ = session::load_font(entry.path())
            .map_err(|source| {
                let path = entry.path().clone();
                RegistrationWarning::LoadFont { path, source }
            })
            .report_err_as_warn(reporter);
    }

    let _ = session::broadcast_font_change()
        .map_err(|source| RegistrationWarning::BroadcastFontAfterInstall { source })
        .report_err_as_warn(reporter);

    Ok(())
}

pub(crate) fn unregister_package_fonts(
    reporter: &Reporter,
    app_id: &str,
    pkg_id: &PackageId,
) -> Result<(), RegistrationError> {
    let entries = registry::list_registered_package_fonts(app_id, pkg_id)
        .map_err(|source| RegistrationWarning::ListInstalledFonts { source })
        .report_err_as_warn(reporter)
        .ok();

    if let Some(entries) = entries {
        for entry in entries {
            session::unload_font(entry.path());
        }
    }

    // Report fatal errors at the point of failure so they stay ordered relative to
    // warnings emitted by later best-effort steps in this function. Callers should
    // return the error without reporting it again.
    let res = registry::unregister_package_fonts(app_id, pkg_id)
        .map_err(|source| RegistrationError::UnregisterFontsFromRegistry { source })
        .report_err_as_error(reporter);

    let _ = session::broadcast_font_change()
        .map_err(|source| RegistrationWarning::BroadcastFontAfterUninstall { source })
        .report_err_as_warn(reporter);

    res
}
