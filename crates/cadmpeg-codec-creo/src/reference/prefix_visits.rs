// SPDX-License-Identifier: Apache-2.0

use super::matching_row_id;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn check(payload: &[u8], close: usize, id: u32, visits: u64, expected: bool) {
    for cap in 0..=visits {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let result = matching_row_id(&ctx, payload, close, id);
        if cap == visits {
            assert_eq!(result.expect("actual reverse-prefix visits"), expected);
            let original = ctx.charge_work_limit(1, "after matching row prefix")
                .expect_err("no unvisited work is charged");
            assert_eq!((original.used, original.additional), (visits, 1));
        } else {
            let original = ctx.resource_refusal().expect("candidate visit refuses");
            assert!(matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == original));
            assert_eq!((original.dimension, original.used, original.additional, original.operation),
                (ResourceDimension::WorkUnits, cap, 1, "creo matching row prefix scan"));
        }
        let original = ctx.resource_refusal().expect("original refusal");
        assert!(matches!(matching_row_id(&ctx, payload, close, id),
            Err(CodecError::ResourceLimit(actual)) if actual == original));
    }
}

#[test]
fn matching_row_prefix_admits_only_existing_candidates_within_eight_bytes() {
    for length in [0, 1, 2, 7, 8, 9, 17] {
        let payload = vec![0xff; length];
        check(&payload, length, 7, length.min(8) as u64, false);
        check(&payload, length + 1, 7, 0, false);
    }
}

#[test]
fn matching_row_prefix_preserves_canonical_identity_and_stops_at_first_match() {
    check(b"\x07\xe3", 1, 7, 1, true);
    check(b"\x07\xf7\x08\xe3", 3, 7, 3, true);
    check(b"\x80\x80\xe3", 2, 128, 2, true);
    check(b"\x80\x07\xe3", 2, 128, 2, false);
    check(b"\x07\xf7\x08\xff\xe3", 4, 7, 4, false);
    check(b"\xff\xff\xff\x07\xe3", 4, 7, 1, true);
}

fn check_candidates<T: std::fmt::Debug + PartialEq>(
    visits: u64,
    label: &'static str,
    expected: T,
    run: impl Fn(&DecodeContext<'_>) -> Result<T, CodecError>,
) {
    for cap in 0..=visits {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let result = run(&ctx);
        if cap == visits {
            assert_eq!(result.expect("exact candidate visits"), expected);
            let original = ctx.charge_work_limit(1, "after candidate search").expect_err("exact work");
            assert_eq!((original.used, original.additional), (visits, 1));
        } else {
            let original = ctx.resource_refusal().expect("candidate visit refuses");
            assert!(matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == original));
            assert_eq!((original.dimension, original.used, original.additional, original.operation),
                (ResourceDimension::WorkUnits, cap, 1, label));
        }
        let original = ctx.resource_refusal().expect("original refusal");
        assert!(matches!(run(&ctx), Err(CodecError::ResourceLimit(actual)) if actual == original));
    }
}

#[test]
fn scalar_suffix_admits_actual_candidates_and_stops_at_ambiguity() {
    let cache = crate::scalar::ScalarCache::default();
    for length in [0, 1, 9, 54, 55, 129] {
        let body = vec![0xff; length];
        check_candidates(length.min(6 * 9) as u64, "creo reference scalar suffix scan", None,
            |ctx| super::scalar_suffix::<6>(ctx, &body, &cache));
    }
    check_candidates(6, "creo reference scalar suffix scan", Some([0.0; 6]),
        |ctx| super::scalar_suffix::<6>(ctx, &[0x0f; 6], &cache));
    // Both starts encode six scalars ending at byte13; ambiguity stops after start1.
    let body = [0x46, 0x2c, 0, 0, 0, 0, 0, 0, 0xe4, 0xe4, 0xe4, 0xe4, 0xe4];
    check_candidates(2, "creo reference scalar suffix scan", None,
        |ctx| super::scalar_suffix::<6>(ctx, &body, &cache));
}

#[test]
fn conic_frame_end_scan_admits_only_present_bounded_frames() {
    let cache = crate::scalar::ScalarCache::default();
    for length in [0, 1, 12, 108, 109, 200] {
        let body = vec![0xff; length];
        check_candidates(length.min(12 * 9) as u64, "creo conic frame end scan", None,
            |ctx| super::positional_conic_local_system(ctx, &body, 0, &cache));
    }
    check_candidates(0, "creo conic frame end scan", None,
        |ctx| super::positional_conic_local_system(ctx, &[], usize::MAX, &cache));
    let mut body = vec![0x0f; 12];
    body.extend([0xe2, 0xff]);
    check_candidates(14, "creo conic frame end scan", Some((12, [0.0; 12])),
        |ctx| super::positional_conic_local_system(ctx, &body, 0, &cache)
            .map(|frame| frame.map(|(end, values)| (end, values.get()))));
}
