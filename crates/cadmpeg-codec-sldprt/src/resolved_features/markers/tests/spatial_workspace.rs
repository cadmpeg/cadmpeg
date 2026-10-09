//! Spatial coordinate output retains no temporary offset roster.

use super::super::{spatial_relation_marker_coordinates, spatial_vertex_coordinates_charged};
use crate::resolved_features::SKETCH_MARKER;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::math::Point3;

const OFFSET_BYTES: u64 = 4 * std::mem::size_of::<usize>() as u64;
const VERTEX_BYTES: u64 = 4 * std::mem::size_of::<FinitePoint3>() as u64;

fn vertex_record(coordinates: [f64; 3]) -> Vec<u8> {
    let mut payload = vec![0x55; 7];
    payload.extend([
        0xff, 0xfe, 0xff, 0x06, b'V', 0x00, b'e', 0x00, b'r', 0x00, b't', 0x00, b'e', 0x00, b'x',
        0x00,
    ]);
    payload.extend([0; 27]);
    payload.extend([0x0e, 0x00]);
    for value in coordinates {
        payload.extend(value.to_le_bytes());
    }
    payload
}

#[test]
fn spatial_vertex_workspace_retains_only_finite_coordinate_output() {
    let payload = vertex_record([1.25, -2.5, 3.75]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = VERTEX_BYTES;
    policy.limits.max_materialized_bytes = OFFSET_BYTES;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let vertices = spatial_vertex_coordinates_charged(&ctx, &payload).unwrap();
    assert_eq!(vertices.as_slice(), &[FinitePoint3::new(Point3::new(1.25, -2.5, 3.75)).unwrap()]);
    ctx.reserve_scoped(OFFSET_BYTES, "reuse spatial offset workspace").unwrap();
    assert!(ctx.resource_refusal().is_none());
}

#[test]
fn spatial_vertex_workspace_discards_nonfinite_vertex_without_retention() {
    let payload = vertex_record([f64::NAN, -2.5, 3.75]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = OFFSET_BYTES;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for _ in 0..2 {
        assert!(spatial_vertex_coordinates_charged(&ctx, &payload).unwrap().is_empty());
        ctx.reserve_scoped(OFFSET_BYTES, "reuse invalid vertex offset workspace").unwrap();
    }
    assert!(ctx.resource_refusal().is_none());
}

#[test]
fn spatial_vertex_workspace_preserves_output_growth_refusal() {
    let payload = vertex_record([1.25, -2.5, 3.75]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = VERTEX_BYTES - 1;
    policy.limits.max_materialized_bytes = OFFSET_BYTES;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let resolve = || spatial_vertex_coordinates_charged(&ctx, &payload);
    let CodecError::ResourceLimit(limit) = resolve().unwrap_err() else {
        panic!("expected spatial vertex output refusal");
    };
    assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(limit.operation, "collect SLDPRT spatial vertices");
    assert_eq!(limit.limit, VERTEX_BYTES - 1);
    assert_eq!(limit.used, 0);
    assert_eq!(limit.additional, VERTEX_BYTES);
    assert_eq!(ctx.resource_refusal().as_ref(), Some(&limit));
    let CodecError::ResourceLimit(repeated) = resolve().unwrap_err() else {
        panic!("expected sticky spatial vertex refusal");
    };
    assert_eq!(repeated, limit);
}

#[test]
fn spatial_vertex_workspace_refuses_first_offset_growth() {
    let payload = vertex_record([1.25, -2.5, 3.75]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = VERTEX_BYTES;
    policy.limits.max_materialized_bytes = OFFSET_BYTES - 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let resolve = || spatial_vertex_coordinates_charged(&ctx, &payload);
    let CodecError::ResourceLimit(limit) = resolve().unwrap_err() else {
        panic!("expected spatial offset workspace refusal");
    };
    assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(limit.operation, "collect SLDPRT spatial vertex offsets");
    assert_eq!(limit.limit, OFFSET_BYTES - 1);
    assert_eq!(limit.used, 0);
    assert_eq!(limit.additional, OFFSET_BYTES);
    assert_eq!(ctx.resource_refusal().as_ref(), Some(&limit));
    let CodecError::ResourceLimit(repeated) = resolve().unwrap_err() else {
        panic!("expected sticky spatial offset refusal");
    };
    assert_eq!(repeated, limit);
}

fn relation_point(tag: usize) -> Vec<u8> {
    let mut payload = vec![0; 90];
    payload[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    payload[27..29].copy_from_slice(&1u16.to_le_bytes());
    payload[tag..tag + 2].copy_from_slice(&[0x0e, 0x00]);
    for (index, value) in [0.125_f64, -0.25, 0.375].into_iter().enumerate() {
        let start = tag + 2 + index * 8;
        payload[start..start + 8].copy_from_slice(&value.to_le_bytes());
    }
    payload
}

#[test]
fn spatial_relation_fixed_tag_roster_accepts_each_unique_position() {
    for tag in [56, 64] {
        let payload = relation_point(tag);
        assert_eq!(spatial_relation_marker_coordinates(&payload, 0), Some(Point3::new(125.0, -250.0, 375.0)));
    }
}

#[test]
fn spatial_relation_fixed_tag_roster_rejects_missing_or_ambiguous_positions() {
    let mut payload = relation_point(56);
    payload[56..58].fill(0);
    assert_eq!(spatial_relation_marker_coordinates(&payload, 0), None);
    payload[56..58].copy_from_slice(&[0x0e, 0x00]);
    payload[64..66].copy_from_slice(&[0x0e, 0x00]);
    assert_eq!(spatial_relation_marker_coordinates(&payload, 0), None);
}
