// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

use crate::scalar::ScalarCache;
use crate::surface::plane_envelope_compound_close;

#[test]
fn plane_envelope_close_bound_preserves_minimal_compact_and_maximum_width_frames() {
    for (compact, width) in [(false, 1), (true, 1), (false, 8), (true, 8)] {
        let mut body = Vec::new();
        if compact {
            body.push(0x0e);
        }
        if width == 1 {
            let standard = [0x0f, 0xe4, 0x0d, 0x0f, 0x0f, 0x0f, 0xe4, 0x0d, 0x0f];
            body.extend_from_slice(if compact { &standard[1..] } else { &standard });
        } else {
            // Each head46 scalar consumes eight bytes. Only the first corner
            // coordinate agrees; the second differs in payload byte one and
            // the third has a different token width from the terminal DICT.
            for value in if compact {
                &[2.0_f64; 8][..]
            } else {
                &[2.0_f64; 9][..]
            } {
                let mut raw = value.to_be_bytes();
                raw[0] = 0x46;
                body.extend_from_slice(&raw);
            }
            let second_corner_y = if compact { 1 + 7 * 8 } else { 8 * 8 };
            body[second_corner_y + 1] = 0x10; // IEEE 4010... states 4.0.
        }
        body.extend_from_slice(&[0x71, 0, 0, 0, 0, 0, 0]);
        let close = body.len();
        assert_eq!(
            close,
            if width == 1 {
                16
            } else if compact {
                72
            } else {
                79
            }
        );
        body.push(0xe3);
        for suffix in [0, 1, 79, 80, 4096] {
            let mut extended = body.clone();
            extended.extend(std::iter::repeat_n(0xe3, suffix));
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = 0;
            policy.limits.max_materialized_bytes = 0;
            policy.limits.max_retained_bytes = 0;
            policy.limits.max_collection_items = 0;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let actual = plane_envelope_compound_close(&ctx, &extended, &ScalarCache::default())
                .expect("bounded envelope uses fixed storage");
            assert_eq!(actual, Some(close));
        }
    }
}

#[test]
fn impossible_plane_envelope_closes_are_free() {
    for size in [0, 1, 7, 79, 80, 81, 4096] {
        let mut body = vec![0xed; size];
        if size >= 80 {
            // Later closes cannot own the nine-scalar prefix and final DICT.
            body[size - 1] = 0xe3;
        }
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 0;
        policy.limits.max_entities = 0;
        policy.limits.max_recursion_depth = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let run = || plane_envelope_compound_close(&ctx, &body, &ScalarCache::default());
        assert!(run().expect("fixed candidate scan is free").is_none());
    }
}

#[test]
fn rejected_plane_envelope_candidates_use_fixed_storage() {
    // Two equal coordinate pairs cannot select an axis-aligned envelope.
    let mut body = vec![0x0f; 9];
    body.extend_from_slice(&[0x71, 0, 0, 0, 0, 0, 0, 0xe3]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    for _ in 0..7 {
        assert_eq!(
            plane_envelope_compound_close(&ctx, &body, &ScalarCache::default())
                .expect("rejected bounded candidates use no heap storage"),
            None
        );
    }
}
