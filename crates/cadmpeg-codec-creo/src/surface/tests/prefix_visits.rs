// SPDX-License-Identifier: Apache-2.0

use super::super::{named_record_boundary, named_record_length, SurfaceKind};
use crate::scalar::ScalarCache;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn check_name(payload: &[u8], offset: usize, visits: u64, expected: Option<usize>) {
    for cap in 0..=visits {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let result = named_record_length(&ctx, payload, offset);
        if cap == visits {
            assert_eq!(result.expect("actual field name visits"), expected);
            let original = ctx.charge_work_limit(1, "after surface name prefix")
                .expect_err("only executed work is charged");
            assert_eq!((original.used, original.additional), (visits, 1));
        } else {
            let original = ctx.resource_refusal().expect("name visit refuses");
            assert!(matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == original));
            assert_eq!((original.dimension, original.used, original.additional, original.operation),
                (ResourceDimension::WorkUnits, cap, 1, "creo surface name prefix scan"));
        }
        let original = ctx.resource_refusal().expect("original refusal");
        assert!(matches!(named_record_length(&ctx, payload, offset),
            Err(CodecError::ResourceLimit(actual)) if actual == original));
    }
}

#[test]
fn surface_name_prefix_admits_present_grammar_bytes_with_the_original_bound() {
    for length in [0, 1, 2, 17, 95, 96, 120] {
        let mut payload = b"\xe0\x24".to_vec();
        payload.extend(std::iter::repeat_n(b'A', length));
        check_name(&payload, 0, length.min(96) as u64, None);
        payload.push(0);
        check_name(&payload, 0, (length + 1).min(96) as u64,
            (1..96).contains(&length).then_some(length + 3));
    }
    check_name(b"\xe0\0_ignored\0", 0, 1, None);
    check_name(b"\xe0\0A-invalid\0", 0, 2, None);
    check_name(b"\xe0\0A_2(x)\0ignored", 0, 7, Some(9));
    check_name(b"\xe0\x25A\0", 0, 0, None);
    check_name(b"\xe0", 0, 0, None);
    check_name(b"", usize::MAX, 0, None);
}

#[test]
fn surface_named_boundary_admits_visited_positions_and_skips_scalar_interiors() {
    let cache = ScalarCache::from_section(&[]);
    for (payload, offset, positions) in [
        (b"".as_slice(), None, 0),
        (b"\xff\xff\xff".as_slice(), None, 3),
        (b"\xe0\0A\0ignored".as_slice(), Some(0), 1),
        (b"\xff\xff\xe0\0A\0ignored".as_slice(), Some(2), 3),
        // One eight-byte scalar, followed by the named field. Its interior is not visited.
        (b"\x46\0\0\0\0\0\0\0\xe0\0A\0ignored".as_slice(), Some(8), 2),
    ] {
        let total = positions + if offset.is_some() { 2 } else { 0 };
        for cap in 0..=total {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            policy.limits.max_materialized_bytes = 0;
            policy.limits.max_retained_bytes = 0;
            policy.limits.max_collection_items = 0;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let result = named_record_boundary(&ctx, SurfaceKind::Plane, payload, &cache);
            if cap == total {
                assert_eq!(result.expect("exact position and name visits"), offset);
                let original = ctx.charge_work_limit(1, "after surface named boundary")
                    .expect_err("unvisited scalar bytes and trailing bytes are free");
                assert_eq!((original.used, original.additional), (total, 1));
            } else {
                let original = ctx.resource_refusal().expect("actual visit refuses");
                assert!(matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == original));
                let operation = if cap < positions { "creo surface named boundary scan" }
                    else { "creo surface name prefix scan" };
                assert_eq!((original.dimension, original.used, original.additional, original.operation),
                    (ResourceDimension::WorkUnits, cap, 1, operation));
            }
            let original = ctx.resource_refusal().expect("original refusal");
            assert!(matches!(named_record_boundary(&ctx, SurfaceKind::Plane, payload, &cache),
                Err(CodecError::ResourceLimit(actual)) if actual == original));
        }
    }
}
