use std::{marker::PhantomData, rc::Rc};

use snafu::{ResultExt as _, Snafu};
use windows::Win32::System::Com::{self, COINIT_MULTITHREADED};

#[derive(Debug, Snafu)]
pub(crate) enum ComError {
    #[snafu(display("failed to initialize COM library"))]
    Initialize { source: windows_core::Error },
}

pub(crate) fn init() -> Result<ComGuard, ComError> {
    // SAFETY: `CoInitializeEx` is unsafe because it mutates COM state for the current thread
    // through the FFI boundary. We pass `None` for the reserved pointer as required by the API,
    // and `COINIT_MULTITHREADED` is a valid initialization flag. Each successful call is later
    // balanced by `CoUninitialize` on the same thread via `ComGuard` or the Tokio worker thread
    // stop hook.
    unsafe { Com::CoInitializeEx(None, COINIT_MULTITHREADED) }
        .ok()
        .context(InitializeSnafu)?;
    Ok(ComGuard {
        armed: true,
        _thread_affine: PhantomData,
    })
}

pub(crate) fn uninit() {
    // SAFETY: `CoUninitialize` is unsafe because it tears down COM state for the current thread.
    // This function is only used to balance a prior successful `CoInitializeEx` on the same
    // thread.
    unsafe { Com::CoUninitialize() };
}

#[derive(Debug)]
#[must_use = "keep this guard alive for as long as COM is needed"]
pub(crate) struct ComGuard {
    armed: bool,
    _thread_affine: PhantomData<Rc<()>>,
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
