// SPDX-License-Identifier: Apache-2.0

use super::matching_row_id;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;

fn check(payload: &[u8], close: usize, id: u32, expected: bool) {
    check_candidates(expected, |ctx| matching_row_id(ctx, payload, close, id));
}

#[test]
fn matching_row_prefix_admits_only_existing_candidates_within_eight_bytes() {
    for length in [0, 1, 2, 7, 8, 9, 17] {
        let payload = vec![0xff; length];
        check(&payload, length, 7, false);
        check(&payload, length + 1, 7, false);
    }
}

#[test]
fn matching_row_prefix_preserves_canonical_identity_and_stops_at_first_match() {
    check(b"\x07\xe3", 1, 7, true);
    check(b"\x07\xf7\x08\xe3", 3, 7, true);
    check(b"\x80\x80\xe3", 2, 128, true);
    check(b"\x80\x07\xe3", 2, 128, false);
    check(b"\x07\xf7\x08\xff\xe3", 4, 7, false);
    check(b"\xff\xff\xff\x07\xe3", 4, 7, true);
}

fn check_candidates<T: std::fmt::Debug + PartialEq>(
    expected: T,
    run: impl Fn(&DecodeContext<'_>) -> Result<T, CodecError>,
) {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    assert_eq!(run(&ctx).expect("bounded search uses no variable work"), expected);
}

#[test]
fn scalar_suffix_admits_actual_candidates_and_stops_at_ambiguity() {
    let cache = crate::scalar::ScalarCache::default();
    for length in [0, 1, 9, 54, 55, 129] {
        let body = vec![0xff; length];
        check_candidates(None,
            |ctx| super::scalar_suffix::<6>(ctx, &body, &cache));
    }
    check_candidates(Some([0.0; 6]),
        |ctx| super::scalar_suffix::<6>(ctx, &[0x0f; 6], &cache));
    // Both starts encode six scalars ending at byte13; ambiguity stops after start1.
    let body = [0x46, 0x2c, 0, 0, 0, 0, 0, 0, 0xe4, 0xe4, 0xe4, 0xe4, 0xe4];
    check_candidates(None,
        |ctx| super::scalar_suffix::<6>(ctx, &body, &cache));
}

#[test]
fn conic_frame_end_scan_admits_only_present_bounded_frames() {
    let cache = crate::scalar::ScalarCache::default();
    for length in [0, 1, 12, 108, 109, 200] {
        let body = vec![0xff; length];
        check_candidates(None,
            |ctx| super::positional_conic_local_system(ctx, &body, 0, &cache));
    }
    check_candidates(None,
        |ctx| super::positional_conic_local_system(ctx, &[], usize::MAX, &cache));
    let mut body = vec![0x0f; 12];
    body.extend([0xe2, 0xff]);
    check_candidates(Some((12, [0.0; 12])),
        |ctx| super::positional_conic_local_system(ctx, &body, 0, &cache)
            .map(|frame| frame.map(|(end, values)| (end, values.get()))));
}
