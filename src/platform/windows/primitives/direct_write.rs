use std::{
    ffi::OsString,
    iter::FusedIterator,
    os::windows::ffi::OsStringExt as _,
    path::{Path, PathBuf},
    ptr,
    range::{Range, RangeIter},
};

use snafu::{IntoError as _, OptionExt as _, ResultExt as _, Snafu};
use windows::Win32::{
    Foundation::E_NOINTERFACE,
    Graphics::DirectWrite::{
        self, DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_PROPERTY_ID,
        DWRITE_FONT_PROPERTY_ID_FACE_NAME, DWRITE_FONT_PROPERTY_ID_FAMILY_NAME,
        DWRITE_FONT_PROPERTY_ID_PREFERRED_FAMILY_NAME,
        DWRITE_FONT_PROPERTY_ID_TYPOGRAPHIC_FACE_NAME, IDWriteFactory3, IDWriteFactory6,
        IDWriteFontSet1, IDWriteLocalFontFileLoader, IDWriteLocalizedStrings,
    },
};
use windows_core::{HSTRING, Interface as _};

use crate::platform::windows::primitives::ui_languages::UiLanguages;

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
        // SAFETY: `DWriteCreateFactory` is unsafe because it crosses the FFI boundary.
        // We pass no raw pointers, only the valid constant `DWRITE_FACTORY_TYPE_SHARED`, and
        // the returned value is an owned COM interface managed by `windows`, so no borrowed
        // data can outlive the call.
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
    #[snafu(display("failed to cast font set for font file: {path}", path = path.display()))]
    CastFontSet {
        path: PathBuf,
        source: windows_core::Error,
    },
    #[snafu(display("failed to cast DirectWrite factory"))]
    CastFactory { source: windows_core::Error },
    #[snafu(display("failed to get system font collection"))]
    GetSystemFontCollection { source: windows_core::Error },
    #[snafu(display("system font collection is empty"))]
    GotSystemFontCollectionIsEmpty,
    #[snafu(display("failed to get font set from collection"))]
    GetFontSetFromCollection { source: windows_core::Error },
    #[snafu(display("failed to cast system font set"))]
    CastSystemFontSet { source: windows_core::Error },
}

#[derive(Debug)]
pub(crate) struct DirectWriteFontSet {
    font_set: IDWriteFontSet1,
}

impl DirectWriteFontSet {
    pub(crate) fn from_file<P>(
        factory: &DirectWriteFactory,
        path: P,
    ) -> Result<DirectWriteFontSet, DirectWriteFontSetError>
    where
        P: AsRef<Path>,
    {
        let path = path.as_ref();

        // SAFETY: `CreateFontSetBuilder` is an FFI call on a valid `IDWriteFactory6`
        // interface stored by `DirectWriteFactory`. The returned builder is an owned COM
        // interface managed by `windows`, so no borrowed data escapes this call.
        let font_set_builder =
            unsafe { factory.factory.CreateFontSetBuilder() }.context(CreateFontSetBuilderSnafu)?;

        // SAFETY: `AddFontFile` is unsafe because it reads a caller-provided UTF-16 path through
        // the FFI boundary. `HSTRING::from(path)` produces a NUL-terminated buffer, and the
        // temporary `HSTRING` is kept alive for the entire call expression, so the pointer
        // passed to DirectWrite remains valid for the duration of the call.
        unsafe { font_set_builder.AddFontFile(&HSTRING::from(path)) }
            .context(AddFontFileSnafu { path })?;

        // SAFETY: `CreateFontSet` is an FFI call on a valid builder interface. The returned font
        // set is an owned COM interface managed by `windows`, so it does not borrow from
        // `font_set_builder`.
        let font_set = unsafe { font_set_builder.CreateFontSet() }
            .context(CreateFontSetSnafu { path })?
            .cast()
            .context(CastFontSetSnafu { path })?;

        Ok(DirectWriteFontSet { font_set })
    }

