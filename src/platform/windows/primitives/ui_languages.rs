use std::{ffi::OsString, iter, os::windows::ffi::OsStringExt as _};

use snafu::{ResultExt as _, Snafu};
use windows::Win32::Globalization::{self, MUI_LANGUAGE_NAME};
use windows_core::PWSTR;

#[derive(Debug, Snafu)]
pub(crate) enum UiLanguagesError {
    #[snafu(display("failed to get user preferred UI languages"))]
    GetUserPreferredUILanguages { source: windows_core::Error },
    #[snafu(display(
        "failed to convert preferred UI language to string: {invalid_string}",
        invalid_string = invalid_string.display()
    ))]
    InvalidString { invalid_string: OsString },
}

#[derive(Debug)]
pub(crate) struct UiLanguages {
    languages: Vec<String>,
}

impl UiLanguages {
    pub(crate) fn get_preferred() -> Result<Self, UiLanguagesError> {
        let mut count = 0;
        let mut buffer_size = 0;

        // SAFETY: `GetUserPreferredUILanguages` writes to the `count` and `buffer_size`
        // out-parameters. Both are valid mutable locals for the duration of the call, and passing
        // `None` for the buffer is the documented probe mode used to obtain the required buffer
        // size.
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
        // SAFETY: `GetUserPreferredUILanguages` writes UTF-16 data into `buffer` and updates the
        // `count` and `buffer_size` out-parameters. `buffer` is allocated with exactly the size
        // reported by the preceding probe call, `buffer.as_mut_ptr()` is valid for that many
        // code units for the duration of the call, and the API writes at most `buffer_size`
        // characters including the trailing double NUL.
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

    fn make_languages(tags: &[&str]) -> UiLanguages {
        UiLanguages {
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
