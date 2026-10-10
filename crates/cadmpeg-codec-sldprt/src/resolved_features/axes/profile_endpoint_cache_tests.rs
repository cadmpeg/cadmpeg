//! Endpoint-index reuse within implicit profile-axis selection.

use crate::records::{FeatureInputLane, SketchInputEntity, SketchInputKind};
use crate::resolved_features::SKETCH_MARKER;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn profile_line() -> FeatureInputLane {
    let mut payload = vec![0; 284 + SKETCH_MARKER.len()];
    let offset = 200;
    payload[offset..offset + SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[offset + 5..offset + 13].fill(0xff);
    payload[offset + 13..offset + 17].copy_from_slice(&[0, 0, 0x80, 0xbf]);
    payload[offset + 17..offset + 21].copy_from_slice(&2u32.to_le_bytes());
    payload[offset + 23..offset + 27].copy_from_slice(&[4, 0, 2, 0]);
    payload[offset + 27..offset + 29].copy_from_slice(&1u16.to_le_bytes());
    payload[offset + 31..offset + 39].copy_from_slice(&[0, 0, 0x80, 0xbf, 0, 0, 4, 0]);
    payload[offset + 48..offset + 56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[offset + 58..offset + 60].copy_from_slice(&1u16.to_le_bytes());
    payload[offset + 60..offset + 64].copy_from_slice(&1u32.to_le_bytes());
    payload[284..].copy_from_slice(SKETCH_MARKER);
    let marker = |id: &str, offset, index, coordinates: Option<[f64; 2]>| {
        let mut marker = SketchInputEntity::new(
            id.to_owned(),
            "lane".to_owned(),
            0,
            offset,
            if coordinates.is_some() {
                SketchInputKind::Point
            } else {
                SketchInputKind::LineOrCircle
            },
        )
        .with_test_identity(index, None);
        marker.feature_ref = Some("profile".to_owned());
        marker.coordinates_m = coordinates.and_then(cadmpeg_ir::units::FiniteVector::new);
        marker
    };
    FeatureInputLane {
        id: "lane".to_owned(),
        configuration: None,
        native_payload: payload,
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
            marker("start", 0, Some(1), Some([0.0, 0.0])),
            marker("end", 100, Some(2), Some([0.01, 0.0])),
            marker("curve", 200, None, None),
        ],
    }
}

#[test]
fn implicit_profile_axis_keeps_endpoint_identity_and_original_index_refusal() {
    let lane = profile_line();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&lane.native_payload, &arena, &policy).unwrap();
    let roster = super::LaneMarkerIndex::new(&ctx, lane.sketch_entities.iter()).unwrap();
    let endpoints = super::profile_roster_implicit_axis_endpoints(&ctx, &lane, "profile", &roster)
        .unwrap()
        .expect("one bounded line resolves the axis");
    assert_eq!(endpoints.map(SketchInputEntity::id), ["start", "end"]);
    crate::test_support::work_refusal_at("index SLDPRT marker geometry", |ctx| {
        let roster = super::LaneMarkerIndex::new(ctx, lane.sketch_entities.iter())?;
        super::profile_roster_implicit_axis_endpoints(ctx, &lane, "profile", &roster)
    });
}

#[test]
fn endpoint_id_pass_leaves_geometry_available_for_the_axis_fallback() {
    const OPERATION: &str = "index SLDPRT marker geometry";
    let lane = profile_line();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    let (ctx, _) = DecodeContext::from_root_bytes(&lane.native_payload, &arena, &policy).unwrap();
    let markers: Vec<_> = lane.sketch_entities.iter().collect();
    let mut geometry = None;
    let curves = super::super::typed_relations::CurveMarkers::new(&ctx, &markers).unwrap();
    let mut ids_storage = ctx.reserve_scoped(0, "hold test endpoint IDs").unwrap();
    let ids =
        super::profile_curve_endpoint_ids_with(&ctx, &mut ids_storage, &markers, false, |curve| {
            super::roster_curve_endpoint_markers_lazy(
                &ctx,
                &lane.native_payload,
                curve,
                &markers,
                &mut geometry,
                Some(&curves),
            )
        })
        .unwrap();
    assert_eq!(ids.into_iter().collect::<Vec<_>>(), vec!["end", "start"]);
    drop(ids_storage);
    assert!(geometry.is_some());
    let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
        ResourceDimension::WorkUnits,
        OPERATION,
        None,
    );
    let (endpoint_storage, endpoints);
    (endpoints, endpoint_storage) = ctx
        .with_scoped_storage("hold test fallback endpoints", || {
            super::roster_curve_endpoint_markers_lazy(
                &ctx,
                &lane.native_payload,
                &lane.sketch_entities[2],
                &markers,
                &mut geometry,
                Some(&curves),
            )
        })
        .unwrap();
    assert_eq!(
        endpoints
            .iter()
            .map(|marker| marker.id())
            .collect::<Vec<_>>(),
        vec!["start", "end"]
    );
    drop((endpoints, endpoint_storage));
    let CodecError::ResourceLimit(limit) = ctx
        .charge_work(1, OPERATION)
        .expect_err("the cached fallback leaves the index-construction probe armed")
    else {
        panic!("work refusal");
    };
    assert_eq!(limit.operation, OPERATION);
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!(ctx.resource_refusal(), Some(limit));
}
