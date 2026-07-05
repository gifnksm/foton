use color_eyre::eyre::{self, WrapErr as _};
use windows::{
    Win32::{
        Foundation::{LPARAM, WPARAM},
        UI::WindowsAndMessaging::{self, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_SETTINGCHANGE},
    },
    core::w,
};
use windows_registry::CURRENT_USER;

pub(crate) fn set_user_envs<I, N, V>(envs: I) -> eyre::Result<()>
where
    I: IntoIterator<Item = (N, V)>,
    N: AsRef<str>,
    V: AsRef<str>,
{
    const REG_KEY_ENVIRONMENT: &str = "Environment";

    let reg_key = CURRENT_USER
        .create(REG_KEY_ENVIRONMENT)
        .wrap_err_with(|| format!(r"failed to create HKCU\{REG_KEY_ENVIRONMENT}"))?;

    for (name, value) in envs {
        let name = name.as_ref();
        let value = value.as_ref();
        reg_key
            .set_string(name, value)
            .wrap_err_with(|| format!(r"failed to set HKCU\{REG_KEY_ENVIRONMENT}\{name}"))?;
    }

    // SAFETY: `SendMessageTimeoutW` is an FFI call with only plain scalar arguments. We broadcast
    // the system-defined `WM_SETTINGCHANGE` message and pass `w!("Environment")`, which yields a
    // process-lifetime, NUL-terminated UTF-16 string literal, so the `LPARAM` pointer remains
    // valid for the duration of the call. The call does not write through any caller-provided
    // pointers because we pass `None` for the optional result out-parameter.
    unsafe {
        let _ = WindowsAndMessaging::SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            WPARAM(0),
            LPARAM(w!("Environment").as_ptr() as isize),
            SMTO_ABORTIFHUNG,
            5000,
            None,
        );
    }

    Ok(())
}
