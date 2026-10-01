// SPDX-License-Identifier: Apache-2.0
//! Resource propagation through analytic patch lookup and mutation.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn curve_attribute_lookup_preserves_carrier_scan_refusal() {
    let body = crate::test_support::parasolid::line_carrier(10, [0.0; 3], [1.0, 0.0, 0.0]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&body, &arena, &policy).unwrap();
    let error = super::curve_by_attr(&ctx, &body, 10).unwrap_err();
    let CodecError::ResourceLimit(limit) = error else { panic!("carrier scan refusal"); };
    assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
    assert_eq!(ctx.resource_refusal(), Some(limit));
}

#[test]
fn compact_attribute_patch_preserves_carrier_scan_refusal() {
    let mut body = crate::test_support::parasolid::line_carrier(10, [0.0; 3], [1.0, 0.0, 0.0]);
    let original = body.clone();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&original, &arena, &policy).unwrap();
    let error = super::patch_compact_values(&ctx, &mut body, 10, &[0.0; 6]).unwrap_err();
    let CodecError::ResourceLimit(limit) = error else { panic!("carrier scan refusal"); };
    assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert_eq!(body, original);
}

#[test]
fn attribute_nurbs_patch_preserves_carrier_scan_refusal() {
    let mut body = crate::test_support::parasolid::nurbs_curve_carrier(10, 11);
    let original = body.clone();
    let baseline = cadmpeg_test_support::service_decode_context();
    let carrier = super::spline::scan_curve_carriers(&baseline, &body, &mut Vec::new()).unwrap().remove(&10).unwrap();
    let Some(cadmpeg_ir::geometry::SolvedCurveGeometry::Nurbs(curve)) = carrier.geometry.solved() else { panic!("NURBS curve"); };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&original, &arena, &policy).unwrap();
    let error = super::patch_nurbs_by_attr(&ctx, &mut body, 10, curve).unwrap_err();
    let CodecError::ResourceLimit(limit) = error else { panic!("carrier scan refusal"); };
    assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert_eq!(body, original);
}
