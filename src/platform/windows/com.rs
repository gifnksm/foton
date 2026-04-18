use color_eyre::eyre::{self, WrapErr as _};
use windows::Win32::System::Com::{self, COINIT_MULTITHREADED};

pub(crate) fn init() -> eyre::Result<ComGuard> {
    // SAFETY: This is an unsafe FFI call. We pass a null reserved pointer and a valid
    // COM initialization flag constant.
    unsafe { Com::CoInitializeEx(None, COINIT_MULTITHREADED) }
        .ok()
        .wrap_err("failed to initialize COM library")?;
    Ok(ComGuard)
}

#[derive(Debug)]
#[must_use = "keep this guard alive for as long as COM is needed"]
pub(crate) struct ComGuard;

impl Drop for ComGuard {
    fn drop(&mut self) {
        // SAFETY: This is an unsafe FFI call. We call it with no arguments, as required.
        unsafe { Com::CoUninitialize() };
    }
}
