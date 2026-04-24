use windows::Win32::System::Com::{self, COINIT_MULTITHREADED};

#[derive(Debug, derive_more::Display, derive_more::Error)]
pub(crate) enum ComError {
    #[display("failed to initialize COM library")]
    Initialize {
        #[error(source)]
        source: windows_core::Error,
    },
}

pub(crate) fn init() -> Result<ComGuard, ComError> {
    // SAFETY: This is an unsafe FFI call. We pass a null reserved pointer and a valid
    // COM initialization flag constant.
    unsafe { Com::CoInitializeEx(None, COINIT_MULTITHREADED) }
        .ok()
        .map_err(|source| ComError::Initialize { source })?;
    Ok(ComGuard { armed: true })
}

pub(crate) fn uninit() {
    // SAFETY: This is an unsafe FFI call. We call it with no arguments, as required.
    unsafe { Com::CoUninitialize() };
}

#[derive(Debug)]
#[must_use = "keep this guard alive for as long as COM is needed"]
pub(crate) struct ComGuard {
    armed: bool,
}

impl Drop for ComGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        uninit();
    }
}

impl ComGuard {
    pub(crate) fn disarm(mut self) {
        self.armed = false;
    }
}
