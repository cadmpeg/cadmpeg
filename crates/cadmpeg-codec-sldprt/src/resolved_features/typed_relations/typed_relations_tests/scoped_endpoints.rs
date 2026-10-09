//! Fixed endpoint pairs release their provisional roster.

use super::super::coordinate_centered_line_endpoints;
use crate::records::{SketchInputEntity, SketchInputKind};
use crate::resolved_features::SKETCH_MARKER;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

const ROSTER_BYTES: u64 = 4 * std::mem::size_of::<&SketchInputEntity>() as u64;

fn centered_source() -> (Vec<u8>, [SketchInputEntity; 3]) {
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
    let entities = [
        ("line", 0, SketchInputKind::LineOrCircle, [2.0, 3.0]),
        ("first", 143, SketchInputKind::Point, [1.0, 2.0]),
        ("second", 144, SketchInputKind::Point, [3.0, 4.0]),
    ].map(|(id, offset, kind, coordinates)| {
        let mut marker = SketchInputEntity::new(id, "lane", 0, offset, kind);
        marker.feature_ref = Some("feature".into());
        marker.coordinates_m = cadmpeg_ir::units::FiniteVector::new(coordinates);
        marker
    });
    (payload, entities)
}

#[test]
fn centered_fixed_endpoint_pair_releases_all_roster_storage() {
    let (payload, entities) = centered_source();
    let roster = [&entities[0], &entities[1], &entities[2]];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = ROSTER_BYTES;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for _ in 0..2 {
        let pair = coordinate_centered_line_endpoints(&ctx, &payload, &entities[0], &roster)
            .unwrap().unwrap();
        assert!(std::ptr::eq(pair[0], &entities[1]));
        assert!(std::ptr::eq(pair[1], &entities[2]));
        ctx.reserve_scoped(ROSTER_BYTES, "reuse centered endpoint roster").unwrap();
    }
    assert!(ctx.resource_refusal().is_none());
}

#[test]
fn centered_fixed_endpoint_pair_refuses_the_first_roster_growth() {
    let (payload, entities) = centered_source();
    let roster = [&entities[0], &entities[1], &entities[2]];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = ROSTER_BYTES - 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let resolve = || coordinate_centered_line_endpoints(&ctx, &payload, &entities[0], &roster);
    let CodecError::ResourceLimit(limit) = resolve().unwrap_err() else {
        panic!("expected centered roster refusal");
    };
    assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(limit.operation, "resolve SLDPRT coordinate centered line endpoints");
    assert_eq!(limit.limit, ROSTER_BYTES - 1);
    assert_eq!(limit.used, 0);
    assert_eq!(limit.additional, ROSTER_BYTES);
    assert_eq!(ctx.resource_refusal().as_ref(), Some(&limit));
    let CodecError::ResourceLimit(repeated) = resolve().unwrap_err() else {
        panic!("expected sticky centered roster refusal");
    };
    assert_eq!(repeated, limit);
}
