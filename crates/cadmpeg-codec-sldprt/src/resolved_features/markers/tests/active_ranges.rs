//! Spatial point output does not retain the active relation range workspace.

use super::super::{sketch_input_entities, SpatialLaneIndex};
use crate::layout::compact_current_spatial_marker_point as layout;
use crate::records::FeatureInputLane;
use crate::resolved_features::SKETCH_MARKER;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::math::Point3;
use std::collections::HashMap;

// Core amortized Vec growth uses four slots for each of these element types.
const RANGE_BYTES: u64 =
    4 * cadmpeg_core::decode::u64_from_index(std::mem::size_of::<(u64, u64)>());
const POINT_BYTES: u64 =
    4 * cadmpeg_core::decode::u64_from_index(std::mem::size_of::<(&str, Point3, usize)>());
const FEATURE: &str = "sldprt:history:feature#active-range";
const MARKER_OFFSET: usize = 10;

fn lane(native_kind: u32) -> FeatureInputLane {
    let mut marker = vec![0; layout::LEN];
    marker[layout::MARKER..layout::HEADER].copy_from_slice(SKETCH_MARKER);
    marker[layout::HEADER..layout::SENTINEL].fill(0xff);
    marker[layout::SENTINEL..layout::NATIVE_KIND].copy_from_slice(&(-1.0_f32).to_le_bytes());
    marker[layout::NATIVE_KIND..layout::NATIVE_KIND + 4]
        .copy_from_slice(&native_kind.to_le_bytes());
    marker[layout::PROFILE_LOCUS..layout::PROFILE_ROLE].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    marker[layout::PROFILE_ROLE..layout::PROFILE_ROLE + 2].copy_from_slice(&1u16.to_le_bytes());
    marker[layout::SELECTOR..layout::SELECTOR + 8]
        .copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    marker[layout::STATE_VALUE..layout::STATE_VALUE + 8].copy_from_slice(&1.0_f64.to_le_bytes());
    marker[layout::COORDINATE_TAG..layout::COORDINATES].copy_from_slice(&[0x0e, 0x00]);
    for (index, value) in [0.125_f64, -0.25, 0.375].into_iter().enumerate() {
        let start = layout::COORDINATES + index * 8;
        marker[start..start + 8].copy_from_slice(&value.to_le_bytes());
    }
    let mut payload = vec![0x55; MARKER_OFFSET];
    payload.extend(marker);
    let id = "sldprt:feature-input:resolved-features#active-range";
    let sketch_entities = sketch_input_entities(&payload, id);
    assert_eq!(sketch_entities.len(), 1);
    FeatureInputLane {
        id: id.into(),
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
        sketch_entities,
    }
}

fn index(lane: &FeatureInputLane, range: (u64, u64)) -> SpatialLaneIndex<'_, '_> {
    // Immutable source/index fixture storage precedes the helper's caller session.
    SpatialLaneIndex {
        lane,
        names: None,
        relation_ranges: vec![range],
        scalar_offsets: HashMap::from([(FEATURE, vec![u64::midpoint(range.0, range.1)])]),
        markers: HashMap::from([(FEATURE, vec![&lane.sketch_entities[0]])]),
        feature_offsets: None,
    }
}

#[test]
fn spatial_active_range_workspace_retains_only_point_output() {
    let lane = lane(1);
    let index = index(&lane, (0, 100));
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = POINT_BYTES;
    policy.limits.max_materialized_bytes = RANGE_BYTES;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let points = index.feature_points(&ctx, FEATURE, false).unwrap();
    assert_eq!(
        points.as_slice(),
        &[(
            lane.sketch_entities[0].id(),
            Point3::new(125.0, -250.0, 375.0),
            MARKER_OFFSET
        )]
    );
    ctx.reserve_scoped(RANGE_BYTES, "reuse active spatial range workspace")
        .unwrap();
    assert!(ctx.resource_refusal().is_none());
}

#[test]
fn spatial_active_range_workspace_discards_filtered_markers() {
    // Code zero is valid compact geometry, but an active range requires 1..=85.
    for (code, range) in [(0, (0, 100)), (1, (20, 100))] {
        let lane = lane(code);
        let index = index(&lane, range);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = RANGE_BYTES;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        for _ in 0..2 {
            assert!(index
                .feature_points(&ctx, FEATURE, false)
                .unwrap()
                .is_empty());
            ctx.reserve_scoped(RANGE_BYTES, "reuse filtered spatial range workspace")
                .unwrap();
        }
        assert!(ctx.resource_refusal().is_none());
    }
}

#[test]
fn spatial_active_range_workspace_preserves_original_refusals() {
    for (dimension, retained, materialized, operation, additional) in [
        (
            ResourceDimension::RetainedBytes,
            POINT_BYTES - 1,
            RANGE_BYTES,
            "collect SLDPRT spatial points",
            POINT_BYTES,
        ),
        (
            ResourceDimension::MaterializedBytes,
            POINT_BYTES,
            RANGE_BYTES - 1,
            "collect SLDPRT active spatial relation ranges",
            RANGE_BYTES,
        ),
    ] {
        let lane = lane(1);
        let index = index(&lane, (0, 100));
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = retained;
        policy.limits.max_materialized_bytes = materialized;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let resolve = || index.feature_points(&ctx, FEATURE, false);
        let CodecError::ResourceLimit(limit) = resolve().unwrap_err() else {
            panic!("expected active spatial range refusal");
        };
        assert_eq!(limit.dimension, dimension);
        assert_eq!(limit.operation, operation);
        assert_eq!(limit.used, 0);
        assert_eq!(limit.additional, additional);
        assert_eq!(limit.limit, additional - 1);
        assert_eq!(ctx.resource_refusal().as_ref(), Some(&limit));
        let CodecError::ResourceLimit(repeated) = resolve().unwrap_err() else {
            panic!("expected sticky active spatial range refusal");
        };
        assert_eq!(repeated, limit);
    }
}
