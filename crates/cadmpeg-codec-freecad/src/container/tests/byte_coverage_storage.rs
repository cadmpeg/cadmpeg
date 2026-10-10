// SPDX-License-Identifier: Apache-2.0
//! Scratch ownership of physical ordering and consumed logical groups.

use crate::container::byte_coverage;
use crate::native::{
    ArchiveSpan, ArchiveSpanRole, ByteSpan, EntryRecord, LogicalClassification, LogicalSpan,
};
use crate::test_support::{entry_record, with_service_context};
use cadmpeg_core::container::ContainerRole;
use cadmpeg_core::decode::{
    u64_from_index, DecodeArena, DecodeContext, DecodePolicy, ResourceDimension,
};
use cadmpeg_core::CodecError;
use std::mem::size_of;

fn physical(start: u64, end: u64) -> ArchiveSpan {
    ArchiveSpan {
        id: format!("fcstd:native:archive-span#{start}"),
        span: ByteSpan::try_new(start, end).unwrap(),
        role: ArchiveSpanRole::ArchivePadding,
    }
}

fn entry(name: &str, count: usize) -> EntryRecord {
    entry_record(
        format!("fcstd:native:entry#{name}"),
        name.to_owned(),
        ContainerRole::Auxiliary,
        Vec::new(),
        (0..count).map(|_| 0).collect(),
    )
}

fn logical(name: &str, start: u64, end: u64) -> LogicalSpan {
    LogicalSpan {
        id: format!("fcstd:native:logical-span#{name}:{start}"),
        entry: name.to_owned(),
        span: ByteSpan::try_new(start, end).unwrap(),
        classification: LogicalClassification::Structural,
    }
}

fn materialized_peak(control: impl FnOnce(&DecodeContext<'_>)) -> u64 {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = u64::MAX;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
        cadmpeg_core::decode::ResourceDimension::MaterializedBytes,
        "storage peak",
        None,
    );
    control(&ctx);
    let CodecError::ResourceLimit(limit) =
        ctx.reserve_scoped(u64::MAX, "storage peak").unwrap_err()
    else {
        panic!("peak probe must refuse")
    };
    drop(probe);
    limit.limit
}

#[test]
fn physical_coverage_ordering_is_scratch_at_exact_bound_and_preserves_fuse() {
    let physical = [physical(0, 1)];
    let bytes = u64_from_index(size_of::<&ArchiveSpan>());
    for cap in [bytes, bytes - 1] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = byte_coverage(&ctx, &physical, &[], &[], 1);
        if cap == bytes {
            let coverage = result.expect("one borrowed physical reference fits exact scratch cap");
            assert!(coverage.exact);
            assert_eq!(coverage.physical_byte_len, 1);
            assert_eq!(coverage.physical_span_count, 1);
            assert_eq!(coverage.logical_span_count, 0);
            assert!(coverage.classification_bytes.is_empty());
            assert!(coverage.named_opaque_entries.is_empty());
            let reuse = ctx
                .reserve_scoped(bytes, "reuse physical coverage scratch")
                .unwrap();
            drop(reuse);
            assert_eq!(ctx.resource_refusal(), None);
        } else {
            let CodecError::ResourceLimit(original) = result.unwrap_err() else {
                panic!("physical reference storage must refuse below its size");
            };
            assert_eq!(original.dimension, ResourceDimension::MaterializedBytes);
            assert_eq!(original.operation, "FCStd ordered physical spans");
            assert_eq!(
                (original.used, original.additional, original.limit),
                (0, bytes, cap)
            );
            assert_eq!(ctx.resource_refusal(), Some(original));
            assert!(matches!(byte_coverage(&ctx, &physical, &[], &[], 1),
                Err(CodecError::ResourceLimit(actual)) if actual == original));
        }
    }
}

