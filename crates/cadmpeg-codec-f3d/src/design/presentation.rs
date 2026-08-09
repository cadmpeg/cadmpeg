// SPDX-License-Identifier: Apache-2.0
//! Stable Design identities and library markers for body presentation records.

use crate::bytes::is_guid_prefix;

/// Width of the GUID prefix in a serialized visual token.
pub(crate) const GUID_LEN: usize = 36;
const POST_2015_SUFFIX: &str = "_Post2015";

/// Parsed identity of one serialized visual-appearance record.
///
/// Records in one visual family share the GUID prefix. Each appended
/// `_Post2015` marker advances to a distinct record in that family.
#[derive(Clone, Copy, Debug)]
pub(crate) struct VisualToken<'a> {
    guid: &'a str,
    post_2015_revisions: usize,
}

impl VisualToken<'_> {
    /// Whether two tokens identify the same visual-appearance record.
    pub(crate) fn matches(self, other: Self) -> bool {
        self.guid.eq_ignore_ascii_case(other.guid)
            && self.post_2015_revisions == other.post_2015_revisions
    }
}

/// Parse a visual token as a GUID followed by zero or more `_Post2015`
/// revision markers.
pub(crate) fn visual_token(value: &str) -> Option<VisualToken<'_>> {
    if !is_guid_prefix(value) {
        return None;
    }
    let mut suffix = &value[GUID_LEN..];
    let mut post_2015_revisions = 0;
    while let Some(rest) = suffix.strip_prefix(POST_2015_SUFFIX) {
        suffix = rest;
        post_2015_revisions += 1;
    }
    suffix.is_empty().then_some(VisualToken {
        guid: &value[..GUID_LEN],
        post_2015_revisions,
    })
}

/// Stable Design type of a body record that owns its presentation envelope.
pub(crate) const BODY_PRESENTATION_TYPE_GUID: &str = "D3937028-C20C-4E65-B010-94AD418A5C20";
pub(crate) const BODY_PRESENTATION_TYPE_VERSION: u32 = 19;

/// Stable Design type of the B-rep container referenced by a body
/// presentation.
pub(crate) const BREP_CONTAINER_TYPE_GUID: &str = "CD57BC48-50EC-47DC-975A-FB6DEA72F4DA";
pub(crate) const BREP_CONTAINER_TYPE_VERSION: u32 = 4;

/// Stable Design type of the scene entity referenced by a body presentation.
pub(crate) const BODY_SCENE_NODE_TYPE_GUID: &str = "702b9cd2-537c-429e-8cc4-22beeeb98c37";
pub(crate) const BODY_SCENE_NODE_TYPE_VERSION: u32 = 1;

/// Stable Design type of a browser node that carries body visibility.
pub(crate) const BROWSER_NODE_TYPE_GUID: &str = "D26351F0-5940-4D23-AA20-2C35475A6D9E";
pub(crate) const BROWSER_NODE_BASE_TYPE_GUID: &str = "CB844AB6-240D-4fc9-9C9F-3679DC896D6F";
pub(crate) const BROWSER_NODE_TYPE_VERSION: u32 = 2;

/// Physical-material library identifier in a body presentation envelope.
pub(crate) const PHYSICAL_MATERIAL_LIBRARY_ID: &str = "C1EEA57C-3F56-45FC-B8CB-A9EC46A9994C";
/// Appearance library identifier in a legacy body presentation envelope.
pub(crate) const APPEARANCE_LIBRARY_ID: &str = "BA5EE55E-9982-449B-9D66-9F036540E140";
/// Appearance-library identifier pair in a current body presentation envelope.
pub(crate) const MODERN_APPEARANCE_LIBRARY_IDS: [&str; 2] = [
    "08861000-1D69-CF2A-C082-CBD98E7E5D7F",
    "005E1000-55CE-AFB6-81A1-36E3EF077C5F",
];

/// Whether a token names a physical material rather than one of its aspects.
pub(crate) fn is_physical_material_token(value: &str) -> bool {
    value.starts_with("PrismMaterial") && !value.contains("_physmat_aspects")
}

#[cfg(test)]
mod tests {
    #[test]
    fn visual_token_retains_revision_depth_as_record_identity() {
        let base =
            super::visual_token("11111111-2222-3333-4444-555555555555").expect("base visual token");
        let revised = super::visual_token("11111111-2222-3333-4444-555555555555_Post2015_Post2015")
            .expect("revision-suffixed visual token");
        let revised_case_variant =
            super::visual_token("11111111-2222-3333-4444-555555555555_post2015_post2015");

        assert!(!base.matches(revised));
        assert_eq!(revised.post_2015_revisions, 2);
        assert!(revised_case_variant.is_none());
        assert!(super::visual_token("11111111-2222-3333-4444-555555555555_unrecognized").is_none());
    }
}
