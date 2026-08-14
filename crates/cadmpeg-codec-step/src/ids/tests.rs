// SPDX-License-Identifier: Apache-2.0
use super::StepIdentity;
use cadmpeg_ir::{format_identity, is_valid_identity, IdentityError};

#[test]
fn signature_uses_three_component_grammar() {
    let id = StepIdentity::signature(0);
    assert_eq!(id.0, "step:file:signature#0");
    assert!(is_valid_identity(&id.0));
}

#[test]
fn data_and_opaque_preserve_existing_forms() {
    assert_eq!(StepIdentity::data("surface", 12u64), "step:data:surface#12");
    assert_eq!(StepIdentity::opaque(2u64), "step:data:opaque#2");
    assert_eq!(StepIdentity::data("", 1u64), "step:data:opaque#1");
    assert!(is_valid_identity(&StepIdentity::data("edge", "3-shell-4")));
}

#[test]
fn scoped_builders_preserve_existing_forms() {
    assert_eq!(
        StepIdentity::product("occurrence", "definition-9"),
        "step:product:occurrence#definition-9"
    );
    assert_eq!(
        StepIdentity::presentation("pmi", 4u64),
        "step:presentation:pmi#4"
    );
    assert_eq!(
        StepIdentity::construction("trimmed_curve", 9u64),
        "step:construction:trimmed_curve#9"
    );
    assert_eq!(
        StepIdentity::tessellation("mesh", 1u64),
        "step:tessellation:mesh#1"
    );
    assert_eq!(
        StepIdentity::drawing("drawing_definition", 2u64),
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
