use std::path::{Path, PathBuf};

use windows::Win32::{
    Foundation::{LPARAM, WPARAM},
    Graphics::Gdi,
    UI::WindowsAndMessaging::{self, HWND_BROADCAST, WM_FONTCHANGE},
};
use windows_core::HSTRING;

#[derive(Debug, derive_more::Display, derive_more::Error)]
pub(crate) enum SessionError {
    #[display("failed to load font into current session: {path}", path = path.display())]
    LoadFont { path: PathBuf },
    #[display("failed to broadcast font change")]
    BroadcastFontChange {
        #[error(source)]
        source: windows_core::Error,
    },
}

pub(crate) fn load_font<P>(font_path: P) -> Result<(), SessionError>
where
    P: AsRef<Path>,
{
    let font_path = font_path.as_ref();
    // SAFETY: This is an unsafe FFI call. We pass a valid path pointer derived from a temporary
    // UTF-16 string that is kept alive for the duration of the call.
    let added = unsafe { Gdi::AddFontResourceW(&HSTRING::from(font_path)) };
    if added == 0 {
        let path = font_path.to_owned();
        return Err(SessionError::LoadFont { path });
    }
    Ok(())
}

pub(crate) fn unload_font<P>(font_path: P)
where
    P: AsRef<Path>,
{
    const MAX_TRIES: usize = 1000;

    let font_path = font_path.as_ref();
    for _ in 0..MAX_TRIES {
        // SAFETY: This is an unsafe FFI call. We pass a valid path pointer derived from a
        // temporary UTF-16 string that is kept alive for the duration of the call.
        let removed = unsafe { Gdi::RemoveFontResourceW(&HSTRING::from(font_path)) }.as_bool();
        if !removed {
            return;
        }
    }
}

pub(crate) fn broadcast_font_change() -> Result<(), SessionError> {
    // SAFETY: This is an unsafe FFI call. We pass only constant scalar arguments.
    unsafe {
        WindowsAndMessaging::SendNotifyMessageW(HWND_BROADCAST, WM_FONTCHANGE, WPARAM(0), LPARAM(0))
    }
    .map_err(|source| SessionError::BroadcastFontChange { source })
}
