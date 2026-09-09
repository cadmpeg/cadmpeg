// SPDX-License-Identifier: Apache-2.0
use crate::ids;
use cadmpeg_ir::{format_identity, is_valid_identity, IdentityError};

#[test]
fn signature_uses_three_component_grammar() {
    let id = ids::signature(0);
    assert_eq!(id.as_str(), "step:file:signature#0");
    assert!(is_valid_identity(id.as_str()));
}

#[test]
fn data_and_opaque_preserve_existing_forms() {
    assert_eq!(ids::data("surface", 12u64).as_str(), "step:data:surface#12");
    assert!(is_valid_identity(ids::data("edge", "3-shell-4").as_str()));
}

#[test]
fn scoped_builders_preserve_existing_forms() {
    assert_eq!(
        ids::product("occurrence", "definition-9").as_str(),
        "step:product:occurrence#definition-9"
    );
    assert_eq!(
        ids::presentation("pmi", 4u64).as_str(),
        "step:presentation:pmi#4"
    );
    assert_eq!(
        ids::construction("trimmed_curve", 9u64).as_str(),
        "step:construction:trimmed_curve#9"
    );
    assert_eq!(
        ids::tessellation("mesh", 1u64).as_str(),
        "step:tessellation:mesh#1"
    );
    assert_eq!(
        ids::drawing("drawing_definition", 2u64).as_str(),
        "step:drawing:drawing_definition#2"
    );
}

#[test]
fn empty_scope_is_rejected_at_construction() {
    // The Phase 1 regression was `step:signature#0` (missing scope).
    let err = format_identity("step", "", "signature", 0u8).expect_err("empty scope");
    assert!(matches!(
        err,
        IdentityError::InvalidComponent { label: "scope", .. }
    ));
    assert!(!is_valid_identity("step:signature#0"));
}
