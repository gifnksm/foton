use crate::{
    package::{Package, PackageId},
    platform::windows::{
        registry::{self, RegisteredFont, RegistryError},
        session::{self, SessionError},
    },
    util::{
        path::AbsolutePath,
        reporter::{ReportErrorExt as _, Reporter},
    },
};

#[derive(Debug, derive_more::Display, derive_more::Error)]
pub(crate) enum PlatformInstallError {
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
    #[display("failed to list registered package fonts for the package")]
    ListInstalledFonts {
        #[error(source)]
        source: RegistryError,
    },
    #[display(
        "failed to load font into current session: {path}; the font was registered persistently but may not be available until next logon",
        path = path.display()
    )]
    LoadFont {
        path: AbsolutePath,
        #[error(source)]
        source: SessionError,
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

pub(crate) fn install_package_fonts(
    reporter: &mut Reporter<'_>,
    app_id: &str,
    package: &Package,
) -> Result<(), PlatformInstallError> {
    let fonts_dir = package.dirs().fonts_dir();
    let registered_fonts = package
        .entries()
        .iter()
        .map(|entry| RegisteredFont::new(entry.title(), fonts_dir.join(entry.file_name())))
        .collect::<Vec<_>>();

    registry::register_package_fonts(app_id, package.id(), &registered_fonts)
        .map_err(|source| PlatformInstallError::RegisterFontsInRegistry { source })?;

    for entry in &registered_fonts {
        let _ = session::load_font(entry.path())
            .map_err(|source| {
                let path = entry.path().clone();
                PlatformInstallError::LoadFont { path, source }
            })
            .report_err_as_warn(reporter);
    }

    let _ = session::broadcast_font_change()
        .map_err(|source| PlatformInstallError::BroadcastFontAfterInstall { source })
        .report_err_as_warn(reporter);

    Ok(())
}

pub(crate) fn uninstall_package_fonts(
    reporter: &mut Reporter<'_>,
    app_id: &str,
    pkg_id: &PackageId,
) -> Result<(), PlatformInstallError> {
    let entries = registry::list_registered_package_fonts(app_id, pkg_id)
        .map_err(|source| PlatformInstallError::ListInstalledFonts { source })
        .report_err_as_warn(reporter)
        .ok();

    if let Some(entries) = entries {
        for entry in entries {
            session::unload_font(entry.path());
        }
    }

    let res = registry::unregister_package_fonts(app_id, pkg_id)
        .map_err(|source| PlatformInstallError::UnregisterFontsFromRegistry { source })
        .report_err_as_error(reporter);

    let _ = session::broadcast_font_change()
        .map_err(|source| PlatformInstallError::BroadcastFontAfterUninstall { source })
        .report_err_as_warn(reporter);

    res
}
