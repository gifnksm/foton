use std::{
    fmt::{self, Display},
    path::Path,
};

use crate::util::glob::PathGlob;

#[derive(Debug, Clone, PartialEq, Eq, Hash, derive_more::IsVariant)]
pub(crate) enum PathMatcher {
    Path(LiteralPathMatcher),
    Glob(PathGlob),
}

impl PathMatcher {
    pub(crate) fn path<S>(path: S) -> Result<Self, PathMatcherError>
    where
        S: Into<String>,
    {
        let original = path.into();
        if original.is_empty() {
            return Err(PathMatcherError::EmptyPath);
        }
        Ok(Self::Path(LiteralPathMatcher::new(original)))
    }

    pub(crate) fn glob(glob: PathGlob) -> Self {
        Self::Glob(glob)
    }

    pub(crate) fn matches<P>(&self, path: P) -> bool
    where
        P: AsRef<Path>,
    {
        match self {
            Self::Path(path_matcher) => path_matcher.matches(path),
            Self::Glob(glob) => glob.matches(path),
        }
    }

    pub(crate) fn as_glob(&self) -> Option<&PathGlob> {
        match self {
            Self::Glob(glob) => Some(glob),
            Self::Path(_) => None,
        }
    }
}

impl Display for PathMatcher {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Path(path) => Display::fmt(path, f),
            Self::Glob(glob) => Display::fmt(glob, f),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct LiteralPathMatcher {
    original: String,
    glob: PathGlob,
}

impl LiteralPathMatcher {
    fn new(original: String) -> Self {
        Self {
            glob: PathGlob::escape(&original),
            original,
        }
    }

    fn matches<P>(&self, path: P) -> bool
    where
        P: AsRef<Path>,
    {
        self.glob.matches(path)
    }

    pub(crate) fn into_original(self) -> String {
        self.original
    }
}

impl Display for LiteralPathMatcher {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Display::fmt(&self.original, f)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, derive_more::Display)]
pub(crate) enum PathMatcherError {
    #[display("path must be non-empty")]
    EmptyPath,
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn path_matcher_matches_paths_case_insensitively() {
        let matcher = PathMatcher::path("fonts/A.ttf").unwrap();

        assert!(matcher.matches(Path::new("fonts/a.ttf")));
        assert!(!matcher.matches(Path::new("fonts/b.ttf")));
    }

    #[test]
    fn path_matcher_matches_literal_glob_metacharacters() {
        let matcher = PathMatcher::path("fonts/[wght].ttf").unwrap();

        assert!(matcher.matches(Path::new("fonts/[WGHT].ttf")));
        assert!(!matcher.matches(Path::new("fonts/awght.ttf")));
    }

    #[cfg(windows)]
    #[test]
    fn path_matcher_follows_windows_separator_matching() {
        let matcher = PathMatcher::path(r"fonts\A.ttf").unwrap();

        assert!(matcher.matches(Path::new("fonts/a.ttf")));
    }

    #[test]
    fn path_matcher_as_glob_returns_only_explicit_globs() {
        let path_matcher = PathMatcher::path("fonts/A.ttf").unwrap();
        assert_eq!(path_matcher.as_glob(), None);

        let glob_matcher = PathMatcher::glob(PathGlob::new("**/*.ttf").unwrap());
        assert_eq!(glob_matcher.as_glob().unwrap().as_str(), "**/*.ttf");
    }
}
