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

#[test]
fn sketch_nurbs_endpoints_refuse_pole_copy_limit() {
    let curve = PcurveNurbs::from_lanes(
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)],
        None,
        false,
    )
    .unwrap();
    let sketch_id = SketchId::mint("synthetic:test:id#nurbs-endpoint-sketch").unwrap();
    let entity = SketchEntity::new(
        SketchEntityId::mint("synthetic:test:id#nurbs-endpoint-curve").unwrap(),
        sketch_id,
        SketchGeometry::try_from(SketchGeometryDefinition::Nurbs { curve }).unwrap(),
    );
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        super::super::sketch_entity_endpoints(&entity, Some(&ctx)),
        Err(CodecError::ResourceLimit(limit)) if limit.operation == "f3d nurbs evaluator poles"
    ));
}

#[test]
fn closed_sketch_nurbs_endpoints_propagate_collection_refusal() {
    let curve = PcurveNurbs::from_lanes(
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)],
        None,
        false,
    )
    .unwrap();
    let sketch_id = SketchId::mint("synthetic:test:id#closed-nurbs-sketch").unwrap();
    let entity = SketchEntity::new(
        SketchEntityId::mint("synthetic:test:id#closed-nurbs-curve").unwrap(),
        sketch_id.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Nurbs { curve }).unwrap(),
    );
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        closed_sketch_profiles(Some(&ctx), &sketch_id, &[entity], 0.01),
        Err(CodecError::ResourceLimit(limit)) if limit.operation == "f3d nurbs evaluator poles"
    ));
}

#[test]
fn coincident_nurbs_loci_propagate_endpoint_refusal() {
    let curve = PcurveNurbs::from_lanes(
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)],
        None,
        false,
    )
    .unwrap();
    let sketch_id = SketchId::mint("synthetic:test:id#coincident-nurbs-sketch").unwrap();
    let nurbs = SketchEntity::new(
        SketchEntityId::mint("synthetic:test:id#coincident-nurbs-curve").unwrap(),
        sketch_id.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Nurbs { curve }).unwrap(),
    );
    let point = SketchEntity::new(
        SketchEntityId::mint("synthetic:test:id#coincident-nurbs-point").unwrap(),
        sketch_id,
        SketchGeometry::try_from(SketchGeometryDefinition::Point {
            position: Point2::new(0.0, 0.0),
        })
        .unwrap(),
    );
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        crate::design::dimensions::exact_coincident_loci(&[&nurbs, &point], Some(&ctx)),
        Err(CodecError::ResourceLimit(limit)) if limit.operation == "f3d nurbs evaluator poles"
    ));
}
