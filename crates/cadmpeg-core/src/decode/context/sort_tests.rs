// SPDX-License-Identifier: Apache-2.0

use super::DecodeContext;
use crate::decode::{DecodeArena, DecodePolicy, ResourceDimension};
use crate::CodecError;

#[test]
fn charged_stable_sort_preserves_duplicate_order_and_permutation_cycles() {
    for count in 0..96 {
        let mut values: Vec<_> = (0..count)
            .map(|index| ((count - index) % 7, index))
            .collect();
        let mut expected = values.clone();
        expected.sort_by_key(|value| value.0);
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("admitted test operation");
        ctx.stable_sort_by(&mut values, |value| &value.0, Ord::cmp, "test stable sort")
            .expect("admitted test operation");
        assert_eq!(values, expected);
    }
}

#[test]
fn charged_stable_sort_scoped_refusal_preserves_input() {
    let mut values: Vec<_> = (0..32).rev().collect();
    let expected = values.clone();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes =
        3 * 32 * u64::try_from(std::mem::size_of::<usize>()).expect("admitted test operation") - 1;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("admitted test operation");
    assert!(matches!(ctx.stable_sort_by(&mut values,
            |value| value,
            Ord::cmp, "test stable sort"), Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::MaterializedBytes));
    assert_eq!(values, expected);
    policy.limits.max_materialized_bytes += 1;
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("admitted test operation");
    ctx.stable_sort_by(&mut values, |value| value, Ord::cmp, "test stable sort")
        .expect("admitted test operation");
    assert_eq!(values, (0..32).collect::<Vec<_>>());
}

/// The smallest work limit under which the stable sort of `values` succeeds.
fn stable_sort_work(values: &[u64]) -> u64 {
    let fits = |limit| {
        let mut values = values.to_vec();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        ctx.stable_sort_by(&mut values, |value| value, Ord::cmp, "ordered sort")
            .is_ok()
    };
    let (mut low, mut high) = (0, 1 << 20);
    assert!(fits(high));
    while low < high {
        let middle = low + (high - low) / 2;
        if fits(middle) {
            high = middle;
        } else {
            low = middle + 1;
        }
    }
    low
}

#[test]
fn stable_sort_charges_depend_only_on_length() {
    for count in [12_u64, 20, 21, 40] {
        let sorted: Vec<_> = (0..count).collect();
        let reversed: Vec<_> = (0..count).rev().collect();
        let shuffled: Vec<_> = (0..count).map(|index| index * 7 % count).collect();
        let interleaved: Vec<_> = (0..count).map(|index| index % 3).collect();
        let work = stable_sort_work(&sorted);
        assert_eq!(stable_sort_work(&reversed), work, "{count} reversed");
        assert_eq!(stable_sort_work(&shuffled), work, "{count} shuffled");
        assert_eq!(stable_sort_work(&interleaved), work, "{count} interleaved");
    }
}

#[test]
fn charged_stable_sort_work_refusal_preserves_input() {
    let mut values = vec![("long compared key", 1), ("different key", 2)];
    let expected = values.clone();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("admitted test operation");
    assert!(matches!(ctx.stable_sort_by(&mut values,
            |value| &value.0,
            Ord::cmp, "test stable sort"), Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::WorkUnits));
    assert_eq!(values, expected);
}

#[test]
fn copied_sort_refuses_before_key_extraction_and_keeps_original_refusal() {
    for stable in [false, true] {
        let mut values = [2_u64, 1];
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let visits = std::cell::Cell::new(0);
        let key = |value: &u64| {
            visits.set(visits.get() + 1);
            *value
        };
        let result = if stable {
            ctx.stable_sort_by_key(&mut values, key, Ord::cmp, "copied sort")
        } else {
            ctx.sort_unstable_by_key(&mut values, key, Ord::cmp, "copied sort")
        };
        let CodecError::ResourceLimit(limit) = result.expect_err("refusal") else {
            panic!("resource refusal")
        };
        assert_eq!(visits.get(), 0);
        assert_eq!(values, [2, 1]);
        assert_eq!(ctx.resource_refusal(), Some(limit));
        let CodecError::ResourceLimit(repeated) = ctx.charge_work(0, "repeat").expect_err("fused")
        else {
            panic!("resource refusal")
        };
        assert_eq!(limit, repeated);
    }
}

#[test]
fn copied_stable_sort_orders_external_identity_keys_without_copying_payloads() {
    let source = ["z", "a", "a", "b"];
    let mut values: Vec<_> = (0..32)
        .map(|index| (index % 4, index, "payload".to_owned()))
        .collect();
    let mut expected = values.clone();
    expected.sort_by_key(|value| source[value.0]);
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
    ctx.stable_sort_by_key(
        &mut values,
        |value| source[value.0],
        Ord::cmp,
        "external identities",
    )
    .expect("admitted sort");
    assert_eq!(values, expected);
}

#[test]
fn borrowed_sort_admits_nested_key_bytes_before_the_first_comparison() {
    for stable in [false, true] {
        let mut values = vec![vec!["second".to_owned()], vec!["first".to_owned()]];
        let expected = values.clone();
        let comparisons = std::cell::Cell::new(0);
        let compare = |left: &Vec<String>, right: &Vec<String>| {
            comparisons.set(comparisons.get() + 1);
            left.cmp(right)
        };
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // Four outer measuring visits and two child measuring visits.
        policy.limits.max_work_units = 6;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let result = if stable {
            ctx.stable_sort_by(&mut values, |value| value, compare, "nested sort")
        } else {
            ctx.sort_unstable_by(&mut values, |value| value, compare, "nested sort")
        };
        assert!(
            matches!(result, Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::WorkUnits)
        );
        assert_eq!(comparisons.get(), 0);
        assert_eq!(values, expected);
    }
}
