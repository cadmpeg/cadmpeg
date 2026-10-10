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
    let endpoints = super::profile_roster_implicit_axis_endpoints(&ctx,
&lane,
"profile",
&roster,
&mut super::ProfileEndpointQueries::new(&ctx, &lane, &roster.all).unwrap(),)
        .unwrap()
        .expect("one bounded line resolves the axis");
    assert_eq!(endpoints.map(SketchInputEntity::id), ["start", "end"]);
    crate::test_support::work_refusal_at("index SLDPRT marker geometry", |ctx| {
        let roster = super::LaneMarkerIndex::new(ctx, lane.sketch_entities.iter())?;
        super::profile_roster_implicit_axis_endpoints(ctx,
&lane,
"profile",
&roster,
&mut super::ProfileEndpointQueries::new(ctx, &lane, &roster.all)?,)
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
    let mut queries = super::ProfileEndpointQueries::new(&ctx, &lane, &markers).unwrap();
    let mut ids_storage = ctx.reserve_scoped(0, "hold test endpoint IDs").unwrap();
    let ids = queries.ids(&ctx, &mut ids_storage, &markers, false).unwrap();
    assert_eq!(ids.into_iter().collect::<Vec<_>>(), vec!["end", "start"]);
    drop(ids_storage);
    assert!(queries.geometry.is_some());
    let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
        ResourceDimension::WorkUnits,
        OPERATION,
        None,
    );
    let endpoints = queries.resolve(&lane.sketch_entities[2]).unwrap();
    assert_eq!(endpoints.iter().map(|marker| marker.id()).collect::<Vec<_>>(), vec!["start", "end"]);
    assert_eq!(super::profile_roster_origin_axis_endpoints(&ctx, "profile", &super::LaneMarkerIndex::new(&ctx, markers.iter().copied()).unwrap(), &mut queries).unwrap(), None);
    assert_eq!(super::profile_roster_principal_axis_endpoints(&ctx, "profile", &super::LaneMarkerIndex::new(&ctx, markers.iter().copied()).unwrap(), &mut queries).unwrap(), Some([[0.0, 0.0], [1.0, 0.0]]));
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

#[test]
fn profile_endpoint_cache_uses_scoped_storage_and_releases_it() {
    let lane = profile_line();
    let markers: Vec<_> = lane.sketch_entities.iter().collect();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 1024 * 1024;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut queries = super::ProfileEndpointQueries::new(&ctx, &lane, &markers).unwrap();
    assert_eq!(queries.resolve(&lane.sketch_entities[2]).unwrap().iter().map(|marker| marker.id()).collect::<Vec<_>>(), ["start", "end"]);
    let roster = super::LaneMarkerIndex::new(&ctx, markers.iter().copied()).unwrap();
    assert_eq!(super::profile_roster_implicit_axis_endpoints(&ctx, &lane, "profile", &roster, &mut queries).unwrap().unwrap().map(SketchInputEntity::id), ["start", "end"]);
    drop(roster);
    drop(queries);
    drop(ctx.reserve_scoped(policy.limits.max_materialized_bytes, "test axis workspace expiry").unwrap());
    ctx.finish_session().unwrap();
}

#[test]
fn profile_endpoint_cache_reuses_an_unresolved_curve_result() {
    const OPERATION: &str = "resolve SLDPRT curve endpoint roster";
    let mut lane = profile_line();
    lane.sketch_entities.retain(|marker| marker.coordinates_m.is_none());
    let markers: Vec<_> = lane.sketch_entities.iter().collect();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut queries = super::ProfileEndpointQueries::new(&ctx, &lane, &markers).unwrap();
    assert!(queries.resolve(&lane.sketch_entities[0]).unwrap().is_empty());
    let probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(ResourceDimension::WorkUnits, OPERATION, None);
    assert!(queries.resolve(&lane.sketch_entities[0]).unwrap().is_empty());
    let CodecError::ResourceLimit(limit) = ctx.charge_work(1, OPERATION).unwrap_err() else { panic!("cached empty result left the resolver probe armed"); };
    assert_eq!(limit.operation, OPERATION);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    drop(queries);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
    drop(probe);
    crate::test_support::work_refusal_at(OPERATION, |ctx| {
        let mut queries = super::ProfileEndpointQueries::new(ctx, &lane, &markers)?;
        queries.resolve(&lane.sketch_entities[0]).map(|_| ())
    });
}

#[test]
fn profile_endpoint_cache_without_a_route_uses_no_index_or_storage() {
    let lane = profile_line();
    let markers: Vec<_> = lane.sketch_entities.iter().collect();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut queries = super::ProfileEndpointQueries::new(&ctx, &lane, &markers).unwrap();
    assert!(queries.resolve(&lane.sketch_entities[0]).unwrap().is_empty());
    drop(queries);
    ctx.finish_session().unwrap();
}
