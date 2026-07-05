use url::Url;

use crate::{
    package::{PackageId, PackageSourceContents},
    util::{
        hash::GenericDigest,
        text::{MatchForm, MatchKind, TextMatcher},
    },
};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct PackageDefinition {
    pub(crate) id: PackageId,
    pub(crate) description: Option<String>,
    pub(crate) homepage: Option<String>,
    pub(crate) repository: Option<String>,
    pub(crate) license: Option<String>,
    pub(crate) sources: Vec<PackageSource>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct PackageSource {
    pub(crate) url: Url,
    pub(crate) hash: GenericDigest,
    pub(crate) contents: PackageSourceContents,
}

impl PackageDefinition {
    pub(crate) fn match_package(&self, matcher: &TextMatcher) -> Option<PackageMatchResult> {
        let mut max_res = None;

        if let Some(res) = matcher.match_text(self.id.name().as_str()) {
            let res = Some(PackageMatchResult {
                form: res.form,
                kind: res.kind,
                field: PackageDefinitionField::Name,
            });
            max_res = Option::max(max_res, res);
        }

        if let Some(res) = self
            .description
            .as_deref()
            .and_then(|description| matcher.match_text(description))
        {
            let res = Some(PackageMatchResult {
                form: res.form,
                kind: res.kind,
                field: PackageDefinitionField::Description,
            });
            max_res = Option::max(max_res, res);
        }

        max_res
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum PackageDefinitionField {
    Description,
    Name,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct PackageMatchResult {
    pub(crate) form: MatchForm,
    pub(crate) kind: MatchKind,
    pub(crate) field: PackageDefinitionField,
}

#[cfg(test)]
mod tests {
    use crate::util::{testing, text::QueryString};

    use super::*;

    fn make_matcher(queries: &[&str]) -> TextMatcher {
        TextMatcher::new(
            queries
                .iter()
                .map(|query| QueryString::try_new(query).unwrap())
                .collect(),
        )
    }

    #[test]
    fn package_metadata_match_package_prefers_stronger_match_from_another_field() {
        let pkg = testing::parse_manifest_to_definition(indoc::indoc! {r#"
            name = "example-font-nerd"
            description = "Example Font"
            version = "0.1.0"

            [[sources]]
            url = "https://example.com/example-font-0.1.0.zip"
            hash = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"

            [sources.contents]
            type = "archive"
        "#});
        let text_matcher = make_matcher(&["example font"]);

        let result = pkg.match_package(&text_matcher).unwrap();

        assert_eq!(
            result,
            PackageMatchResult {
                form: MatchForm::Separated,
                kind: MatchKind::Exact,
                field: PackageDefinitionField::Description,
            }
        );
    }

    #[test]
    fn package_metadata_match_package_requires_all_queries_in_the_same_field() {
        let pkg = testing::parse_manifest_to_definition(indoc::indoc! {r#"
            name = "example-font"
            description = "Nerd variant"
            version = "0.1.0"

            [[sources]]
            url = "https://example.com/example-font-0.1.0.zip"
            hash = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"

            [sources.contents]
            type = "archive"
        "#});
        let text_matcher = make_matcher(&["example", "nerd"]);

        assert_eq!(pkg.match_package(&text_matcher), None);
    }

    #[test]
    fn package_metadata_match_package_uses_weakest_match_kind_for_multiple_queries() {
        let pkg = testing::parse_manifest_to_definition(indoc::indoc! {r#"
            name = "typeface"
            version = "0.1.0"

            [[sources]]
            url = "https://example.com/example-font-0.1.0.zip"
            hash = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"

            [sources.contents]
            type = "archive"
        "#});
        let text_matcher = make_matcher(&["typeface", "face"]);

        let result = pkg.match_package(&text_matcher).unwrap();

        assert_eq!(
            result,
            PackageMatchResult {
                form: MatchForm::Separated,
                kind: MatchKind::Substring,
                field: PackageDefinitionField::Name,
            }
        );
    }
}
