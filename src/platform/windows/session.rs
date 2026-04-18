use std::path::Path;

use color_eyre::eyre::{self, WrapErr as _};
use windows::Win32::{
    Foundation::{LPARAM, WPARAM},
    Graphics::Gdi,
    UI::WindowsAndMessaging::{self, HWND_BROADCAST, WM_FONTCHANGE},
};
use windows_core::HSTRING;

pub(crate) fn load_font(font_path: &Path) -> eyre::Result<()> {
    // SAFETY: This is an unsafe FFI call. We pass a valid path pointer derived from a temporary
    // UTF-16 string that is kept alive for the duration of the call.
    let added = unsafe { Gdi::AddFontResourceW(&HSTRING::from(font_path)) };
    if added == 0 {
        eyre::bail!("Failed to load font: {}", font_path.display());
    }
    Ok(())
}

pub(crate) fn unload_font(font_path: &Path) {
    const MAX_TRIES: usize = 1000;
    for _ in 0..MAX_TRIES {
        // SAFETY: This is an unsafe FFI call. We pass a valid path pointer derived from a
        // temporary UTF-16 string that is kept alive for the duration of the call.
        let removed = unsafe { Gdi::RemoveFontResourceW(&HSTRING::from(font_path)) }.as_bool();
        if !removed {
            return;
        }
    }
}

pub(crate) fn broadcast_font_change() -> eyre::Result<()> {
    // SAFETY: This is an unsafe FFI call. We pass only constant scalar arguments.
    unsafe {
        WindowsAndMessaging::SendNotifyMessageW(HWND_BROADCAST, WM_FONTCHANGE, WPARAM(0), LPARAM(0))
    }
    .wrap_err("Failed to broadcast font change")
}
