use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsString,
    fmt::{self, Display},
};

#[derive(Debug, Default)]
pub(crate) struct FontFamilyAccumulator {
    families: BTreeMap<OsString, FontFamilyInfo>,
}

impl FontFamilyAccumulator {
    fn entry(&mut self, family: OsString) -> &mut FontFamilyInfo {
        self.families
            .entry(family.clone())
            .or_insert_with(|| FontFamilyInfo {
                family,
                faces: BTreeSet::new(),
            })
    }

    pub(crate) fn add_face(&mut self, family: OsString, face: OsString) {
        self.entry(family).faces.insert(face);
    }

    pub(crate) fn append_family(&mut self, family: &FontFamilyInfo) {
        self.entry(family.family.clone())
            .faces
            .extend(family.faces.iter().cloned());
    }

    pub(crate) fn into_families(self) -> Vec<FontFamilyInfo> {
        self.families.into_values().collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FontFamilyInfo {
    pub(crate) family: OsString,
    pub(crate) faces: BTreeSet<OsString>,
}

impl Display for FontFamilyInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self { family, faces } = self;
        let faces = faces
            .iter()
            .map(|s| s.to_string_lossy())
            .collect::<Vec<_>>()
            .join(", ");
        write!(f, "{} ({faces})", family.display())
    }
}
