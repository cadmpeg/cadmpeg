//! Endpoint fallback publishes only its surviving pointer slots.

use crate::records::{SketchInputEntity, SketchInputKind, SketchInputLink, SketchInputLinks};
use crate::resolved_features::endpoints::geometry_index::{MarkerGeometryIndex, MarkerPrefixIndex};
use crate::resolved_features::endpoints::output_curve_endpoint_markers;
use crate::resolved_features::SKETCH_MARKER;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use std::collections::HashMap;

const ENDPOINT_BYTES: u64 = 4 * std::mem::size_of::<&SketchInputEntity>() as u64;

fn centered_payload() -> Vec<u8> {
    let mut payload = vec![0; 142 + SKETCH_MARKER.len()];
    payload[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    payload[27..29].copy_from_slice(&1u16.to_le_bytes());
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[64..66].copy_from_slice(&[0x1e, 0x00]);
    payload[66..74].copy_from_slice(&2.0f64.to_le_bytes());
    payload[74..82].copy_from_slice(&3.0f64.to_le_bytes());
    payload[82..86].copy_from_slice(&1u32.to_le_bytes());
    payload[92..96].copy_from_slice(&(-2i32).to_le_bytes());
    payload[142..].copy_from_slice(SKETCH_MARKER);
    payload
}

fn centered_entities() -> [SketchInputEntity; 3] {
    let mut line = SketchInputEntity::new("line", "lane", 0, 0, SketchInputKind::LineOrCircle);
    line.feature_ref = Some("feature".into());
    line.coordinates_m = cadmpeg_ir::units::FiniteVector::new([2.0, 3.0]);
    line.links = SketchInputLinks::new(0, vec![SketchInputLink {
        local_id: 1,
        entity_ref: "first".into(),
    }]);
    let mut first = SketchInputEntity::new("first", "lane", 1, 143, SketchInputKind::Point);
    first.feature_ref = Some("feature".into());
    first.coordinates_m = cadmpeg_ir::units::FiniteVector::new([1.0, 2.0]);
    let mut second = SketchInputEntity::new("second", "lane", 2, 144, SketchInputKind::Point);
    second.feature_ref = Some("feature".into());
    second.coordinates_m = cadmpeg_ir::units::FiniteVector::new([3.0, 4.0]);
    [line, first, second]
}

#[test]
fn centered_endpoint_fallback_retains_only_the_selected_vector() {
    let payload = centered_payload();
    let entities = centered_entities();
    let roster = [&entities[0], &entities[1], &entities[2]];
    let by_id = roster.into_iter().map(|marker| (marker.id(), marker)).collect::<HashMap<_, _>>();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = ENDPOINT_BYTES;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let geometry = MarkerGeometryIndex::new(
        &ctx, &roster, MarkerPrefixIndex::new(&ctx, &payload).unwrap(),
    ).unwrap();
    let endpoints = output_curve_endpoint_markers(
        &ctx, &payload, &entities[0], &by_id, &roster, &geometry,
    ).unwrap();
    assert_eq!(endpoints.len(), 2);
    assert!(std::ptr::eq(endpoints[0], &entities[1]));
    assert!(std::ptr::eq(endpoints[1], &entities[2]));
    assert_eq!(endpoints[0].coordinates_m.unwrap().get(), [1.0, 2.0]);
    assert_eq!(endpoints[1].coordinates_m.unwrap().get(), [3.0, 4.0]);
    assert!(ctx.resource_refusal().is_none());
}

#[test]
fn centered_endpoint_fallback_preserves_the_output_refusal() {
    let payload = centered_payload();
    let entities = centered_entities();
    let roster = [&entities[0], &entities[1], &entities[2]];
    let by_id = roster.into_iter().map(|marker| (marker.id(), marker)).collect::<HashMap<_, _>>();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = ENDPOINT_BYTES - 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let geometry = MarkerGeometryIndex::new(
        &ctx, &roster, MarkerPrefixIndex::new(&ctx, &payload).unwrap(),
    ).unwrap();
    let resolve = || output_curve_endpoint_markers(
        &ctx, &payload, &entities[0], &by_id, &roster, &geometry,
    );
    let CodecError::ResourceLimit(limit) = resolve().unwrap_err() else {
        panic!("expected endpoint publication refusal");
    };
    assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(limit.operation, "resolve SLDPRT output curve endpoints");
    assert_eq!(limit.limit, ENDPOINT_BYTES - 1);
    assert_eq!(limit.used, 0);
    assert_eq!(limit.additional, ENDPOINT_BYTES);
    assert_eq!(ctx.resource_refusal().as_ref(), Some(&limit));
    let CodecError::ResourceLimit(repeated) = resolve().unwrap_err() else {
        panic!("expected sticky endpoint refusal");
    };
    assert_eq!(repeated, limit);
}
