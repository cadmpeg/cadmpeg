// SPDX-License-Identifier: Apache-2.0
use super::sort_by;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn stable_sort_permutation_refuses_collection_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 32;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut values = [0; 33];
    assert!(matches!(sort_by(Some(&ctx), &mut values, Ord::cmp),
        Err(CodecError::ResourceLimit(failure)) if failure.dimension == ResourceDimension::CollectionItems
            && failure.operation == "f3d stable sort permutation"));
}

#[test]
fn stable_sort_scratch_refuses_materialized_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_materialized_bytes =
        u64::try_from(33 * std::mem::size_of::<usize>() - 1).unwrap();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut values = [0; 33];
    assert!(matches!(sort_by(Some(&ctx), &mut values, Ord::cmp),
        Err(CodecError::ResourceLimit(failure)) if failure.dimension == ResourceDimension::MaterializedBytes
            && failure.operation == "f3d stable sort scratch"));
}

#[test]
fn stable_sort_refuses_work_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_work_units = 33 * 6 - 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut values = [0; 33];
    assert!(matches!(sort_by(Some(&ctx), &mut values, Ord::cmp),
        Err(CodecError::ResourceLimit(failure)) if failure.dimension == ResourceDimension::WorkUnits
            && failure.operation == "f3d stable sort work"));
}

#[test]
fn stable_sort_matches_equal_key_order_without_cloning() {
    struct Item {
        key: usize,
        ordinal: usize,
    }
    for count in 0..=160 {
        let mut values: Vec<_> = (0..count)
            .map(|ordinal| Item {
                key: (ordinal * 13 + 7) % 9,
                ordinal,
            })
            .collect();
        let mut expected: Vec<_> = values.iter().map(|item| (item.key, item.ordinal)).collect();
        expected.sort_by_key(|(key, _)| *key);
        crate::design::test_support::with_test_decode_context(|ctx| {
            sort_by(Some(ctx), &mut values, |left, right| {
                left.key.cmp(&right.key)
            })
        })
        .unwrap();
        assert_eq!(
            values
                .iter()
                .map(|item| (item.key, item.ordinal))
                .collect::<Vec<_>>(),
            expected
        );
    }
}

#[test]
fn stable_key_sort_refuses_collection_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 20;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut values = [0; 21];
    assert!(
        matches!(super::sort_by_key(Some(&ctx), &mut values, |value| *value),
        Err(CodecError::ResourceLimit(failure)) if failure.dimension == ResourceDimension::CollectionItems
            && failure.operation == "f3d stable sort permutation")
    );
}
