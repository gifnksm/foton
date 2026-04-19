use std::path::Path;

use color_eyre::eyre::{self, WrapErr as _};

use crate::{
    package::{FontEntry, Package},
    platform::windows::{
        font_info::{self, FontInspector},
        registry::{self, RegisteredFont},
        session,
    },
    util::error::{IgnoreError as _, MessageResultExt as _},
};

pub(crate) fn install_package_fonts(app_id: &str, package: &Package) -> eyre::Result<()> {
    if let Err(err) = try_install(app_id, package) {
        // Rollback on failure
        uninstall_package_fonts(app_id, package)
            .wrap_err(
                "install failed and rollback also failed; partial font registration may remain",
            )
            .ignore_err_with_error();
        return Err(err);
    }
    Ok(())
}

fn try_install(app_id: &str, package: &Package) -> eyre::Result<()> {
    let fonts_dir = package.dirs().fonts_dir();
    let registered_fonts = package
        .entries()
        .iter()
        .map(|entry| {
            RegisteredFont::new(entry.title(), fonts_dir.join(entry.file_name())).wrap_err_with(
                || {
                    format!(
                        "failed to create RegisteredFont for `{}`; this is a bug",
                        entry.title()
                    )
                },
            )
        })
        .collect::<eyre::Result<Vec<_>>>()?;

    registry::register_package_fonts(app_id, package.id(), &registered_fonts)
        .wrap_err("failed to register fonts in the registry")?;

    for entry in &registered_fonts {
        session::load_font(entry.path()).wrap_err_with(|| {
            format!("failed to load font into current session: {}; the font was registered persistently but may not be available until next logon", entry.path().display())
        }).ignore_err_with_warn();
    }

    session::broadcast_font_change()
        .wrap_err("failed to broadcast font change after install; applications may not see the new font immediately")
        .ignore_err_with_warn();

    Ok(())
}

pub(crate) fn uninstall_package_fonts(app_id: &str, package: &Package) -> eyre::Result<()> {
    let entries = registry::list_registered_package_fonts(app_id, package.id())
        .wrap_err("failed to list registered fonts for the package during uninstall")
        .ok_with_warn();

    if let Some(entries) = entries {
        for entry in entries {
            session::unload_font(entry.path());
        }
    }

    let res = registry::unregister_package_fonts(app_id, package.id())
        .wrap_err("failed to unregister package fonts from the registry");

    session::broadcast_font_change()
        .wrap_err("failed to broadcast font change after uninstall; applications may continue to use stale font information until refresh")
        .ignore_err_with_warn();

    res
}

#[derive(Debug)]
pub(crate) struct FontValidator {
    inspector: FontInspector,
}

impl FontValidator {
    pub(crate) fn new() -> eyre::Result<Self> {
        let inspector = FontInspector::new()?;
        Ok(Self { inspector })
    }

    pub(crate) fn validate_font(
        &self,
        fonts_dir: &Path,
        file_name: &str,
    ) -> eyre::Result<Option<FontEntry>> {
        let path = fonts_dir.join(file_name);
        let supported = self
            .inspector
            .is_supported_font_file(&path)
            .wrap_err_with(|| {
                format!(
                    "failed to check if font file is supported: {}",
                    path.display()
                )
            })?;

        if !supported {
            return Ok(None);
        }

        let title = font_info::get_font_title(&path)
            .wrap_err_with(|| format!("failed to get font title for file: {}", path.display()))?
            .unwrap_or_else(|| file_name.to_owned());

        Ok(Some(FontEntry::new(title, file_name)?))
    }
}