    pub(crate) fn system_font_set(
        factory: &DirectWriteFactory,
    ) -> Result<DirectWriteFontSet, DirectWriteFontSetError> {
        let factory3 = factory
            .factory
            .cast::<IDWriteFactory3>()
            .context(CastFactorySnafu)?;
        let mut collection = None;

        // SAFETY: `GetSystemFontCollection` writes the returned collection to the provided
        // out-parameter. `factory3` is a valid DirectWrite factory interface, and `collection` is
        // a valid mutable local for the duration of the call. On success, any returned collection
        // is an owned COM interface managed by `windows`.
        unsafe { factory3.GetSystemFontCollection(false, &raw mut collection, true) }
            .context(GetSystemFontCollectionSnafu)?;
        let collection = collection.context(GotSystemFontCollectionIsEmptySnafu)?;

        // SAFETY: `GetFontSet` is an FFI call on a valid font collection interface. The returned
        // font set is an owned COM interface managed by `windows`, so no borrowed data escapes the
        // call.
        let font_set = unsafe { collection.GetFontSet() }.context(GetFontSetFromCollectionSnafu)?;

        let font_set = font_set.cast().context(CastSystemFontSetSnafu)?;

        Ok(DirectWriteFontSet { font_set })
    }

    pub(crate) fn entry_count(&self) -> u32 {
        // SAFETY: `GetFontCount` is an FFI call on a valid `IDWriteFontSet1` interface and takes
        // no raw pointers.
        unsafe { self.font_set.GetFontCount() }
    }

    pub(crate) fn entry_at(&self, index: u32) -> DirectWriteFontSetEntry<'_> {
        DirectWriteFontSetEntry {
            font_set: self,
            index,
        }
    }

    pub(crate) fn entries(&self) -> impl Iterator<Item = DirectWriteFontSetEntry<'_>> {
        let count = self.entry_count();
        DirectWriteFontSetEntries {
            font_set: self,
            index: Range::from(0..count).into_iter(),
        }
    }
}

#[derive(Debug)]
pub(crate) struct DirectWriteFontSetEntries<'a> {
    font_set: &'a DirectWriteFontSet,
    index: RangeIter<u32>,
}

impl<'a> Iterator for DirectWriteFontSetEntries<'a> {
    type Item = DirectWriteFontSetEntry<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        self.index.next().map(|index| self.font_set.entry_at(index))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.index.size_hint()
    }
}

impl DoubleEndedIterator for DirectWriteFontSetEntries<'_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        self.index
            .next_back()
            .map(|index| self.font_set.entry_at(index))
    }
}

impl ExactSizeIterator for DirectWriteFontSetEntries<'_> {}
impl FusedIterator for DirectWriteFontSetEntries<'_> {}

