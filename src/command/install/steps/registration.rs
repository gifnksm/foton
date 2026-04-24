use crate::{
    package::{Package, PackageId},
    platform::windows::steps::{registration, unregistration},
    util::reporter::{Step, StepReporter},
};

pub(in crate::command::install) fn register_package_fonts<'a, 'b, S>(
    reporter: &'a StepReporter<'b, S>,
    app_id: &'a str,
    package: &'a Package,
) -> Result<RegistrationGuard<'a, 'b, S>, S::Error>
where
    S: Step,
{
    let guard = RegistrationGuard {
        armed: true,
        reporter,
        app_id,
        pkg_id: package.id(),
    };
    registration::register_package_fonts(reporter, app_id, package)?;
    Ok(guard)
}

#[must_use]
#[derive(Debug)]
pub(in crate::command::install) struct RegistrationGuard<'a, 'b, S>
where
    S: Step,
{
    armed: bool,
    reporter: &'a StepReporter<'b, S>,
    app_id: &'a str,
    pkg_id: &'a PackageId,
}

impl<S> Drop for RegistrationGuard<'_, '_, S>
where
    S: Step,
{
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        self.reporter.report_info(format_args!(
            "rolling back registration of package fonts..."
        ));
        let _ = unregistration::unregister_package_fonts(self.reporter, self.app_id, self.pkg_id);
    }
}

impl<S> RegistrationGuard<'_, '_, S>
where
    S: Step,
{
    pub(crate) fn disarm(mut self) {
        self.armed = false;
    }
}
