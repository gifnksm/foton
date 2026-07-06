use std::path::{Path, PathBuf};

use snafu::{IntoError as _, Snafu};
use windows::Win32::{
    Foundation::{self, ERROR_SUCCESS, LPARAM, WPARAM},
    Graphics::Gdi,
    UI::WindowsAndMessaging::{self, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_FONTCHANGE},
};
use windows_core::HSTRING;

#[derive(Debug, Snafu)]
pub(crate) enum SessionError {
    #[snafu(display("failed to load font into current session: {path}", path = path.display()))]
    LoadFont { path: PathBuf },
    #[snafu(display("failed to broadcast font change"))]
    BroadcastFontChange { source: windows_core::Error },
}

pub(crate) fn load_font<P>(font_path: P) -> Result<(), SessionError>
where
    P: AsRef<Path>,
{
    let font_path = font_path.as_ref();
    // SAFETY: `AddFontResourceW` reads a caller-provided NUL-terminated UTF-16 path through the
    // FFI boundary. `HSTRING::from(font_path)` provides such a buffer, and the temporary
    // `HSTRING` remains alive for the full call expression, so the pointer stays valid for the
    // duration of the call.
    let added = unsafe { Gdi::AddFontResourceW(&HSTRING::from(font_path)) };
    snafu::ensure!(added > 0, LoadFontSnafu { path: font_path });
    Ok(())
}

pub(crate) fn unload_font<P>(font_path: P)
where
    P: AsRef<Path>,
{
    const MAX_TRIES: usize = 1000;

    let font_path = font_path.as_ref();
    for _ in 0..MAX_TRIES {
        // SAFETY: `RemoveFontResourceW` reads a caller-provided NUL-terminated UTF-16 path
        // through the FFI boundary. `HSTRING::from(font_path)` provides such a buffer, and the
        // temporary `HSTRING` remains alive for the full call expression, so the pointer stays
        // valid for the duration of the call.
        let removed = unsafe { Gdi::RemoveFontResourceW(&HSTRING::from(font_path)) }.as_bool();
        if !removed {
            return;
        }
    }
}

pub(crate) fn broadcast_font_change() -> Result<(), SessionError> {
    const SEND_MESSAGE_TIMEOUT_MS: u32 = 500;

    // SAFETY: `SetLastError` updates thread-local last-error state and takes a plain scalar value.
    unsafe {
        Foundation::SetLastError(ERROR_SUCCESS);
    }

    // SAFETY: `SendMessageTimeoutW` is an FFI call with only plain scalar arguments. We broadcast
    // the system-defined `WM_FONTCHANGE` message with null payload values and pass no borrowed
    // pointers or buffers, so no caller-owned data must remain valid after the call returns.
    let sent = unsafe {
        WindowsAndMessaging::SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_FONTCHANGE,
            WPARAM(0),
            LPARAM(0),
            SMTO_ABORTIFHUNG,
            SEND_MESSAGE_TIMEOUT_MS,
            None,
        )
    };

    if sent.0 != 0 {
        return Ok(());
    }

    // SAFETY: `GetLastError` reads the thread-local last-error value set by the preceding Win32
    // call sequence and takes no pointers.
    let last_error = unsafe { Foundation::GetLastError() };
    if last_error == ERROR_SUCCESS {
        return Ok(());
    }
    Err(BroadcastFontChangeSnafu.into_error(last_error.into()))
}
