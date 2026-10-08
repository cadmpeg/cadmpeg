// SPDX-License-Identifier: Apache-2.0
//! Validation stops admission at the first failing source element.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use crate::curves::GeometryError;

fn used_work(ctx: &DecodeContext<'_>) -> u64 {
    let CodecError::ResourceLimit(limit) = ctx.charge_work(u64::MAX, "test validation work")
        .expect_err("work probe must refuse") else {
        panic!("work refusal required");
    };
    limit.used
}

#[test]
fn duplicate_brep_reference_stops_before_unused_suffix() {
    let work = |values: &[i32]| {
        let arena = DecodeArena::new();
        let ctx = DecodeContext::new(&arena, &DecodePolicy::service(), false);
        let error = super::super::unique(&ctx, values, "edge trim").expect_err("duplicate reference");
        assert!(matches!(error, GeometryError::Malformed(crate::chunks::FramingError::Unpositioned { message })
            if message == "edge trim reference is duplicated"));
        assert_eq!(ctx.resource_refusal(), None);
        used_work(&ctx)
    };
    let mut long = vec![7, 7];
    long.extend(0..1022);
    assert_eq!(work(&[7, 7]), work(&long));
}

#[test]
fn invalid_brep_slot_stops_before_unused_suffix() {
    let work = |values: &[i32]| {
        let arena = DecodeArena::new();
        let ctx = DecodeContext::new(&arena, &DecodePolicy::service(), false);
        let error = super::super::slots(&ctx, values, 1024, "vertex edge").expect_err("invalid first slot");
        assert!(matches!(error, GeometryError::Malformed(crate::chunks::FramingError::Unpositioned { message })
            if message == "vertex edge reference is out of range"));
        assert_eq!(ctx.resource_refusal(), None);
        used_work(&ctx)
    };
    let mut long = vec![-1];
    long.extend(0..1023);
    assert_eq!(work(&[-1]), work(&long));
}

#[test]
fn empty_first_brep_ring_does_not_admit_later_loops() {
    let mut raw = super::one_face_raw();
    let mut resolved = super::degenerate_trim_resolved(Some(0));
    raw.loops = vec![raw.loops[0].clone(); 1024];
    resolved.loops = vec![resolved.loops[0].clone(); 1024];
    resolved.loops[0].trims.clear();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    let arena = DecodeArena::new();
    let ctx = DecodeContext::new(&arena, &policy, false);
    assert!(matches!(super::super::validate_rings(&ctx, &raw, &resolved),
        Err(GeometryError::Malformed(crate::chunks::FramingError::Structural { offset, message }))
            if offset == raw.loops[0].source_range.start && message == "loop ring is empty"));
    assert_eq!(ctx.resource_refusal(), None);
    assert_eq!(used_work(&ctx), 1);
}

#[test]
fn discontinuous_first_brep_adjacency_does_not_admit_later_pairs() {
    let raw = super::one_face_raw();
    let mut resolved = super::degenerate_trim_resolved(Some(0));
    resolved.trims.push(resolved.trims[0].clone());
    resolved.trims[0].vertices = [0, 1];
    resolved.trims[1].vertices = [2, 3];
    resolved.loops[0].trims = (0..1024).map(|index| index % 2).collect();
    let mut policy = DecodePolicy::service();
    // One loop visit and its first adjacent pair execute.
    policy.limits.max_work_units = 2;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    let arena = DecodeArena::new();
    let ctx = DecodeContext::new(&arena, &policy, false);
    assert!(matches!(super::super::validate_rings(&ctx, &raw, &resolved),
        Err(GeometryError::Malformed(crate::chunks::FramingError::Structural { offset, message }))
            if offset == raw.loops[0].source_range.start
                && message == "loop ring is discontinuous between trims 0 and 1 (1 != 2)"));
    assert_eq!(ctx.resource_refusal(), None);
    assert_eq!(used_work(&ctx), 2);
}

#[test]
fn first_brep_ring_visit_preserves_its_sticky_refusal() {
    let raw = super::one_face_raw();
    let resolved = super::degenerate_trim_resolved(Some(0));
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let ctx = DecodeContext::new(&arena, &policy, false);
    assert!(matches!(super::super::validate_rings(&ctx, &raw, &resolved),
        Err(GeometryError::Codec(CodecError::ResourceLimit(limit)))
            if limit.dimension == ResourceDimension::WorkUnits
                && limit.operation == "Rhino Brep ring loop traversal"
                && limit.used == 0 && limit.additional == 1
                && ctx.resource_refusal() == Some(limit)));
}

#[test]
fn empty_brep_ring_route_preserves_original_session_refusal() {
    let mut raw = super::one_face_raw();
    let mut resolved = super::degenerate_trim_resolved(Some(0));
    raw.loops.clear();
    resolved.loops.clear();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let ctx = DecodeContext::new(&arena, &policy, false);
    let CodecError::ResourceLimit(first) = ctx.charge_work(1, "test initial refusal")
        .expect_err("initial refusal") else { panic!("resource refusal"); };
    assert!(matches!(super::super::validate_rings(&ctx, &raw, &resolved),
        Err(GeometryError::Codec(CodecError::ResourceLimit(limit))) if limit == first));
    assert_eq!(ctx.resource_refusal(), Some(first));
}
