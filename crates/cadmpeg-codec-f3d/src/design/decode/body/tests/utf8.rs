// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::ResourceDimension;

#[test]
fn body_recipe_utf8_validation_refusals_propagate() {
    let design_id = b"123";
    let ctx = cadmpeg_test_support::service_decode_context();
    assert_eq!(
        super::super::recipe_design_id(&ctx, design_id, 23, b"").unwrap(),
        Some(("123", 0))
    );
    let design_id_error = crate::test_support::resource_refusal_at(
        ResourceDimension::WorkUnits,
        "validate F3D body recipe Design ID",
        0,
        |ctx| super::super::recipe_design_id(ctx, design_id, 23, b"").map(|_| ()),
    );
    assert!(matches!(
        design_id_error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::WorkUnits
                && limit.operation == "validate F3D body recipe Design ID"
                && limit.additional == 3
    ));

    let mut recipe_id = Vec::new();
    recipe_id.extend_from_slice(&3u32.to_le_bytes());
    recipe_id.extend_from_slice(b"abc");
    assert_eq!(
        super::super::ascii_id_at(&ctx, &recipe_id, 0).unwrap(),
        Some(("abc", 4))
    );
    let recipe_id_error = crate::test_support::resource_refusal_at(
        ResourceDimension::WorkUnits,
        "validate F3D body recipe ID",
        0,
        |ctx| super::super::ascii_id_at(ctx, &recipe_id, 0).map(|_| ()),
    );
    assert!(matches!(
        recipe_id_error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::WorkUnits
                && limit.operation == "validate F3D body recipe ID"
                && limit.additional == 3
    ));
}
