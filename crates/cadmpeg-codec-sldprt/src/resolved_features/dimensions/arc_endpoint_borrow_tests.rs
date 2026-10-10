//! Native endpoint identities are borrowed until final arc publication.

use super::{
    dimensioned_arc_native_geometry, DimensionedArcNative, DimensionedCurveNative, LaneMarkerIndex,
};
use crate::records::{FeatureInputLane, SketchInputEntity, SketchInputKind, SketchInputLink};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn endpoint_lane() -> FeatureInputLane {
    let marker = |id: &str, ordinal, kind, coordinates| {
        let mut marker = SketchInputEntity::new(id, "lane", ordinal, u64::from(ordinal) * 10, kind);
        marker.feature_ref = Some("feature".into());
        marker.coordinates_m = cadmpeg_ir::units::FiniteVector::new(coordinates);
        marker
    };
    let mut center = marker("center", 1, SketchInputKind::Arc, [0.0, 0.0]);
    center.links = crate::records::SketchInputLinks::new(
        0,
        vec![
            SketchInputLink {
                local_id: 0,
                entity_ref: "start".into(),
            },
            SketchInputLink {
                local_id: 0,
                entity_ref: "end".into(),
            },
        ],
    );
    FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: Vec::new(),
        classes: Vec::new(),
        names: Vec::new(),
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: vec![
            center,
            marker("start", 2, SketchInputKind::Point, [0.003, 0.0]),
            marker("end", 3, SketchInputKind::Point, [0.0, 0.003]),
        ],
    }
}

#[test]
fn native_arc_borrows_endpoints_without_retaining_text() {
    let lane = endpoint_lane();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let index = LaneMarkerIndex::new(&ctx, std::slice::from_ref(&lane)).unwrap();
    let mut last = None;
    for _ in 0..64 {
        let Some(DimensionedCurveNative::Arc(arc)) =
            dimensioned_arc_native_geometry(&ctx, &index, &lane.sketch_entities[0], 3.0).unwrap()
        else {
            panic!("matching endpoints define a bounded arc");
        };
        let endpoints = arc.endpoints.unwrap();
        assert_eq!(endpoints, ["start", "end"]);
        assert!(std::ptr::eq(endpoints[0], lane.sketch_entities[1].id()));
        assert!(std::ptr::eq(endpoints[1], lane.sketch_entities[2].id()));
        last = Some(arc);
    }
    drop(index);
    // The lane owns the identities; they survive the lookup cache.
    assert_eq!(last.unwrap().endpoints, Some(["start", "end"]));
    assert_eq!(ctx.resource_refusal(), None);
}

#[test]
fn transformed_arc_copies_only_final_endpoint_text_with_exact_limits() {
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::sketches::{Sketch, SketchId, SketchPlacement};
    const QUANTUM: f64 = 1.0e-8;
    let sketch = Sketch {
        id: SketchId::mint("synthetic:test:id#sketch").unwrap(),
        name: None,
        configuration: None,
        visible: None,
        placement: SketchPlacement::try_resolved(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
        profiles: cadmpeg_ir::sketches::SketchProfiles::default(),
        native_ref: None,
    };
    let transform =
        super::super::transforms::sketch_frame_marker_transform(&sketch, QUANTUM).unwrap();
    let start = String::from("start");
    let end = String::from("end");
    let arc = DimensionedArcNative {
        center: [0.0, 0.0],
        start: [0.003, 0.0],
        end: [0.0, 0.003],
        endpoints: Some([start.as_str(), end.as_str()]),
    };
    // reserve_vec(2) uses four String slots (96 bytes); final text uses 5 + 3.
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 104;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_collection_items = 2;
    policy.limits.max_work_units = 8;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let output = super::transformed_dimensioned_arc(&ctx, transform, &arc, 1000.0, QUANTUM)
        .unwrap()
        .unwrap();
    assert_eq!(output.endpoint_refs, ["start", "end"]);
    assert_ne!(output.endpoint_refs[0].as_ptr(), start.as_ptr());
    assert_ne!(output.endpoint_refs[1].as_ptr(), end.as_ptr());
    assert_eq!(ctx.resource_refusal(), None);
    for (dimension, operation, used, additional) in [
        (
            ResourceDimension::RetainedBytes,
            "retain SLDPRT dimensioned arc endpoint",
            101,
            3,
        ),
        (
            ResourceDimension::WorkUnits,
            "retain SLDPRT dimensioned arc endpoint",
            5,
            3,
        ),
        (
            ResourceDimension::CollectionItems,
            "collect SLDPRT dimensioned arc endpoints",
            0,
            2,
        ),
    ] {
        let mut refusal_policy = policy.clone();
        match dimension {
            ResourceDimension::RetainedBytes => refusal_policy.limits.max_retained_bytes = 103,
            ResourceDimension::WorkUnits => refusal_policy.limits.max_work_units = 7,
            ResourceDimension::CollectionItems => refusal_policy.limits.max_collection_items = 1,
            _ => unreachable!("only the executed endpoint operations"),
        }
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &refusal_policy).unwrap();
        let error = match super::transformed_dimensioned_arc(&ctx, transform, &arc, 1000.0, QUANTUM)
        {
            Ok(_) => panic!("one-below executed endpoint demand must refuse"),
            Err(error) => error,
        };
        let CodecError::ResourceLimit(limit) = error else {
            panic!("resource refusal");
        };
        assert_eq!(limit.dimension, dimension);
        assert_eq!(limit.operation, operation);
        assert_eq!(limit.used, used);
        assert_eq!(limit.additional, additional);
        assert_eq!(ctx.resource_refusal(), Some(limit));
    }
}
