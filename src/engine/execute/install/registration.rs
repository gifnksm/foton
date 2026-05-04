use crate::{
    cli::{context::ReportContext, reporter::ReportScope},
    package::{Package, PackageId},
    platform::windows::steps::{registration, unregistration},
};

pub(super) fn register_package_fonts<S>(
    cx: &ReportContext<S>,
    package: &Package,
) -> Result<RegistrationGuard<S>, S::Error>
where
    S: ReportScope,
{
    let guard = RegistrationGuard {
        armed: true,
        cx: cx.clone(),
        pkg_id: package.id().clone(),
    };
    registration::register_package_fonts(cx, package)?;
    Ok(guard)
}

#[must_use]
#[derive(Debug)]
pub(super) struct RegistrationGuard<S>
where
    S: ReportScope,
{
    armed: bool,
    cx: ReportContext<S>,
    pkg_id: PackageId,
}

impl<S> Drop for RegistrationGuard<S>
where
    S: ReportScope,
{
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        self.cx.reporter().report_info(format_args!(
            "rolling back registration of package fonts..."
        ));
        let _ = unregistration::unregister_package_fonts(&self.cx, &self.pkg_id);
    }
}

impl<S> RegistrationGuard<S>
where
    S: ReportScope,
{
    pub(super) fn disarm(mut self) {
        self.armed = false;
    }
}