#[derive(Debug, Snafu)]
pub(crate) enum DirectWriteFontSetEntryError {
    #[snafu(display(
        "failed to get font property values for font set entry at index {index} and property ID {property_id}",
        property_id = property_id.0,
    ))]
    GetFontPropertyValues {
        index: u32,
        property_id: DWRITE_FONT_PROPERTY_ID,
        source: windows_core::Error,
    },
    #[snafu(display("font file is missing family name at font set entry at index {index}"))]
    MissingFontFamilyName { index: u32 },
    #[snafu(display("font file is missing face name at font set entry at index {index}"))]
    MissingFontFaceName { index: u32 },
    #[snafu(display("failed to get font face reference for font set entry at index {index}"))]
    GetFontFaceReference {
        index: u32,
        source: windows_core::Error,
    },
    #[snafu(display("failed to get font file for font set entry at index {index}"))]
    GetFontFile {
        index: u32,
        source: windows_core::Error,
    },
    #[snafu(display("failed to get font loader for font set entry at index {index}"))]
    GetFontFileLoader {
        index: u32,
        source: windows_core::Error,
    },
    #[snafu(display(
        "failed to cast font loader to local file loader for font set entry at index {index}"
    ))]
    CastFontFileLoader {
        index: u32,
        source: windows_core::Error,
    },
    #[snafu(display("failed to get font file reference key for font set entry at index {index}"))]
    GetFontFileReferenceKey {
        index: u32,
        source: windows_core::Error,
    },
    #[snafu(display("failed to get font file path length for font set entry at index {index}"))]
    GetFontFilePathLength {
        index: u32,
        source: windows_core::Error,
    },
    #[snafu(display(
        "failed to get font file path from reference key for font set entry at index {index}"
    ))]
    GetFontFilePathFromKey {
        index: u32,
        source: windows_core::Error,
    },
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

    pub(crate) fn path(&self) -> Result<Option<PathBuf>, DirectWriteFontSetEntryError> {
        // SAFETY: `GetFontFaceReference` is an FFI call on a valid `IDWriteFontSet1` interface.
        // The returned font face reference is an owned COM interface managed by `windows`.
        let face_ref = unsafe { self.font_set.font_set.GetFontFaceReference(self.index) }
            .context(GetFontFaceReferenceSnafu { index: self.index })?;
        // SAFETY: `GetFontFile` is an FFI call on a valid font face reference. The returned font
        // file is an owned COM interface managed by `windows`.
        let font_file =
            unsafe { face_ref.GetFontFile() }.context(GetFontFileSnafu { index: self.index })?;

        // SAFETY: `GetLoader` is an FFI call on a valid font file. The returned loader is an
        // owned COM interface managed by `windows`.
        let loader = unsafe { font_file.GetLoader() }
            .context(GetFontFileLoaderSnafu { index: self.index })?;
        let local_loader = match loader.cast::<IDWriteLocalFontFileLoader>() {
            Ok(local_loader) => local_loader,
            Err(e) if e.code() == E_NOINTERFACE => {
                // The loader is not a local file loader, so we cannot get a path.
                return Ok(None);
            }
            Err(e) => return Err(CastFontFileLoaderSnafu { index: self.index }.into_error(e)),
        };

        let mut path_buf;
        let path_len;
        // SAFETY: The key returned by `GetReferenceKey` is valid only while `font_file` is alive,
        // so obtaining the key and consuming it through `local_loader` must be treated as one
        // unsafe unit. `font_file` is kept alive for this entire block, and `local_loader` was
        // cast from the exact loader returned by `font_file.GetLoader()`, so the loader/key pair
        // matches.
        unsafe {
            let mut ref_key = ptr::null_mut();
            let mut ref_key_size = 0;
            // SAFETY: `GetReferenceKey` writes the key pointer and size to these out-parameters,
            // and both locals are valid for the duration of the call.
            font_file
                .GetReferenceKey(&raw mut ref_key, &raw mut ref_key_size)
                .context(GetFontFileReferenceKeySnafu { index: self.index })?;

            // SAFETY: `ref_key` and `ref_key_size` came from `GetReferenceKey` on `font_file`,
            // and are still valid because `font_file` is kept alive for this whole block.
            path_len = local_loader
                .GetFilePathLengthFromKey(ref_key, ref_key_size)
                .context(GetFontFilePathLengthSnafu { index: self.index })?;

            path_buf = vec![0; (path_len + 1) as usize];
            // SAFETY: `path_buf` has `path_len + 1` UTF-16 code units, where `path_len` came
            // from `GetFilePathLengthFromKey`, so there is room for the path and its terminating
            // NUL.
            local_loader
                .GetFilePathFromKey(ref_key, ref_key_size, &mut path_buf)
                .context(GetFontFilePathFromKeySnafu { index: self.index })?;
        };

        let path_str = OsString::from_wide(&path_buf[..path_len as usize]);
        Ok(Some(PathBuf::from(path_str)))
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

        // SAFETY: `GetPropertyValues3` writes to the provided out-parameters. `exists` and
        // `strings` are valid mutable locals for the duration of the call. On success, any
        // returned `IDWriteLocalizedStrings` is an owned COM interface managed by `windows`, not
        // a borrowed pointer into `font_set`.
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
        preferred_languages: &UiLanguages,
    ) -> Result<OsString, DirectWriteLocalizedStringsError> {
        for locale_name in preferred_languages.tags() {
            let mut index = 0;
            let mut exists = false.into();

            // SAFETY: `FindLocaleName` reads the provided locale name and writes to the `index`
            // and `exists` out-parameters. `HSTRING::from(locale_name)` stays alive for the full
            // call expression, and both out-parameters are valid mutable locals for the duration
            // of the call.
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
        // SAFETY: `GetStringLength` is an FFI call on a valid `IDWriteLocalizedStrings`
        // interface and takes no raw pointers.
        let len = unsafe { self.strings.GetStringLength(index) }
            .context(GetStringLengthSnafu { index })?;

        let mut buf = vec![0u16; (len + 1) as usize];
        // SAFETY: `GetString` writes UTF-16 data into `buf`. `buf` is a valid mutable slice, and
        // its length is `len + 1`, where `len` came from `GetStringLength(index)`, so there is
        // enough space for the string plus the terminating NUL required by the API.
        unsafe { self.strings.GetString(index, &mut buf) }.context(GetStringSnafu { index })?;

        Ok(OsString::from_wide(&buf[..len as usize]))
    }
}
