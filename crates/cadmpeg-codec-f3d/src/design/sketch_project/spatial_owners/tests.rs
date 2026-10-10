// SPDX-License-Identifier: Apache-2.0

use super::{sketch_curve_is_spatial, spatial_geometry_owners};
use crate::records::sketch_geometry::{
    SketchCurveGeometry, SketchCurveIdentity, SketchNurbsGeometry, SketchNurbsPoles, SketchPoint,
    SketchPointCompanion, SketchPointDraft, SketchPointRecordForm,
};
use cadmpeg_core::decode::{
    DecodeArena, DecodeContext, DecodePolicy, ResourceDimension, ResourceFailure, ResourceLimit,
};
use cadmpeg_core::CodecError;
use cadmpeg_ir::math::{Point2, Point3, Vector3};

fn policy(work: u64) -> DecodePolicy {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = work;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    policy
}

fn curve(geometry: Option<SketchCurveGeometry>, owner: Option<u32>) -> SketchCurveIdentity {
    SketchCurveIdentity {
        id: "f3d:test:curve#1".into(), record_index: 1, owner_reference: owner,
        class_tag: "301".to_owned().try_into().unwrap(), byte_offset: 0,
        geometry_offset: 0, entity_genesis: None,
        primary_id: std::num::NonZeroU64::new(1).unwrap(), secondary_id: 0, geometry,
    }
}

fn nurbs(depths: &[f64], rational: bool) -> SketchCurveGeometry {
    let points = depths.iter().map(|depth| Point3::new(0.0, 0.0, *depth)).collect();
    let weights = if rational { depths.iter().map(|_| 2.0).collect() } else { Vec::new() };
    let knots = (0..depths.len() + 2).map(|index| f64::from(u32::try_from(index).unwrap())).collect();
    SketchCurveGeometry::nurbs_from_parts(None, "302".to_owned().try_into().unwrap(), 2,
        SketchNurbsGeometry::from_parts(1, 0.0, 8, knots,
            SketchNurbsPoles::from_wire(points, weights).unwrap()).unwrap())
}

fn point(owner: Option<u32>, depth: f64) -> SketchPoint {
    SketchPoint::try_from(SketchPointDraft {
        id: "f3d:test:point#1".into(), record_index: 1, owner_reference: owner,
        class_tag: "000".to_owned().try_into().unwrap(), byte_offset: 0,
        coordinate_offset: 1, paired_reference: 2,
        coordinates: Point2::new(2.0, 3.0),
        record_form: SketchPointRecordForm::Version8 {
            depth, persistent_id: std::num::NonZeroU64::new(1).unwrap(),
            flags: [false; 7],
        },
        companion: SketchPointCompanion { incident_curves: vec![7, 2] },
    }).unwrap()
}

fn refusal(error: CodecError, operation: &'static str, cap: u64) -> ResourceLimit {
    let CodecError::ResourceLimit(actual) = error else { panic!("expected resource refusal: {error:?}"); };
    assert_eq!(actual, ResourceLimit {
        dimension: ResourceDimension::WorkUnits, reason: ResourceFailure::BudgetExceeded,
        limit: cap, used: cap, additional: 1, operation,
    });
    actual
}

#[test]
fn spatial_curve_constant_geometry_is_free_then_keeps_original_refusal() {
    let line = |depth| SketchCurveGeometry::line(
        Point3::new(0.0, 0.0, depth), Point3::new(1.0, 0.0, depth),
        Vector3::new(1.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0)).unwrap();
    let arc = |depth| SketchCurveGeometry::arc(Point3::new(0.0, 0.0, depth),
        Vector3::new(0.0, 0.0, 1.0), Vector3::new(1.0, 0.0, 0.0), 1.0, 0.0, 1.0).unwrap();
    let cases = [(None, false), (Some(line(0.0)), false), (Some(line(1.0)), true),
        (Some(arc(0.0)), false), (Some(arc(1.0)), true)];
    let curves = cases.into_iter().map(|(geometry, expected)| (curve(geometry, Some(42)), expected)).collect::<Vec<_>>();
    let arena = DecodeArena::new();
    let policy = policy(0);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for (curve, expected) in &curves {
        assert_eq!(sketch_curve_is_spatial(&ctx, curve).unwrap(), *expected);
    }
    assert_eq!(ctx.resource_refusal(), None);
    let original = refusal(ctx.charge_work(1, "original spatial refusal").unwrap_err(), "original spatial refusal", 0);
    for (curve, _) in &curves {
        assert!(matches!(sketch_curve_is_spatial(&ctx, curve),
            Err(CodecError::ResourceLimit(actual)) if actual == original));
    }
    assert!(matches!(spatial_geometry_owners(&ctx, &[], &[]),
        Err(CodecError::ResourceLimit(actual)) if actual == original));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == original));
}

#[test]
fn spatial_nurbs_classification_pays_only_reached_polynomial_and_rational_poles() {
    // A first/second spatial pole stops before any remaining source step.
    // All-planar poles require two visits and the actual terminal probe.
    for rational in [false, true] {
        for (depths, work, expected) in [([1.0, 0.0], 1, true), ([0.0, 1.0], 2, true), ([0.0, 0.0], 3, false)] {
            let curve = curve(Some(nurbs(&depths, rational)), Some(42));
            let before = serde_json::to_value(&curve).unwrap();
            for allowed in [true, false] {
                let cap = work - u64::from(!allowed);
                let arena = DecodeArena::new();
                let policy = policy(cap);
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                let result = sketch_curve_is_spatial(&ctx, &curve);
                if allowed {
                    assert_eq!(result.unwrap(), expected);
                    assert_eq!(ctx.resource_refusal(), None);
                    ctx.finish_session().unwrap();
                } else {
                    let original = refusal(result.unwrap_err(), "classify F3D sketch NURBS poles", cap);
                    assert!(matches!(spatial_geometry_owners(&ctx, &[], &[]),
                        Err(CodecError::ResourceLimit(actual)) if actual == original));
                    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == original));
                }
                assert_eq!(serde_json::to_value(&curve).unwrap(), before);
            }
        }
    }
}

