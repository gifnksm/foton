use color_eyre::eyre::{self, WrapErr as _};

use crate::{
    cli::message::warn,
    package::Package,
    platform::windows::{
        registry::{self, RegisteredFont},
        session,
    },
    util::error::FormatErrorChain as _,
};

pub(crate) fn install_package_fonts(app_id: &str, package: &Package) -> eyre::Result<()> {
    if let Err(err) = try_install(app_id, package) {
        // Rollback on failure
        if let Err(rollback_err) = uninstall_package_fonts(app_id, package) {
            let rollback_err = rollback_err.wrap_err(
                "install failed and rollback also failed; partial font registration may remain",
            );
            warn!("{}", rollback_err.format_error_chain());
        }

        return Err(err);
    }
    Ok(())
}

fn try_install(app_id: &str, package: &Package) -> eyre::Result<()> {
    let registered_fonts = package
        .entries()
        .iter()
        .map(|entry| {
            RegisteredFont::new(
                entry.title(),
                package.dirs().fonts_dir().join(entry.file_name()),
            )
        })
        .collect::<eyre::Result<Vec<_>>>()?;
    registry::register_package_fonts(app_id, package.id(), &registered_fonts)?;
    for entry in &registered_fonts {
        if let Err(err) = session::load_font(entry.path()) {
            let err = err.wrap_err(format!("failed to load font into current session: {}; the font was registered persistently but may not be available until next logon", entry.path().display()));
            warn!("{}", err.format_error_chain());
        }
    }
    if let Err(err) = session::broadcast_font_change() {
        let err = err.wrap_err("failed to broadcast font change after install; applications may not see the new font immediately");
        warn!("{}", err.format_error_chain());
    }
    Ok(())
}

pub(crate) fn uninstall_package_fonts(app_id: &str, package: &Package) -> eyre::Result<()> {
    let entries = match registry::list_registered_package_fonts(app_id, package.id()) {
        Ok(entries) => Some(entries),
        Err(err) => {
            let err = err.wrap_err("failed to list registered fonts for package during uninstall");
            warn!("{}", err.format_error_chain());
            None
        }
    };
    if let Some(entries) = entries {
        for entry in entries {
            session::unload_font(entry.path());
        }
    }

    let res = registry::unregister_package_fonts(app_id, package.id())
        .wrap_err("failed to unregister package fonts from registry");

    if let Err(err) = session::broadcast_font_change() {
        let err = err.wrap_err("failed to broadcast font change after uninstall; applications may continue to use stale font information until refresh");
        warn!("{}", err.format_error_chain());
    }

    res
}
