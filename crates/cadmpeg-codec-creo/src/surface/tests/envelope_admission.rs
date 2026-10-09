// SPDX-License-Identifier: Apache-2.0

use crate::scalar::ScalarCache;
use crate::surface::{decode_inline_selector_cylinder_envelope,
    plane_envelope_has_one_held_coordinate, SurfaceKind};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn selector_envelope_fixed_axes_are_free_and_preserve_original_refusal() {
    for (placeholders, accepted) in [
        (&[][..], true), (&[5][..], true), (&[1][..], false), (&[3, 5][..], false),
    ] {
        let mut body = vec![0x12];
        for (index, value) in [4.0_f64, -4.0, 2.0, 4.0, 2.0, -2.0, -4.0, -2.0]
            .into_iter().enumerate()
        {
            if index == 1 { body.push(0x14); }
            if index >= 2 && placeholders.contains(&(index - 2)) {
                body.extend_from_slice(&[0xda, 0, 0, 0, 0, 0, 1]);
            } else {
                let raw = value.to_be_bytes();
                assert!(matches!(raw[0], 0x40 | 0xc0));
                body.push(if value > 0.0 { 0x2d } else { 0x46 });
                body.extend_from_slice(&raw[1..]);
            }
        }
        body.push(0xe3);
        for kind in [SurfaceKind::Cylinder, SurfaceKind::Plane] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = 0;
            policy.limits.max_materialized_bytes = 0;
            policy.limits.max_retained_bytes = 0;
            policy.limits.max_collection_items = 0;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let run = || decode_inline_selector_cylinder_envelope(&ctx, kind, &body,
                &ScalarCache::default());
            let envelope = run().expect("fixed envelope needs no resources");
            if kind == SurfaceKind::Cylinder && accepted {
                let envelope = envelope.expect("at most one absent radial span");
                assert_eq!(envelope.axial, [-4.0, 4.0]);
                assert_eq!(envelope.corners[0], [Some(-2.0), Some(-4.0), Some(-2.0)]);
                assert_eq!(envelope.corners[1], [Some(2.0), Some(4.0),
                    if placeholders.is_empty() { Some(2.0) } else { None }]);
                assert_eq!(envelope.close, body.len() - 1);
            } else {
                assert!(envelope.is_none());
            }
            let original = ctx.charge_work_limit(1, "after fixed selector envelope")
                .expect_err("zero Work cap");
            assert_eq!((original.dimension, original.used, original.additional),
                (ResourceDimension::WorkUnits, 0, 1));
            assert!(matches!(run(), Err(CodecError::ResourceLimit(actual)) if actual == original));
            assert_eq!(ctx.resource_refusal(), Some(original));
            assert!(matches!(decode_inline_selector_cylinder_envelope(&ctx, kind, &[],
                &ScalarCache::default()), Err(CodecError::ResourceLimit(actual)) if actual == original));
        }
    }
}

#[test]
fn plane_envelope_fixed_pairs_admit_only_actual_byte_comparisons() {
    const PAIRS: [[usize; 2]; 3] = [[0, 3], [1, 4], [2, 5]];
    for (bytes, total, expected) in [
        ([b"a".as_slice(), b"b", b"d", b"a", b"c", b"e"], 3, true),
        ([b"a".as_slice(), b"bc", b"", b"a", b"bc", b""], 3, false),
        ([b"abcdefg".as_slice(), b"abc", b"x", b"abcdefg", b"abd", b"y"], 11, true),
        ([b"".as_slice(); 6], 0, false),
    ] {
        let slots = bytes.map(|bytes| (None, bytes));
        // Equal-length pairs pay once per byte through the first mismatch.
        // The three fixed pair selections and equality count add no work.
        for cap in 0..=total {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            policy.limits.max_materialized_bytes = 0;
            policy.limits.max_retained_bytes = 0;
            policy.limits.max_collection_items = 0;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let run = || plane_envelope_has_one_held_coordinate(&ctx, &slots, PAIRS);
            let result = run();
            if cap == total {
                assert_eq!(result.expect("exact compared-byte cap"), expected);
                let original = ctx.charge_work_limit(1, "after held-coordinate comparisons")
                    .expect_err("exact cap");
                assert_eq!((original.used, original.additional), (total, 1));
            } else {
                let original = ctx.resource_refusal().expect("actual byte pair refuses");
                assert!(matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == original));
                assert_eq!((original.dimension, original.used, original.additional, original.operation),
                    (ResourceDimension::WorkUnits, cap, 1, "creo plane envelope held coordinate bytes"));
            }
            let original = ctx.resource_refusal().expect("original refusal");
            assert!(matches!(run(), Err(CodecError::ResourceLimit(actual)) if actual == original));
            assert!(matches!(plane_envelope_has_one_held_coordinate(&ctx, &[], PAIRS),
                Err(CodecError::ResourceLimit(actual)) if actual == original));
            assert_eq!(ctx.resource_refusal(), Some(original));
        }
    }
}
