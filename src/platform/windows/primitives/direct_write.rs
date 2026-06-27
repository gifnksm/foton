use std::{
    ffi::OsString,
    os::windows::ffi::OsStringExt as _,
    path::{Path, PathBuf},
};

use snafu::{OptionExt as _, ResultExt as _, Snafu};
use windows::Win32::Graphics::DirectWrite::{
    self, DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_PROPERTY_ID, DWRITE_FONT_PROPERTY_ID_FACE_NAME,
    DWRITE_FONT_PROPERTY_ID_FAMILY_NAME, DWRITE_FONT_PROPERTY_ID_PREFERRED_FAMILY_NAME,
    DWRITE_FONT_PROPERTY_ID_TYPOGRAPHIC_FACE_NAME, IDWriteFactory6, IDWriteFontSet,
    IDWriteLocalizedStrings,
};
use windows_core::HSTRING;

use crate::platform::windows::primitives::ui_languages::PreferredUiLanguages;

#[derive(Debug, Snafu)]
pub(crate) enum DirectWriteFactoryError {
    #[snafu(display("failed to create DirectWrite factory"))]
    CreateFactory { source: windows_core::Error },
}

#[derive(Debug)]
pub(crate) struct DirectWriteFactory {
    factory: IDWriteFactory6,
}

impl DirectWriteFactory {
    pub(crate) fn new() -> Result<Self, DirectWriteFactoryError> {
        // SAFETY: This is an unsafe FFI call. We pass only the constant factory type argument.
        let factory = unsafe { DirectWrite::DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED) }
            .context(CreateFactorySnafu)?;
        Ok(Self { factory })
    }
}

#[derive(Debug, Snafu)]
pub(crate) enum DirectWriteFontSetError {
    #[snafu(display("failed to create font set builder"))]
    CreateFontSetBuilder { source: windows_core::Error },
    #[snafu(display("failed to add font file to font set builder: {path}", path = path.display()))]
    AddFontFile {
        path: PathBuf,
        source: windows_core::Error,
    },
    #[snafu(display("failed to create font set for font file: {path}", path = path.display()))]
    CreateFontSet {
        path: PathBuf,
        source: windows_core::Error,
    },
}

#[derive(Debug)]
pub(crate) struct DirectWriteFontSet {
    font_set: IDWriteFontSet,
}

impl DirectWriteFontSet {
    pub(crate) fn new<P>(
        factory: &DirectWriteFactory,
        path: P,
    ) -> Result<DirectWriteFontSet, DirectWriteFontSetError>
    where
        P: AsRef<Path>,
    {
        let path = path.as_ref();

        // SAFETY: This is an unsafe FFI call.
        let font_set_builder =
            unsafe { factory.factory.CreateFontSetBuilder() }.context(CreateFontSetBuilderSnafu)?;

        // SAFETY: This is an unsafe FFI call. We pass a valid path pointer derived from a temporary
        // UTF-16 string that is kept alive for the duration of the call.
        unsafe { font_set_builder.AddFontFile(&HSTRING::from(path)) }
            .context(AddFontFileSnafu { path })?;

        // SAFETY: This is an unsafe FFI call.
        let font_set =
            unsafe { font_set_builder.CreateFontSet() }.context(CreateFontSetSnafu { path })?;

        Ok(DirectWriteFontSet { font_set })
    }

    pub(crate) fn entries(&self) -> impl Iterator<Item = DirectWriteFontSetEntry<'_>> {
        // SAFETY: This is an unsafe FFI call.
        let count = unsafe { self.font_set.GetFontCount() };
        (0..count).map(|index| DirectWriteFontSetEntry {
            font_set: self,
            index,
        })
    }
}

#[derive(Debug, Snafu)]
pub(crate) enum DirectWriteFontSetEntryError {
    #[snafu(display(
        "failed to get font property values for index {index} and property ID {property_id}",
        property_id = property_id.0,
    ))]
    GetFontPropertyValues {
        index: u32,
        property_id: DWRITE_FONT_PROPERTY_ID,
        source: windows_core::Error,
    },
    #[snafu(display("font file is missing family name at index {index}"))]
    MissingFontFamilyName { index: u32 },
    #[snafu(display("font file is missing face name at index {index}"))]
    MissingFontFaceName { index: u32 },
}

#[derive(Debug)]
pub(crate) struct DirectWriteFontSetEntry<'a> {
    font_set: &'a DirectWriteFontSet,
    index: u32,
}

