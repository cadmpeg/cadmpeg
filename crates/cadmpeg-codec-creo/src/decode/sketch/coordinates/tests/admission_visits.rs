// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use std::collections::{BTreeMap, BTreeSet};
use std::mem::{align_of, size_of};

#[test]
fn dimension_last_segment_reaches_axis_recovery_without_an_empty_visit() {
    let mut definition = super::incomplete_segment_definition();
    definition.relations = None;
    let segment = definition.segments.take().expect("segments").rows.ordinary().next().expect("line").clone();
    // The empty parity map admits three node passes before its first slot.
    let alignment = align_of::<u32>().max(align_of::<bool>()).max(align_of::<usize>());
    let node_bytes = 11 * (size_of::<u32>() + size_of::<bool>())
        + 16 * size_of::<usize>() + 2 * alignment;
    let seed_work = u64::try_from(3 * node_bytes).expect("parity seed node bound");
    for cap in 0..=1 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let run = || super::super::section_linear_distance_coordinate(&ctx, &definition,
            &[&segment], [1, 2], &BTreeMap::new(), &[], &BTreeSet::new());
        let Err(CodecError::ResourceLimit(original)) = run() else { panic!("source visit or parity seed must refuse"); };
        if cap == 0 {
            assert_eq!((original.dimension, original.used, original.additional, original.operation),
                (ResourceDimension::WorkUnits, 0, 1, "creo dimension segment search"));
        } else {
            // The single row already proves uniqueness. Recovery proceeds
            // directly to its first parity node instead of an empty suffix.
            assert_eq!((original.dimension, original.used, original.additional, original.operation),
                (ResourceDimension::WorkUnits, 1, seed_work, "creo fixed-coordinate parity seed"));
        }
        for _ in 0..2 {
            assert!(matches!(run(), Err(CodecError::ResourceLimit(actual)) if actual == original));
        }
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == original));
    }
}

#[test]
fn dimension_duplicate_segment_is_admitted_before_rejection() {
    let definition = super::incomplete_segment_definition();
    let segment = definition.segments.as_ref().expect("segments").rows.ordinary().next().expect("line");
    // One first-match visit and one actual duplicate visit; no axis recovery.
    for cap in 0..=2 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let run = || super::super::section_linear_distance_coordinate(&ctx, &definition,
            &[segment, segment], [1, 2], &BTreeMap::new(), &[], &BTreeSet::new());
        let original = if cap == 2 {
            assert_eq!(run().expect("two present rows admitted"), None);
            ctx.charge_work_limit(1, "after duplicate dimension").expect_err("exact work")
        } else {
            let Err(CodecError::ResourceLimit(original)) = run() else { panic!("present row must refuse"); };
            assert_eq!(original.operation, if cap == 0 {
                "creo dimension segment search"
            } else { "creo dimension segment uniqueness" });
            original
        };
        assert_eq!((original.dimension, original.used, original.additional, original.limit),
            (ResourceDimension::WorkUnits, cap, 1, cap));
        for _ in 0..2 {
            assert!(matches!(run(), Err(CodecError::ResourceLimit(actual)) if actual == original));
        }
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == original));
    }
}
