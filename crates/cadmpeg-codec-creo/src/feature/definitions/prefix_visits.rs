// SPDX-License-Identifier: Apache-2.0

use super::s2d_replay_starts;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn check(payload: &[u8], visits: usize, expected: &[usize]) {
    // Every four-byte marker window is admitted once, followed by actual name bytes.
    let windows = payload.len().saturating_sub(3) as u64;
    let total = windows + visits as u64;
    for cap in 0..=total {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        if expected.is_empty() {
            policy.limits.max_materialized_bytes = 0;
            policy.limits.max_retained_bytes = 0;
            policy.limits.max_collection_items = 0;
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let result = s2d_replay_starts(&ctx, payload);
        if cap == total {
            assert_eq!(result.expect("exact marker and name visits"), expected);
            let original = ctx.charge_work_limit(1, "after replay prefix")
                .expect_err("all executed work is accounted");
            assert_eq!((original.used, original.additional), (total, 1));
        } else {
            let original = ctx.resource_refusal().expect("actual work refuses");
            assert!(matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == original));
            let expected = if cap < windows {
                (0, windows, "creo replay marker traversal")
            } else {
                (cap, 1, "creo replay name prefix scan")
            };
            assert_eq!(original.dimension, ResourceDimension::WorkUnits);
            assert_eq!((original.used, original.additional, original.operation), expected);
        }
        let original = ctx.resource_refusal().expect("original refusal");
        assert!(matches!(s2d_replay_starts(&ctx, payload),
            Err(CodecError::ResourceLimit(actual)) if actual == original));
    }
}

#[test]
fn replay_prefix_admits_present_digits_through_terminator_or_invalid_byte() {
    for length in [0, 1, 2, 11, 12, 17] {
        let mut payload = b"\xe3S2D".to_vec();
        payload.extend(std::iter::repeat_n(b'7', length));
        check(&payload, length.min(12), &[]);
        payload.push(0);
        check(&payload, (length + 1).min(12),
            if (1..12).contains(&length) { &[0] } else { &[] });
    }
    check(b"\xe3S2DXignored\0", 1, &[]);
    check(b"\xe3S2D12Xignored\0", 3, &[]);
    check(b"", 0, &[]);
    check(b"\xe3S2", 0, &[]);
}

#[test]
fn replay_prefix_preserves_absolute_marker_offsets_and_the_twelve_byte_bound() {
    check(b"junk\xe3S2D123\0ignored", 4, &[4]);
    check(b"\xe3S2D1\0\xe3S2D22\0", 5, &[0, 6]);
    check(b"\xe3S2D12345678901\0", 12, &[0]);
    check(b"\xe3S2D123456789012\0", 12, &[]);
}