#[test]
fn spatial_nurbs_first_pole_leaves_long_native_suffix_unvisited() {
    for rational in [false, true] {
        let mut depths = [0.0; 64];
        depths[0] = 1.0;
        let curve = curve(Some(nurbs(&depths, rational)), Some(42));
        let before = serde_json::to_value(&curve).unwrap();
        let arena = DecodeArena::new();
        let policy = policy(1);
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(sketch_curve_is_spatial(&ctx, &curve).unwrap());
        assert_eq!(serde_json::to_value(&curve).unwrap(), before);
        ctx.finish_session().unwrap();
    }
}

#[test]
fn spatial_empty_poles_and_sources_need_no_positive_resource() {
    let curve = curve(Some(nurbs(&[], false)), Some(42));
    let before = serde_json::to_value(&curve).unwrap();
    let arena = DecodeArena::new();
    let policy = policy(0);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(!sketch_curve_is_spatial(&ctx, &curve).unwrap());
    assert!(spatial_geometry_owners(&ctx, &[], &[]).unwrap().is_empty());
    assert_eq!(serde_json::to_value(&curve).unwrap(), before);
    assert_eq!(ctx.resource_refusal(), None);
    ctx.finish_session().unwrap();
}

#[test]
fn spatial_missing_owners_skip_geometry_and_identity_children() {
    let mut curve = curve(Some(nurbs(&[1.0, 1.0], true)), None);
    curve.id = "u".repeat(4096);
    let mut point = point(None, 1.0);
    point.id = "p".repeat(4096);
    let before = (serde_json::to_value(&curve).unwrap(), serde_json::to_value(&point).unwrap());
    // Only each concrete source slot and its terminal probe are executed.
    let arena = DecodeArena::new();
    let policy = policy(4);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(spatial_geometry_owners(&ctx, std::slice::from_ref(&point), std::slice::from_ref(&curve)).unwrap().is_empty());
    assert_eq!((serde_json::to_value(&curve).unwrap(), serde_json::to_value(&point).unwrap()), before);
    ctx.finish_session().unwrap();
}

#[test]
fn spatial_point_stream_refuses_before_owner_backing_or_publication() {
    let point = point(Some(42), 1.0);
    let before = serde_json::to_value(&point).unwrap();
    let arena = DecodeArena::new();
    let policy = policy(1);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let original = refusal(spatial_geometry_owners(&ctx, std::slice::from_ref(&point), &[]).unwrap_err(),
        "resolve F3D spatial point owner stream", 1);
    assert_eq!(serde_json::to_value(&point).unwrap(), before);
    assert!(matches!(spatial_geometry_owners(&ctx, &[], &[]),
        Err(CodecError::ResourceLimit(actual)) if actual == original));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == original));
}

#[test]
fn spatial_planar_points_skip_unneeded_identity_search() {
    let mut point = point(Some(42), 0.0);
    point.id = "p".repeat(4096);
    let before = serde_json::to_value(&point).unwrap();
    let arena = DecodeArena::new();
    let policy = policy(2);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(spatial_geometry_owners(&ctx, std::slice::from_ref(&point), &[]).unwrap().is_empty());
    assert_eq!(serde_json::to_value(&point).unwrap(), before);
    ctx.finish_session().unwrap();
}

#[test]
fn spatial_owner_classifies_planar_nurbs_before_unneeded_long_stream_query() {
    let mut curve = curve(Some(nurbs(&[0.0, 0.0], true)), Some(42));
    curve.id = "u".repeat(4096);
    let before = serde_json::to_value(&curve).unwrap();
    // Curve slot, two planar poles and their end probe, then the curve end probe.
    for cap in [5, 4] {
        let arena = DecodeArena::new();
        let policy = policy(cap);
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = spatial_geometry_owners(&ctx, &[], std::slice::from_ref(&curve));
        if cap == 5 {
            assert!(result.unwrap().is_empty());
            ctx.finish_session().unwrap();
        } else {
            let original = refusal(result.unwrap_err(), "visit F3D spatial owner curve", cap);
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == original));
        }
        assert_eq!(serde_json::to_value(&curve).unwrap(), before);
    }
}

#[test]
fn spatial_owner_nurbs_refusal_precedes_stream_search_and_backing() {
    let mut curve = curve(Some(nurbs(&[1.0, 0.0], false)), Some(42));
    curve.id = "u".repeat(4096);
    let before = serde_json::to_value(&curve).unwrap();
    let arena = DecodeArena::new();
    let policy = policy(1);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let original = refusal(spatial_geometry_owners(&ctx, &[], std::slice::from_ref(&curve)).unwrap_err(),
        "classify F3D sketch NURBS poles", 1);
    assert_eq!(serde_json::to_value(&curve).unwrap(), before);
    assert!(matches!(spatial_geometry_owners(&ctx, &[], &[]),
        Err(CodecError::ResourceLimit(actual)) if actual == original));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == original));
}
