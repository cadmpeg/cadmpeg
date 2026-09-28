// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_core::CodecError;

#[test]
fn sketch_nurbs_point_refuses_pole_copy_limit() {
    let curve = PcurveNurbs::from_lanes(
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)],
        None,
        false,
    )
    .unwrap();
    let geometry = SketchGeometry::try_from(SketchGeometryDefinition::Nurbs { curve }).unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = super::super::sketch_geometry_point(&geometry, 0.5, Some(&ctx)).unwrap_err();
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.operation == "f3d nurbs evaluator poles"));
}

#[test]
fn sketch_nurbs_point_refuses_weight_copy_limit() {
    let curve = PcurveNurbs::from_lanes(
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)],
        Some(vec![1.0, 1.0]),
        false,
    )
    .unwrap();
    let geometry = SketchGeometry::try_from(SketchGeometryDefinition::Nurbs { curve }).unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 2;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = super::super::sketch_geometry_point(&geometry, 0.5, Some(&ctx)).unwrap_err();
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.operation == "f3d nurbs evaluator weights"));
}

#[test]
fn certified_nurbs_tubes_refuse_point_copy_limit() {
    let curve = PcurveNurbs::from_lanes(
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)],
        None,
        false,
    )
    .unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        super::super::certified_nurbs_tubes(&curve, 0.5, Some(&ctx)),
        Err(CodecError::ResourceLimit(limit)) if limit.operation == "f3d nurbs tube points"
    ));
}

#[test]
fn certified_nurbs_tubes_refuse_weight_copy_limit() {
    let curve = PcurveNurbs::from_lanes(
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)],
        Some(vec![1.0, 1.0]),
        false,
    )
    .unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 2;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        super::super::certified_nurbs_tubes(&curve, 0.5, Some(&ctx)),
        Err(CodecError::ResourceLimit(limit)) if limit.operation == "f3d nurbs tube weights"
    ));
}
