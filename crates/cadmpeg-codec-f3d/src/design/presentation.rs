// SPDX-License-Identifier: Apache-2.0
//! Stable Design identities and library markers for body presentation records.

use crate::bytes::is_guid_prefix;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

/// Width of the GUID prefix in a serialized visual token.
pub(crate) const GUID_LEN: usize = 36;
const POST_2015_SUFFIX: &str = "_Post2015";

/// Select fallible decode admission or infallible reconstruction for token grammar.
pub(crate) trait VisualTokenAdmission {
    type Error;
    fn next_revision<'value>(&self, value: &'value str, prefix: &str)
        -> Result<Option<&'value str>, Self::Error>;
    fn finish<T>(&self, result: Result<T, Self::Error>) -> Result<T, CodecError>;
}

impl VisualTokenAdmission for DecodeContext<'_> {
    type Error = CodecError;
    fn next_revision<'value>(&self, value: &'value str, prefix: &str)
        -> Result<Option<&'value str>, CodecError> {
        self.charge_work(1, "f3d visual token revision step")?;
        self.strip_prefix(value, prefix, "f3d visual token revision prefix")
    }
    fn finish<T>(&self, result: Result<T, CodecError>) -> Result<T, CodecError> {
        result
    }
}

impl VisualTokenAdmission for cadmpeg_ir::index::StandardIndex {
    type Error = std::convert::Infallible;
    fn next_revision<'value>(&self, value: &'value str, prefix: &str)
        -> Result<Option<&'value str>, Self::Error> {
        Ok(value.strip_prefix(prefix))
    }
    fn finish<T>(&self, result: Result<T, Self::Error>) -> Result<T, CodecError> {
        match result {
            Ok(value) => Ok(value),
            Err(error) => match error {},
        }
    }
}

/// Parse a visual token as a GUID followed by zero or more `_Post2015`
/// revision markers.
pub(crate) fn visual_token<A: VisualTokenAdmission>(
    admission: &A,
    value: &str,
) -> Result<Option<usize>, A::Error> {
    if !is_guid_prefix(value) {
        return Ok(None);
    }
    let mut suffix = &value[GUID_LEN..];
    let mut post_2015_revisions = 0;
    while let Some(rest) = admission.next_revision(suffix, POST_2015_SUFFIX)? {
        suffix = rest;
        post_2015_revisions += 1;
    }
    Ok(suffix.is_empty().then_some(post_2015_revisions))
}

/// Stable Design type of a body record that owns its presentation envelope.
pub(crate) const BODY_PRESENTATION_TYPE_GUID: &str = "D3937028-C20C-4E65-B010-94AD418A5C20";
pub(crate) const BODY_PRESENTATION_BASE_TYPE_GUID: &str = "63D6AD42-466A-40B7-9CCD-EB21BC500EF6";
pub(crate) const BODY_PRESENTATION_TYPE_VERSION: u32 = 19;

/// Stable Design type of the B-rep container referenced by a body
/// presentation.
pub(crate) const BREP_CONTAINER_TYPE_GUID: &str = "CD57BC48-50EC-47DC-975A-FB6DEA72F4DA";
pub(crate) const BREP_CONTAINER_TYPE_VERSION: u32 = 4;

/// Stable Design type of the scene entity referenced by a body presentation.
pub(crate) const BODY_SCENE_NODE_TYPE_GUID: &str = "702b9cd2-537c-429e-8cc4-22beeeb98c37";
pub(crate) const BODY_SCENE_NODE_TYPE_VERSION: u32 = 1;

/// Stable Design type family containing the body-visibility browser-node
/// member form.
pub(crate) const BROWSER_NODE_TYPE_GUID: &str = "D26351F0-5940-4D23-AA20-2C35475A6D9E";
pub(crate) const BROWSER_NODE_BASE_TYPE_GUID: &str = "CB844AB6-240D-4fc9-9C9F-3679DC896D6F";
pub(crate) const BROWSER_NODE_TYPE_VERSION: u32 = 2;

/// Marker that opens a bare-owner body presentation envelope.
pub(super) const BODY_PRESENTATION_MATERIAL_ENVELOPE_ID: &str =
    "D87FBE62-3B12-4CA8-9014-BAD31ABDB101";
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
pub(super) fn is_physical_material_token(
    ctx: &DecodeContext<'_>,
    value: &str,
) -> Result<bool, CodecError> {
    Ok(value.starts_with("PrismMaterial")
        && !ctx.contains_text(value, "_physmat_aspects", "f3d physical material token aspects")?)
}

#[cfg(test)]
mod tests {
    #[test]
    fn visual_token_retains_revision_depth_as_record_identity() {
        crate::test_support::with_decode_context(|ctx| {
            let base = super::visual_token(ctx, "11111111-2222-3333-4444-555555555555")
                .unwrap().expect("base visual token");
            let revised = super::visual_token(ctx,
                "11111111-2222-3333-4444-555555555555_Post2015_Post2015")
                .unwrap().expect("revision-suffixed visual token");
            let revised_case_variant = super::visual_token(ctx,
                "11111111-2222-3333-4444-555555555555_post2015_post2015").unwrap();
            assert_ne!(base, revised);
            assert_eq!(revised, 2);
            assert!(revised_case_variant.is_none());
            assert!(super::visual_token(ctx,
                "11111111-2222-3333-4444-555555555555_unrecognized").unwrap().is_none());
        });
    }

    #[test]
    fn presentation_token_work_refusals_propagate() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;
        for (limit, operation) in [(0, "f3d visual token revision step"), (1, "f3d visual token revision prefix")] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = limit;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let result = super::visual_token(&ctx, "11111111-2222-3333-4444-555555555555_Post2015");
            assert!(matches!(result, Err(CodecError::ResourceLimit(failure))
                if failure.dimension == ResourceDimension::WorkUnits && failure.operation == operation));
        }
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(matches!(crate::records::references::DesignVisualToken::new(&ctx,
            "11111111-2222-3333-4444-555555555555_Post2015".to_owned()),
            Err(CodecError::ResourceLimit(failure)) if failure.dimension == ResourceDimension::WorkUnits
                && failure.operation == "f3d visual token revision step"));
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(matches!(super::is_physical_material_token(&ctx, "PrismMaterial_sample"),
            Err(CodecError::ResourceLimit(failure)) if failure.dimension == ResourceDimension::WorkUnits
                && failure.operation == "f3d physical material token aspects"));
    }
}
