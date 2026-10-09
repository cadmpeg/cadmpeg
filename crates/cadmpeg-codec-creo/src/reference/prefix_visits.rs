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
