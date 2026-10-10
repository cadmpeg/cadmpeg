// SPDX-License-Identifier: Apache-2.0

use crate::curves::GeometryError;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn extrusion_knot_slice_equality_preserves_refusal() {
    let (start, end) = super::simple_extrusion_curves();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
    let error = super::super::extrusion_nurbs(
        &ctx,
        &start,
        &end,
        cadmpeg_ir::units::FiniteVector::new([0.0, 1.0]).unwrap(),
        false,
        0,
    )
    .unwrap_err();
    let GeometryError::Codec(CodecError::ResourceLimit(refusal)) = error else {
        panic!("knot comparison must preserve its resource refusal");
    };
    assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
    assert_eq!(refusal.operation, "Rhino extrusion knot equality");
    assert_eq!(ctx.resource_refusal(), Some(refusal));
}

fn incompatible_knots(knots: Vec<f64>, max_work: u64) {
    use cadmpeg_ir::geometry::nurbs::NurbsCurve;
    use cadmpeg_ir::math::Point3;
    let (start, _) = super::simple_extrusion_curves();
    let points = (0..knots.len() - 2)
        .map(|_| Point3::new(0.0, 0.0, 0.0))
        .collect();
    let end = NurbsCurve::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        1,
        knots,
        points,
        None,
        false,
    )
    .expect("fixture admission")
    .expect("valid degree-one curve");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = max_work;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
    let error = super::super::extrusion_nurbs(
        &ctx,
        &start,
        &end,
        cadmpeg_ir::units::FiniteVector::new([0.0, 1.0]).unwrap(),
        false,
        0,
    )
    .expect_err("incompatible profile knots");
    assert!(matches!(error,
        GeometryError::Malformed(crate::chunks::FramingError::Structural { ref message, .. })
        if message == "extrusion tensor inputs are incompatible"));
    ctx.finish_session()
        .expect("unvisited knots consume no work");
}

#[test]
fn extrusion_unequal_knot_lengths_need_no_scan() {
    incompatible_knots(vec![0.0, 0.0, 0.5, 1.0, 1.0], 0);
}

#[test]
fn extrusion_first_knot_mismatch_visits_only_one_pair() {
    incompatible_knots(vec![2.0, 2.0, 3.0, 3.0], 1);
}
