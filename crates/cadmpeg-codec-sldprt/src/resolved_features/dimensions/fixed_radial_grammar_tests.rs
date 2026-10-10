//! Fixed radial grammar rejects malformed fields without byte-work admission.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn malformed_radial_record_does_not_execute_a_fallible_fixed_comparison() {
    const OLD_COMPARISON: &str = "compare SLDPRT repeated radial indices";
    let mut payload = vec![0; 112];
    payload[..super::LEGACY_EXTENDED_SKETCH_MARKER.len()]
        .copy_from_slice(super::LEGACY_EXTENDED_SKETCH_MARKER);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..31].copy_from_slice(&[4, 0, 2, 0, 1, 0, 1, 0]);
    payload[31..39].copy_from_slice(&[0, 0, 0x80, 0xbf, 0, 0, 4, 0]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&12u16.to_le_bytes());
    payload[58..60].copy_from_slice(&12u16.to_le_bytes());
    // The old fallback compared these equal fields before rejecting the
    // missing state, signed selector and trailer; the compact grammar rejects it.
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_work_units = u64::MAX;
    let (ctx, _) = DecodeContext::from_root_bytes(&payload, &arena, &policy).unwrap();
    let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
        ResourceDimension::WorkUnits,
        OLD_COMPARISON,
        None,
    );
    assert_eq!(
        super::radial_circle_records(&ctx, &payload)
            .unwrap()
            .count(),
        0
    );
    let CodecError::ResourceLimit(limit) = ctx
        .charge_work(1, OLD_COMPARISON)
        .expect_err("the fixed grammar left the comparison probe armed")
    else {
        panic!("work refusal");
    };
    assert_eq!(limit.operation, OLD_COMPARISON);
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    drop(_probe);
    crate::test_support::work_refusal_at("scan SLDPRT radial circle records", |ctx| {
        super::radial_circle_records(ctx, &payload).map(Iterator::count)
    });
}

#[test]
fn radial_scan_admits_only_complete_marker_starts() {
    const OPERATION: &str = "scan SLDPRT radial circle records";
    // A five-byte marker has 112 - 5 + 1 complete starts in these bytes.
    const STARTS: u64 = 108;
    const REPEATS: u64 = 64;
    let payload = [0; 112];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = STARTS * REPEATS;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&payload, &arena, &policy).unwrap();
    for _ in 0..REPEATS {
        assert_eq!(
            super::radial_circle_records(&ctx, &payload)
                .unwrap()
                .count(),
            0
        );
    }
    let arena = DecodeArena::new();
    policy.limits.max_work_units = STARTS - 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&payload, &arena, &policy).unwrap();
    let CodecError::ResourceLimit(limit) = (match super::radial_circle_records(&ctx, &payload) {
        Ok(_) => panic!("the source-start admission must refuse before iteration"),
        Err(error) => error,
    }) else {
        panic!("work refusal");
    };
    assert_eq!(limit.operation, OPERATION);
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!(limit.used, 0);
    assert_eq!(limit.additional, STARTS);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    let arena = DecodeArena::new();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&payload[..4], &arena, &policy).unwrap();
    assert_eq!(
        super::radial_circle_records(&ctx, &payload[..4])
            .unwrap()
            .count(),
        0
    );
}
