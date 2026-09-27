// SPDX-License-Identifier: Apache-2.0

use crate::scalar;
use crate::surface::SurfacePrototypeFamily;

fn named_surface_limit_error(
    name: &str,
    body: &[u8],
    collection_limit: u64,
    retained_limit: u64,
) -> cadmpeg_core::CodecError {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = collection_limit;
    policy.limits.max_retained_bytes = retained_limit;
    let (ctx, _) = DecodeContext::from_root_bytes(body, &arena, &policy)
        .expect("named surface input fits the root limit");
    crate::surface::named_surface_value(
        &ctx,
        &SurfacePrototypeFamily::Spline(crate::surface::SplineLabel::Spline),
        name,
        body,
        &scalar::ScalarCache::default(),
        &"prototype fixture",
        &mut crate::lane_refusal::LaneRefusals::new(),
    )
    .expect_err("the selected surface allocation exceeds its limit")
}

#[test]
fn opaque_surface_parameter_refuses_before_byte_copy() {
    let error = named_surface_limit_error("flip", &[0xf1], u64::MAX, 0);
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "creo opaque surface parameter bytes"
    ));
}

#[test]
fn zero_surface_scalar_refuses_before_vector_allocation() {
    let error = named_surface_limit_error("radius", &[0x18], 0, u64::MAX);
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "creo zero surface scalar"
    ));
}

#[test]
fn compact_surface_integers_refuse_before_vector_growth() {
    let error = named_surface_limit_error("dum_array", &[0xf8, 0x02, 0x07, 0x08], 0, u64::MAX);
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "creo compact surface integers"
    ));
}

#[test]
fn contiguous_surface_references_refuse_before_vector_allocation() {
    let error =
        named_surface_limit_error("i_pnts", &[0xf8, 0x03, 0xf7, 0x80, 0x80, 0xfb], 2, u64::MAX);
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "creo contiguous surface references"
    ));
}
