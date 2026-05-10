use std::{str::FromStr, sync::LazyLock};

use regex::Regex;
use snafu::Snafu;

#[derive(Debug, Clone)]
pub(crate) struct NormalizedString {
    separated: String,
    compact: String,
}

impl NormalizedString {
    pub(crate) fn new<S>(s: S) -> Self
    where
        S: AsRef<str>,
    {
        static RE: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(
                r"(?x)
                [\W_-]+
                | (\p{Lowercase}) (\p{Uppercase})
                | (\p{Alphabetic}) (\p{Number})
                | (\p{Number}) (\p{Alphabetic})
            ",
            )
            .unwrap()
        });

        let s = s.as_ref();

        let mut haystack = s;
        let mut separated = String::with_capacity(s.len());
        while let Some(cap) = RE.captures(haystack) {
            let pre = cap.get(1).or_else(|| cap.get(3)).or_else(|| cap.get(5));
            let post = cap.get(2).or_else(|| cap.get(4)).or_else(|| cap.get(6));
            let sep_start = if let Some(pre) = pre {
                pre.end()
            } else {
                cap.get_match().start()
            };
            let sep_end = if let Some(post) = post {
                post.start()
            } else {
                cap.get_match().end()
            };
            separated.push_str(&haystack[..sep_start]);
            separated.push(' ');
            haystack = &haystack[sep_end..];
        }
        separated.push_str(haystack);

        let separated = separated.trim().to_lowercase();
        let compact = separated.replace(' ', "");
        Self { separated, compact }
    }
}

impl From<String> for NormalizedString {
    fn from(s: String) -> Self {
        Self::new(s)
    }
}

impl From<&str> for NormalizedString {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

#[derive(Debug, Snafu)]
pub(crate) enum QueryStringError {
    #[snafu(display("normalized query string is empty"))]
    Empty,
}

#[derive(Debug, Clone)]
pub(crate) struct QueryString(NormalizedString);

impl QueryString {
    pub(crate) fn try_new<Q>(query: Q) -> Result<Self, QueryStringError>
    where
        Q: AsRef<str>,
    {
        let query = NormalizedString::new(query);
        if query.compact.is_empty() {
            return Err(EmptySnafu.build());
        }
        Ok(Self(query))
    }
}

impl FromStr for QueryString {
    type Err = QueryStringError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::try_new(s)
    }
}

impl TryFrom<String> for QueryString {
    type Error = QueryStringError;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        Self::try_new(s)
    }
}

