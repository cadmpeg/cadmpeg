// SPDX-License-Identifier: Apache-2.0

use crate::brep::index::scan_carriers;


#[test]
fn incomplete_brep_marker_does_not_consume_collection_slots() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let body = [0x00, 0x1e, 0x00];
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&body, &arena, &policy).unwrap();
    let carriers = scan_carriers(&ctx, &body).unwrap();
    assert!(carriers.curve_attrs(&ctx).unwrap().is_empty());
    let tables = crate::brep::topology::scan(&ctx, &body).unwrap();
    assert!(tables.points().is_empty());
    assert!(tables.loops().is_empty());
    ctx.finish_session().unwrap();
    // Graph construction owns one annotation-stream handle even without records.
    policy.limits.max_collection_items = 1;
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&body, &arena, &policy).unwrap();
    let decoded =
        crate::brep::graph::decode_body(&ctx, &body, &cadmpeg_ir::stream_name!("incomplete-marker"))
            .unwrap();
    assert!(decoded.points.is_empty());
    assert!(decoded.loops.is_empty());
    ctx.finish_session().unwrap();
}

#[test]
fn empty_brep_scan_preserves_work_refusal_without_candidate_census() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let body = [0xff; 4096];
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&body, &arena, &policy).unwrap();
    let stream = cadmpeg_ir::stream_name!("no-candidates");
    let (result, allocations) = crate::test_support::allocation::count_allocations(|| {
        crate::brep::graph::decode_body(&ctx, &body, &stream)
    });
    let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = result else {
        panic!("work refusal");
    };
    assert_eq!(
        limit.dimension,
        cadmpeg_core::decode::ResourceDimension::WorkUnits
    );
    assert_eq!(limit.operation, "scan SLDPRT analytic carriers");
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert_eq!(allocations, 0);
}