#[test]
fn consumed_logical_group_releases_scratch_before_next_group_sort() {
    const FIRST: usize = 128;
    const SECOND: usize = 1024;
    let physical = [physical(0, 1)];
    let entries = [entry("First", FIRST), entry("Second", SECOND)];
    let logical: Vec<_> = [("First", FIRST), ("Second", SECOND)]
        .into_iter()
        .flat_map(|(name, count)| {
            (0..count)
                .rev()
                .map(move |index| logical(name, u64_from_index(index), u64_from_index(index + 1)))
        })
        .collect();
    let control_entries = [entry("First", 1), entry("Second", SECOND)];
    let mut control_logical: Vec<_> = std::iter::once(logical[0].clone())
        .chain(logical[FIRST..].iter().cloned())
        .collect();
    control_logical[0] = self::logical("First", 0, 1);
    let cap = materialized_peak(|ctx| {
        let coverage =
            byte_coverage(ctx, &physical, &control_entries, &control_logical, 1).unwrap();
        assert!(coverage.exact);
    });
    for limit in [cap, cap - 1] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = byte_coverage(&ctx, &physical, &entries, &logical, 1);
        if limit == cap {
            let coverage = result.expect("removed first group no longer overlaps second sort");
            assert!(coverage.exact);
            assert_eq!(coverage.logical_entry_count, 2);
            assert_eq!(coverage.logical_span_count, FIRST + SECOND);
            assert_eq!(coverage.logical_byte_len, u64_from_index(FIRST + SECOND));
            assert_eq!(
                coverage.classification_bytes.get("structural"),
                Some(&u64_from_index(FIRST + SECOND))
            );
            assert!(coverage.named_opaque_entries.is_empty());
            let reuse = ctx
                .reserve_scoped(cap, "reuse consumed coverage scratch")
                .unwrap();
            drop(reuse);
            assert_eq!(ctx.resource_refusal(), None);
        } else {
            let CodecError::ResourceLimit(original) = result.unwrap_err() else {
                panic!("the control storage bound must refuse below its peak");
            };
            assert_eq!(original.dimension, ResourceDimension::MaterializedBytes);
            assert_eq!(ctx.resource_refusal(), Some(original));
            assert!(
                matches!(byte_coverage(&ctx, &physical, &entries, &logical, 1),
                Err(CodecError::ResourceLimit(actual)) if actual == original)
            );
        }
    }
}

#[test]
fn scoped_coverage_groups_preserve_empty_missing_duplicate_gap_and_overlap_results() {
    let physical = [physical(0, 1)];
    with_service_context(&[], |ctx| {
        let cases = [
            (
                vec![entry("First", 2), entry("Second", 0)],
                vec![logical("First", 1, 2), logical("First", 0, 1)],
                true,
            ),
            (vec![entry("First", 2)], vec![logical("First", 0, 1)], false),
            (
                vec![entry("First", 2)],
                vec![logical("First", 0, 2), logical("First", 1, 2)],
                false,
            ),
            (vec![entry("First", 0)], vec![logical("Ghost", 0, 1)], false),
            (
                vec![entry("First", 1), entry("First", 1)],
                vec![logical("First", 0, 1)],
                false,
            ),
            (vec![entry("First", 0)], vec![logical("First", 0, 1)], false),
            (vec![entry("First", 0)], Vec::new(), true),
        ];
        for (entries, logical, exact) in cases {
            let coverage = byte_coverage(ctx, &physical, &entries, &logical, 1).unwrap();
            assert_eq!(coverage.exact, exact);
            assert_eq!(coverage.logical_entry_count, entries.len());
            assert_eq!(coverage.logical_span_count, logical.len());
            assert!(coverage.named_opaque_entries.is_empty());
        }
        for (spans, end, exact) in [
            (vec![self::physical(1, 2), self::physical(0, 1)], 2, true),
            (vec![self::physical(0, 1), self::physical(2, 3)], 3, false),
            (vec![self::physical(0, 2), self::physical(1, 3)], 3, false),
        ] {
            assert_eq!(
                byte_coverage(ctx, &spans, &[], &[], end).unwrap().exact,
                exact
            );
        }
        assert_eq!(ctx.resource_refusal(), None);
    });
}

#[test]
fn scoped_coverage_groups_preserve_classification_totals_and_ordered_opaque_names() {
    let physical = [physical(0, 1)];
    let entries = [entry("First", 3), entry("Second", 1)];
    let mut logical = vec![
        logical("First", 0, 1),
        logical("First", 1, 2),
        logical("First", 2, 3),
        logical("Second", 0, 1),
    ];
    logical[1].classification = LogicalClassification::Typed {
        owner: "fcstd:native:entry#First".to_owned(),
    };
    logical[2].classification = LogicalClassification::NamedOpaque {
        owner: "fcstd:native:entry#First".to_owned(),
    };
    logical[3].classification = LogicalClassification::NamedOpaque {
        owner: "fcstd:native:entry#Second".to_owned(),
    };
    with_service_context(&[], |ctx| {
        let coverage = byte_coverage(ctx, &physical, &entries, &logical, 1).unwrap();
        assert!(coverage.exact);
        assert_eq!(coverage.logical_byte_len, 4);
        assert_eq!(coverage.classification_bytes.get("structural"), Some(&1));
        assert_eq!(coverage.classification_bytes.get("typed"), Some(&1));
        assert_eq!(coverage.classification_bytes.get("named_opaque"), Some(&2));
        assert_eq!(
            coverage.named_opaque_entries,
            vec!["First".to_owned(), "Second".to_owned()]
        );
        assert_eq!(ctx.resource_refusal(), None);
    });
}