impl TryFrom<&str> for QueryString {
    type Error = QueryStringError;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        Self::try_new(s)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum MatchKind {
    Substring,
    Prefix,
    Exact,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum MatchForm {
    Compact,
    Separated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct MatchResult {
    pub(crate) form: MatchForm,
    pub(crate) kind: MatchKind,
}

#[derive(Debug, Clone)]
pub(crate) struct TextMatcher {
    queries: Vec<QueryString>,
}

impl TextMatcher {
    pub(crate) fn new(queries: Vec<QueryString>) -> Self {
        Self { queries }
    }

    pub(crate) fn match_text<N>(&self, s: N) -> Option<MatchResult>
    where
        N: Into<NormalizedString>,
    {
        let s = s.into();

        let form = MatchForm::Separated;
        if let Some(kind) = self
            .queries
            .iter()
            .map(|q| match_text(&s.separated, &q.0.separated))
            .min()
            .flatten()
        {
            return Some(MatchResult { form, kind });
        }

        let form = MatchForm::Compact;
        if let Some(kind) = self
            .queries
            .iter()
            .map(|q| match_text(&s.compact, &q.0.compact))
            .min()
            .flatten()
        {
            return Some(MatchResult { form, kind });
        }

        None
    }
}

fn match_text(text: &str, query: &str) -> Option<MatchKind> {
    if text == query {
        return Some(MatchKind::Exact);
    }
    if text.starts_with(query) {
        return Some(MatchKind::Prefix);
    }
    if text.contains(query) {
        return Some(MatchKind::Substring);
    }
    None
}

#[cfg(test)]
mod tests {
    use crate::util::text::QueryString;

    use super::{MatchForm, MatchKind, MatchResult, NormalizedString, TextMatcher};

    #[test]
    fn normalized_string_normalizes_expected_forms() {
        let cases = [
            ("HackGen", "hack gen", "hackgen"),
            ("hack-gen", "hack gen", "hackgen"),
            ("hack_gen", "hack gen", "hackgen"),
            ("  Hack   Gen  ", "hack gen", "hackgen"),
            ("HackGen12", "hack gen 12", "hackgen12"),
            ("Font12UI", "font 12 ui", "font12ui"),
            ("font_12-ui", "font 12 ui", "font12ui"),
            ("aB1", "a b 1", "ab1"),
            ("UDPGothic", "udpgothic", "udpgothic"),
        ];

        for (input, expected_separated, expected_compact) in cases {
            let normalized = NormalizedString::new(input);
            assert_eq!(normalized.separated, expected_separated, "input: {input}");
            assert_eq!(normalized.compact, expected_compact, "input: {input}");
        }
    }

    #[test]
    fn text_matcher_classifies_expected_matches() {
        let cases = [
            (
                &["hack gen"][..],
                "hack-gen",
                Some(MatchResult {
                    form: MatchForm::Separated,
                    kind: MatchKind::Exact,
                }),
            ),
            (
                &["hack gen"],
                "hackgen",
                Some(MatchResult {
                    form: MatchForm::Compact,
                    kind: MatchKind::Exact,
                }),
            ),
            (
                &["hack gen"],
                "hack-gen nerd",
                Some(MatchResult {
                    form: MatchForm::Separated,
                    kind: MatchKind::Prefix,
                }),
            ),
            (
                &["hack gen"],
                "hackgennerd",
                Some(MatchResult {
                    form: MatchForm::Compact,
                    kind: MatchKind::Prefix,
                }),
            ),
            (
                &["hack gen"],
                "best_hack_gen_nerd",
                Some(MatchResult {
                    form: MatchForm::Separated,
                    kind: MatchKind::Substring,
                }),
            ),
            (
                &["hack gen"],
                "besthackgennerd",
                Some(MatchResult {
                    form: MatchForm::Compact,
                    kind: MatchKind::Substring,
                }),
            ),
            (
                &["udp gothic"],
                "UDPGothic",
                Some(MatchResult {
                    form: MatchForm::Compact,
                    kind: MatchKind::Exact,
                }),
            ),
            (&["hack gen"], "maple mono", None),
            (
                &["hack gen", "hack gen"],
                "hack gen",
                Some(MatchResult {
                    form: MatchForm::Separated,
                    kind: MatchKind::Exact,
                }),
            ),
            (
                &["hack", "gen"],
                "hack gen",
                Some(MatchResult {
                    form: MatchForm::Separated,
                    kind: MatchKind::Substring,
                }),
            ),
            (
                &["hackg", "ckgen"],
                "hack gen",
                Some(MatchResult {
                    form: MatchForm::Compact,
                    kind: MatchKind::Substring,
                }),
            ),
            (&["hack", "gothic"], "hack gen", None),
        ];

        for (query_strs, text, expected) in cases {
            let queries = query_strs
                .iter()
                .map(|q| QueryString::try_new(q).unwrap())
                .collect::<Vec<_>>();
            let matcher = TextMatcher::new(queries);
            assert_eq!(
                matcher.match_text(text),
                expected,
                "queries: {query_strs:?}, text: {text:?}"
            );
        }
    }

    #[test]
    fn match_result_orders_expected_priority() {
        let cases = [
            MatchResult {
                form: MatchForm::Compact,
                kind: MatchKind::Substring,
            },
            MatchResult {
                form: MatchForm::Compact,
                kind: MatchKind::Prefix,
            },
            MatchResult {
                form: MatchForm::Compact,
                kind: MatchKind::Exact,
            },
            MatchResult {
                form: MatchForm::Separated,
                kind: MatchKind::Substring,
            },
            MatchResult {
                form: MatchForm::Separated,
                kind: MatchKind::Prefix,
            },
            MatchResult {
                form: MatchForm::Separated,
                kind: MatchKind::Exact,
            },
        ];
        assert!(cases.is_sorted());
    }
}