impl DirectWriteFontSetEntry<'_> {
    pub(crate) fn family(
        &self,
    ) -> Result<DirectWriteLocalizedStrings, DirectWriteFontSetEntryError> {
        self.get_property_string_with_fallback(&[
            DWRITE_FONT_PROPERTY_ID_PREFERRED_FAMILY_NAME,
            DWRITE_FONT_PROPERTY_ID_FAMILY_NAME,
        ])?
        .context(MissingFontFamilyNameSnafu { index: self.index })
    }

    pub(crate) fn face(&self) -> Result<DirectWriteLocalizedStrings, DirectWriteFontSetEntryError> {
        self.get_property_string_with_fallback(&[
            DWRITE_FONT_PROPERTY_ID_TYPOGRAPHIC_FACE_NAME,
            DWRITE_FONT_PROPERTY_ID_FACE_NAME,
        ])?
        .context(MissingFontFaceNameSnafu { index: self.index })
    }

    fn get_property_string_with_fallback(
        &self,
        property_ids: &[DWRITE_FONT_PROPERTY_ID],
    ) -> Result<Option<DirectWriteLocalizedStrings>, DirectWriteFontSetEntryError> {
        for &property_id in property_ids {
            if let Some(strings) = self.get_property_strings(property_id)? {
                return Ok(Some(strings));
            }
        }
        Ok(None)
    }

    fn get_property_strings(
        &self,
        property_id: DWRITE_FONT_PROPERTY_ID,
    ) -> Result<Option<DirectWriteLocalizedStrings>, DirectWriteFontSetEntryError> {
        let index = self.index;
        let mut exists = false.into();
        let mut strings = None;

        // SAFETY: This is an unsafe FFI call. We pass valid pointers for the `exists` and `values` out parameters.
        unsafe {
            self.font_set.font_set.GetPropertyValues3(
                index,
                property_id,
                &raw mut exists,
                &raw mut strings,
            )
        }
        .context(GetFontPropertyValuesSnafu { index, property_id })?;

        let Some(strings) = strings else {
            return Ok(None);
        };
        if !exists.as_bool() {
            return Ok(None);
        }

        Ok(Some(DirectWriteLocalizedStrings { strings }))
    }
}

#[derive(Debug, Snafu)]
pub(crate) enum DirectWriteLocalizedStringsError {
    #[snafu(display("failed to query locale name '{locale_name}'", locale_name = locale_name.display()))]
    FindLocaleName {
        index: u32,
        locale_name: OsString,
        source: windows_core::Error,
    },
    #[snafu(display("failed to get string length at index {index}"))]
    GetStringLength {
        index: u32,
        source: windows_core::Error,
    },
    #[snafu(display("failed to get string at index {index}"))]
    GetString {
        index: u32,
        source: windows_core::Error,
    },
}

#[derive(Debug)]
pub(crate) struct DirectWriteLocalizedStrings {
    strings: IDWriteLocalizedStrings,
}

impl DirectWriteLocalizedStrings {
    pub(crate) fn pick_localized_string(
        &self,
        preferred_languages: &PreferredUiLanguages,
    ) -> Result<OsString, DirectWriteLocalizedStringsError> {
        for locale_name in preferred_languages.tags() {
            let mut index = 0;
            let mut exists = false.into();

            // SAFETY: This is an unsafe FFI call. We pass valid pointers for the `index` and `exists` out parameters.
            unsafe {
                self.strings.FindLocaleName(
                    &HSTRING::from(locale_name),
                    &raw mut index,
                    &raw mut exists,
                )
            }
            .context(FindLocaleNameSnafu { index, locale_name })?;

            if exists.as_bool() {
                return self.get_string(index);
            }
        }

        self.get_string(0)
    }

    fn get_string(&self, index: u32) -> Result<OsString, DirectWriteLocalizedStringsError> {
        // SAFETY: This is an unsafe FFI call. We pass a valid pointer for the `length` out parameter.
        let len = unsafe { self.strings.GetStringLength(index) }
            .context(GetStringLengthSnafu { index })?;

        let mut buf = vec![0u16; (len + 1) as usize];
        // SAFETY: This is an unsafe FFI call. We pass a valid pointer to the buffer and its size.
        unsafe { self.strings.GetString(index, &mut buf) }.context(GetStringSnafu { index })?;

        Ok(OsString::from_wide(&buf[..len as usize]))
    }
}
