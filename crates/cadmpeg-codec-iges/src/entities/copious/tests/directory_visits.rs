// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use std::collections::BTreeMap;

fn global() -> crate::global::ProjectedGlobal {
    let bytes = crate::test_support::test_owned::owned_test_file(&[]);
    crate::test_support::with_service_context(&bytes, |setup| {
        let scan = crate::card::scan_with_context(&bytes, setup).unwrap();
        let (global, _, _) = crate::global::parse(&scan, setup).unwrap();
        global.length_context().unwrap()
    })
}

#[test]
fn empty_copious_directory_preserves_the_original_sticky_refusal() {
    let global = global();
    let directory = [crate::test_support::directory_target(1, 110)];
    let entries = BTreeMap::new();
    let records = BTreeMap::new();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut ir = cadmpeg_ir::CadIr::empty();
    let mut sequences = super::super::super::geometry::SourceSequences::default();
    let outcome = super::super::project(
        &mut ir, &[], &entries, &records, &global, &ctx, &mut sequences,
    ).unwrap();
    assert!(outcome.decoded.is_empty());
    assert!(outcome.losses.is_empty());
    assert!(outcome.wire_edges.is_empty());
    assert!(outcome.free_vertices.is_empty());
    drop(outcome);
    let result = super::super::project(
        &mut ir, &directory, &entries, &records, &global, &ctx, &mut sequences,
    );
    let first = match result.as_ref() {
        Err(CodecError::ResourceLimit(first)) => *first,
        _ => panic!("expected the original source-step refusal"),
    };
    drop(result);
    assert_eq!(first.dimension, ResourceDimension::WorkUnits);
    assert_eq!(first.operation, "iges copious directory traversal");
    assert_eq!((first.limit, first.used, first.additional), (0, 0, 1));
    let result = super::super::project(
        &mut ir, &[], &entries, &records, &global, &ctx, &mut sequences,
    );
    assert!(matches!(result.as_ref(), Err(CodecError::ResourceLimit(last)) if *last == first));
    drop(result);
    assert!(ir.model.bodies.is_empty());
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
}

#[test]
fn skipped_copious_directory_entries_have_exact_work_without_empty_reads() {
    let global = global();
    let directory = [crate::test_support::directory_target(1, 110),
        crate::test_support::directory_target(3, 110),
        crate::test_support::directory_target(5, 110)];
    let entries = BTreeMap::new();
    let records = BTreeMap::new();
    for count in [1, 3] {
        let required = u64::try_from(count).unwrap();
        for cap in [required - 1, required] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            // One pass over the actual count; no payload or empty source read.
            policy.limits.max_work_units = cap;
            policy.limits.max_collection_items = 0;
            policy.limits.max_retained_bytes = 0;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut ir = cadmpeg_ir::CadIr::empty();
            let mut sequences = super::super::super::geometry::SourceSequences::default();
            let result = super::super::project(
                &mut ir, &directory[..count], &entries, &records, &global, &ctx, &mut sequences,
            );
            if cap == required {
                let outcome = result.unwrap();
                assert!(outcome.decoded.is_empty());
                assert!(outcome.losses.is_empty());
                assert!(outcome.wire_edges.is_empty());
                assert!(outcome.free_vertices.is_empty());
                drop(outcome);
                ctx.finish_session().unwrap();
            } else {
                let first = match result.as_ref() {
                    Err(CodecError::ResourceLimit(first)) => *first,
                    _ => panic!("expected the last actual source-step refusal"),
                };
                drop(result);
                assert_eq!(first.dimension, ResourceDimension::WorkUnits);
                assert_eq!(first.operation, "iges copious directory traversal");
                assert_eq!((first.limit, first.used, first.additional), (cap, cap, 1));
                assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
            }
            assert!(ir.model.bodies.is_empty());
        }
    }
}

#[test]
fn copious_directory_does_not_admit_an_unvisited_tail_before_a_loss_refusal() {
    let global = global();
    let mut entry = crate::test_support::directory_target(1, 106);
    entry.form = 11;
    let directory = [entry, crate::test_support::directory_target(3, 110),
        crate::test_support::directory_target(5, 110)];
    let entries = BTreeMap::new();
    let records = BTreeMap::new();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // One source step, then the original missing-parameter loss slot.
    policy.limits.max_work_units = 1;
    policy.limits.max_collection_items = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut ir = cadmpeg_ir::CadIr::empty();
    let mut sequences = super::super::super::geometry::SourceSequences::default();
    let result = super::super::project(
        &mut ir, &directory, &entries, &records, &global, &ctx, &mut sequences,
    );
    let first = match result.as_ref() {
        Err(CodecError::ResourceLimit(first)) => *first,
        _ => panic!("expected the visited entity loss-slot refusal"),
    };
    drop(result);
    assert_eq!(first.dimension, ResourceDimension::CollectionItems);
    assert_eq!(first.operation, "iges entity loss slots");
    assert_eq!((first.limit, first.used, first.additional), (0, 0, 1));
    assert!(ir.model.bodies.is_empty());
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
}
