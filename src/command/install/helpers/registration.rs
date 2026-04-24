use crate::{
    command::InstallError,
    package::{Package, PackageId},
    platform::windows::services::registration,
    util::reporter::Reporter,
};

pub(crate) fn register_package_fonts<'a>(
    reporter: &'a Reporter,
    app_id: &'a str,
    package: &'a Package,
) -> Result<RegistrationGuard<'a>, Box<InstallError>> {
    let guard = RegistrationGuard {
        armed: true,
        reporter,
        app_id,
        pkg_id: package.id(),
    };
    registration::register_package_fonts(reporter, app_id, package).map_err(|source| {
        let pkg_id = package.id().clone();
        InstallError::Registration { pkg_id, source }
    })?;
    Ok(guard)
}

#[must_use]
#[derive(Debug)]
pub(crate) struct RegistrationGuard<'a> {
    armed: bool,
    reporter: &'a Reporter,
    app_id: &'a str,
    pkg_id: &'a PackageId,
}

impl Drop for RegistrationGuard<'_> {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let _ = registration::unregister_package_fonts(self.reporter, self.app_id, self.pkg_id);
    }
}

impl RegistrationGuard<'_> {
    pub(crate) fn disarm(mut self) {
        self.armed = false;
    }
}
