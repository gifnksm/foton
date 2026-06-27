use std::{ffi::OsString, iter, os::windows::ffi::OsStringExt as _};

use snafu::{ResultExt as _, Snafu};
use windows::Win32::Globalization::{self, MUI_LANGUAGE_NAME};
use windows_core::PWSTR;

#[derive(Debug, Snafu)]
pub(crate) enum PreferredUiLanguagesError {
    #[snafu(display("failed to get user preferred UI languages"))]
    GetUserPreferredUILanguages { source: windows_core::Error },
    #[snafu(display(
        "failed to convert preferred UI language to string: {invalid_string}",
        invalid_string = invalid_string.display()
    ))]
    InvalidString { invalid_string: OsString },
}

#[derive(Debug)]
pub(crate) struct PreferredUiLanguages {
    languages: Vec<String>,
}

impl PreferredUiLanguages {
    pub(crate) fn get() -> Result<Self, PreferredUiLanguagesError> {
        let mut count = 0;
        let mut buffer_size = 0;

        // SAFETY: This is an unsafe FFI call. We pass null pointers to get the required buffer size and count of languages.
        unsafe {
            Globalization::GetUserPreferredUILanguages(
                MUI_LANGUAGE_NAME,
                &raw mut count,
                None,
                &raw mut buffer_size,
            )
        }
        .context(GetUserPreferredUILanguagesSnafu)?;

        if buffer_size == 0 {
            return Ok(Self { languages: vec![] });
        }

        let mut buffer = vec![0u16; buffer_size as usize];
        // SAFETY: This is an unsafe FFI call. We pass a valid pointer to the buffer and its size to retrieve the preferred UI languages.
        unsafe {
            Globalization::GetUserPreferredUILanguages(
                MUI_LANGUAGE_NAME,
                &raw mut count,
                Some(PWSTR::from_raw(buffer.as_mut_ptr())),
                &raw mut buffer_size,
            )
        }
        .context(GetUserPreferredUILanguagesSnafu)?;

        let languages = buffer
            .split(|&c| c == 0)
            .take_while(|s| !s.is_empty())
            .take(count as usize)
            .map(|s| {
                OsString::from_wide(s)
                    .into_string()
                    .map_err(|s| InvalidStringSnafu { invalid_string: s }.build())
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Self { languages })
    }

    pub(crate) fn tags(&self) -> impl Iterator<Item = &str> {
        let has_en_us = self.languages.iter().any(|s| s == "en-US");

        let tags = self.languages.iter().flat_map(|s| {
            iter::once(s.as_str()).chain(
                s.rmatch_indices('-')
                    .filter_map(|(pos, _)| (pos > 0).then_some(&s[..pos])),
            )
        });
        let fallback_tags = (!has_en_us)
            .then_some(["en-US", "en"])
            .into_iter()
            .flatten();

        tags.chain(fallback_tags)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_languages(tags: &[&str]) -> PreferredUiLanguages {
        PreferredUiLanguages {
            languages: tags.iter().map(|s| (*s).to_owned()).collect(),
        }
    }

    #[test]
    fn tags_returns_expected_locale_candidates() {
        let cases = [
            ("empty", vec![], vec!["en-US", "en"]),
            ("exact en-US", vec!["en-US"], vec!["en-US", "en"]),
            (
                "single locale with parent fallback",
                vec!["ja-JP"],
                vec!["ja-JP", "ja", "en-US", "en"],
            ),
            (
                "multi-segment locale with all parents",
                vec!["zh-Hant-HK"],
                vec!["zh-Hant-HK", "zh-Hant", "zh", "en-US", "en"],
            ),
            (
                "multiple locales preserve input order",
                vec!["en-GB", "fr-CA"],
                vec!["en-GB", "en", "fr-CA", "fr", "en-US", "en"],
            ),
        ];

        for (name, input, expected) in cases {
            let languages = make_languages(&input);
            let tags = languages.tags().collect::<Vec<_>>();
            assert_eq!(tags, expected, "case: {name}");
        }
    }
}
