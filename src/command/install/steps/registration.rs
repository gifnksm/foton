use crate::{
    cli::context::StepContext,
    package::{Package, PackageId},
    platform::windows::steps::{registration, unregistration},
    util::reporter::Step,
};

pub(in crate::command::install) fn register_package_fonts<S>(
    cx: &StepContext<S>,
    package: &Package,
) -> Result<RegistrationGuard<S>, S::Error>
where
    S: Step,
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
pub(in crate::command::install) struct RegistrationGuard<S>
where
    S: Step,
{
    armed: bool,
    cx: StepContext<S>,
    pkg_id: PackageId,
}

impl<S> Drop for RegistrationGuard<S>
where
    S: Step,
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
    S: Step,
{
    pub(crate) fn disarm(mut self) {
        self.armed = false;
    }
}
