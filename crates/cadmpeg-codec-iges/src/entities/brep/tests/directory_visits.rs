// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use std::collections::BTreeMap;

#[test]
fn brep_directory_does_not_admit_an_unvisited_tail_before_its_first_loss_refusal() {
    let mut vertex = crate::test_support::directory_target(1, 502);
    vertex.form = 1;
    let directory = [
        vertex,
        crate::test_support::directory_target(3, 110),
        crate::test_support::directory_target(5, 110),
    ];
    let entries: BTreeMap<_, _> = directory.iter().map(|entry| (entry.sequence, entry)).collect();
    let records = BTreeMap::new();
    let bytes = crate::test_support::test_owned::owned_test_file(&[]);
    let global = crate::test_support::with_service_context(&bytes, |setup| {
        let scan = crate::card::scan_with_context(&bytes, setup).unwrap();
        let (global, _, _) = crate::global::parse(&scan, setup).unwrap();
        global.length_context().unwrap()
    });
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // One original Directory step. The empty parameter map has no key comparisons.
    policy.limits.max_work_units = 1;
    policy.limits.max_collection_items = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut ir = cadmpeg_ir::CadIr::empty();
    let mut sequences = super::super::super::geometry::SourceSequences::default();
    let result = super::super::project(
        &mut ir, &directory, (&entries, &records), &global, &ctx, &mut sequences,
    );
    let first = match result.as_ref() {
        Err(CodecError::ResourceLimit(first)) => *first,
        _ => panic!("expected first visited entity loss-slot refusal"),
    };
    drop(result);
    assert_eq!(first.dimension, ResourceDimension::CollectionItems);
    assert_eq!(first.operation, "iges entity loss slots");
    assert_eq!((first.limit, first.used, first.additional), (0, 0, 1));
    assert!(ir.model.bodies.is_empty());
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
}

#[test]
fn later_brep_directory_passes_refuse_before_their_unvisited_tail() {
    let bytes = crate::test_support::test_owned::owned_test_file(&[]);
    let global = crate::test_support::with_service_context(&bytes, |setup| {
        let scan = crate::card::scan_with_context(&bytes, setup).unwrap();
        let (global, _, _) = crate::global::parse(&scan, setup).unwrap();
        global.length_context().unwrap()
    });
    for (entity_type, form, preceding_passes) in [
        (504, 1, 1_u64), (508, 1, 2), (510, 1, 3), (514, 1, 4), (186, 0, 6),
    ] {
        let mut entry = crate::test_support::directory_target(1, entity_type);
        entry.form = form;
        let directory = [entry,
            crate::test_support::directory_target(3, 110),
            crate::test_support::directory_target(5, 110)];
        let entries: BTreeMap<_, _> = directory.iter().map(|entry| (entry.sequence, entry)).collect();
        let records = BTreeMap::new();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // Each preceding pass visits all three skipped entries.
        // The selected pass visits its first entry; its empty map lookup costs zero.
        policy.limits.max_work_units = preceding_passes * 3 + 1;
        policy.limits.max_collection_items = 0;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut ir = cadmpeg_ir::CadIr::empty();
        let mut sequences = super::super::super::geometry::SourceSequences::default();
        let result = super::super::project(
            &mut ir, &directory, (&entries, &records), &global, &ctx, &mut sequences,
        );
        let first = match result.as_ref() {
            Err(CodecError::ResourceLimit(first)) => *first,
            _ => panic!("expected the visited family loss-slot refusal"),
        };
        drop(result);
        assert_eq!(first.dimension, ResourceDimension::CollectionItems);
        assert_eq!(first.operation, "iges entity loss slots");
        assert_eq!((first.limit, first.used, first.additional), (0, 0, 1));
        assert!(ir.model.bodies.is_empty());
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
    }
}

#[test]
fn skipped_brep_directory_entries_have_exact_work_without_empty_reads() {
    let bytes = crate::test_support::test_owned::owned_test_file(&[]);
    let global = crate::test_support::with_service_context(&bytes, |setup| {
        let scan = crate::card::scan_with_context(&bytes, setup).unwrap();
        let (global, _, _) = crate::global::parse(&scan, setup).unwrap();
        global.length_context().unwrap()
    });
    let skipped = [crate::test_support::directory_target(1, 110),
        crate::test_support::directory_target(3, 110),
        crate::test_support::directory_target(5, 110)];
    for count in [0, 1, 3] {
        let directory = &skipped[..count];
        let entries: BTreeMap<_, _> = directory.iter().map(|entry| (entry.sequence, entry)).collect();
        let records = BTreeMap::new();
        // Eight actual Directory passes, each with count admitted visits.
        // The slice length gate stops before an empty read.
        // The empty body traversal has a zero complete-source bound.
        let required = 8 * u64::try_from(count).unwrap();
        for cap in [required.saturating_sub(1), required] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            policy.limits.max_collection_items = 0;
            policy.limits.max_retained_bytes = 0;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut ir = cadmpeg_ir::CadIr::empty();
            let mut sequences = super::super::super::geometry::SourceSequences::default();
            let result = super::super::project(
                &mut ir, directory, (&entries, &records), &global, &ctx, &mut sequences,
            );
            if cap == required {
                let outcome = result.unwrap();
                assert!(outcome.decoded.is_empty());
                assert!(outcome.losses.is_empty());
                drop(outcome);
                ctx.finish_session().unwrap();
            } else {
                let first = match result.as_ref() {
                    Err(CodecError::ResourceLimit(first)) => *first,
                    _ => panic!("expected the last visited Directory entry refusal"),
                };
                drop(result);
                assert_eq!(first.dimension, ResourceDimension::WorkUnits);
                assert_eq!(first.operation, "iges B-rep directory traversal");
                assert_eq!((first.limit, first.used, first.additional), (cap, cap, 1));
                assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
            }
            assert!(ir.model.bodies.is_empty());
        }
    }
}

#[test]
fn empty_brep_directory_preserves_an_existing_sticky_refusal() {
    let bytes = crate::test_support::test_owned::owned_test_file(&[]);
    let global = crate::test_support::with_service_context(&bytes, |setup| {
        let scan = crate::card::scan_with_context(&bytes, setup).unwrap();
        let (global, _, _) = crate::global::parse(&scan, setup).unwrap();
        global.length_context().unwrap()
    });
    let directory = [crate::test_support::directory_target(1, 110)];
    let entries = BTreeMap::new();
    let records = BTreeMap::new();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut ir = cadmpeg_ir::CadIr::empty();
    let mut sequences = super::super::super::geometry::SourceSequences::default();
    let result = super::super::project(
        &mut ir, &directory, (&entries, &records), &global, &ctx, &mut sequences,
    );
    let first = match result.as_ref() {
        Err(CodecError::ResourceLimit(first)) => *first,
        _ => panic!("expected the first Directory source-step refusal"),
    };
    drop(result);
    assert_eq!(first.dimension, ResourceDimension::WorkUnits);
    assert_eq!(first.operation, "iges B-rep directory traversal");
    assert_eq!((first.limit, first.used, first.additional), (0, 0, 1));
    let result = super::super::project(
        &mut ir, &[], (&entries, &records), &global, &ctx, &mut sequences,
    );
    assert!(matches!(result.as_ref(), Err(CodecError::ResourceLimit(last)) if *last == first));
    drop(result);
    assert!(ir.model.bodies.is_empty());
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
}
