// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use std::collections::{BTreeMap, BTreeSet};
use std::mem::{align_of, size_of};

#[test]
fn dimension_last_segment_reaches_axis_recovery_without_an_empty_visit() {
    let mut definition = super::incomplete_segment_definition();
    definition.relations = None;
    let segment = definition
        .segments
        .take()
        .expect("segments")
        .rows
        .ordinary()
        .next()
        .expect("line")
        .clone();
    // An empty parity map admits three node passes before its first slot.
    let alignment = align_of::<u32>()
        .max(align_of::<bool>())
        .max(align_of::<usize>());
    let node_bytes =
        11 * (size_of::<u32>() + size_of::<bool>()) + 16 * size_of::<usize>() + 2 * alignment;
    let seed_work = u64::try_from(3 * node_bytes).expect("parity seed node bound");
    let cap = crate::test_support::allocation_limit_at(
        ResourceDimension::WorkUnits,
        Some("creo fixed-coordinate parity seed"),
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            policy.limits.max_collection_items = 0;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let result = super::super::section_linear_distance_coordinate(
                &ctx,
                &definition,
                &[&segment],
                [1, 2],
                &BTreeMap::new(),
                &[],
                &BTreeSet::new(),
            );
            if let Err(CodecError::ResourceLimit(original)) = &result {
                assert_eq!(original.dimension, ResourceDimension::WorkUnits);
                match original.operation {
                    "creo dimension segment search" => {
                        assert_eq!((original.used, original.additional), (0, 1));
                    }
                    "creo fixed-coordinate parity seed" => {
                        assert_eq!(original.used, 1, "only the source row precedes recovery");
                        assert_eq!(original.additional, seed_work);
                    }
                    operation => panic!("unexpected axis recovery operation: {operation}"),
                }
            }
            result
        },
    );
    assert_eq!(
        cap, seed_work,
        "one unit below the source row plus parity seed"
    );
}

#[test]
fn dimension_duplicate_segment_is_admitted_before_rejection() {
    let definition = super::incomplete_segment_definition();
    let segment = definition
        .segments
        .as_ref()
        .expect("segments")
        .rows
        .ordinary()
        .next()
        .expect("line");
    let value = crate::test_support::assert_refusal_order(
        ResourceDimension::WorkUnits,
        &[
            "creo dimension segment search",
            "creo dimension segment uniqueness",
        ],
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            policy.limits.max_retained_bytes = 0;
            policy.limits.max_materialized_bytes = 0;
            policy.limits.max_collection_items = 0;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let result = super::super::section_linear_distance_coordinate(
                &ctx,
                &definition,
                &[segment, segment],
                [1, 2],
                &BTreeMap::new(),
                &[],
                &BTreeSet::new(),
            );
            let original = match &result {
                Err(CodecError::ResourceLimit(original)) => *original,
                Ok(value) => {
                    assert_eq!(*value, None);
                    let original = ctx
                        .charge_work_limit(1, "after duplicate dimension")
                        .expect_err("exact work");
                    assert_eq!((original.used, original.additional), (2, 1));
                    original
                }
                Err(error) => panic!("unexpected error: {error:?}"),
            };
            assert_eq!(
                (
                    original.dimension,
                    original.used,
                    original.additional,
                    original.limit
                ),
                (ResourceDimension::WorkUnits, cap, 1, cap)
            );
            result
        },
    );
    assert_eq!(value, None);
}
