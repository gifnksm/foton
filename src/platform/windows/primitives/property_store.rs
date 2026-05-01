use std::{ffi::OsString, os::windows::ffi::OsStringExt as _, path::PathBuf};

use snafu::{ResultExt as _, Snafu};
use windows::Win32::{
    Foundation::PROPERTYKEY,
    Storage::EnhancedStorage,
    UI::Shell::PropertiesSystem::{self, GPS_DEFAULT, IPropertyStore},
};
use windows_core::{BSTR, HSTRING};

#[derive(Debug, Snafu)]
pub(crate) enum PropertyStoreError {
    #[snafu(display("failed to get property store for file: {path}", path = path.display()))]
    GetPropertyStoreForFile {
        path: PathBuf,
        source: windows_core::Error,
    },
    #[snafu(display("failed to get value for key `{key}` from property store of file: {path}", path = path.display()))]
    GetPropertyStoreValue {
        path: PathBuf,
        key: PropertyStoreKey,
        source: windows_core::Error,
    },
    #[snafu(display("failed to convert property store value for key `{key}` to string: {path}", path = path.display()))]
    ConvertPropertyStoreValueToString {
        path: PathBuf,
        key: PropertyStoreKey,
        source: windows_core::Error,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, derive_more::Display)]
pub(crate) enum PropertyStoreKey {
    #[display("Title")]
    Title,
}

impl PropertyStoreKey {
    fn as_property_key(self) -> &'static PROPERTYKEY {
        match self {
            Self::Title => &EnhancedStorage::PKEY_Title,
        }
    }
}

#[derive(Debug)]
pub(crate) struct PropertyStore {
    path: PathBuf,
    store: IPropertyStore,
}

impl PropertyStore {
    pub(crate) fn new<P>(path: P) -> Result<Self, PropertyStoreError>
    where
        P: Into<PathBuf>,
    {
        let path = path.into();
        // SAFETY: This is an unsafe FFI call. We pass a valid path pointer derived from a
        // temporary UTF-16 string that is kept alive for the duration of the call.
        let store: IPropertyStore = unsafe {
            PropertiesSystem::SHGetPropertyStoreFromParsingName(
                &HSTRING::from(path.as_path()),
                None,
                GPS_DEFAULT,
            )
        }
        .context(GetPropertyStoreForFileSnafu { path: &path })?;
        Ok(Self { path, store })
    }

    pub(crate) fn get_property_as_os_string(
        &self,
        key: PropertyStoreKey,
    ) -> Result<OsString, PropertyStoreError> {
        // SAFETY: This is an unsafe FFI call. We pass a valid pointer to the constant property key.
        let value = unsafe { self.store.GetValue(key.as_property_key()) }.context(
            GetPropertyStoreValueSnafu {
                path: &self.path,
                key,
            },
        )?;
        let value = BSTR::try_from(&value).context(ConvertPropertyStoreValueToStringSnafu {
            path: &self.path,
            key,
        })?;
        Ok(OsString::from_wide(&value))
    }
}
