// SPDX-License-Identifier: Apache-2.0
use crate::container::{ContainerScan, Layout, Section};
use crate::decode::coverage::source_section_ref;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn policy(work: u64) -> DecodePolicy {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = work;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    policy
}

fn scan_with_sections(layout: Layout) -> ContainerScan<'static> {
    let mut scan = crate::test_support::empty_container_scan();
    scan.framing.layout = layout;
    let data = [0; 48];
    scan.framing.sections = [
        ("empty", 0, 0),
        ("first", 0, 16),
        ("overlap", 8, 24),
        ("middle", 16, 32),
        ("last", 32, 48),
    ]
    .map(|(name, start, end)| {
        Section::scan_for_test(name.to_string(), start, end, None, &data)
            .expect("bounded section")
            .section
    })
    .into_iter()
    .collect();
    scan
}

#[test]
fn source_section_empty_fallback_is_free_and_preserves_original_refusal() {
    for (layout, fallback) in [
        (Layout::Nd, "unknown"),
        (crate::test_support::legacy_layout(), "legacy_ascii"),
    ] {
        let mut scan = scan_with_sections(layout);
        let arena = DecodeArena::new();
        let policy = policy(0);
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let sections = std::mem::take(&mut scan.framing.sections);
        assert_eq!(source_section_ref(&ctx, &scan, 0).expect("empty fallback"), fallback);
        assert_eq!(source_section_ref(&ctx, &scan, usize::MAX).expect("empty fallback"), fallback);
        let original = ctx.charge_work_limit(1, "seed source section refusal").expect_err("zero cap");
        assert_eq!((original.used, original.additional), (0, 1));
        assert!(matches!(source_section_ref(&ctx, &scan, 0),
            Err(CodecError::ResourceLimit(actual)) if actual == original));
        scan.framing.sections = sections;
        assert!(matches!(source_section_ref(&ctx, &scan, 0),
            Err(CodecError::ResourceLimit(actual)) if actual == original));
        assert_eq!(ctx.resource_refusal(), Some(original));
    }
}

#[test]
fn source_section_search_admits_only_present_sections_and_keeps_first_match() {
    for (layout, fallback) in [
        (Layout::Nd, "unknown"),
        (crate::test_support::legacy_layout(), "legacy_ascii"),
    ] {
        let scan = scan_with_sections(layout);
        for (offset, expected, index, visits) in [
            (0, "first", Some(1), 2),
            (8, "first", Some(1), 2),
            (15, "first", Some(1), 2),
            (16, "overlap", Some(2), 3),
            (24, "middle", Some(3), 4),
            (31, "middle", Some(3), 4),
            (32, "last", Some(4), 5),
            (47, "last", Some(4), 5),
            (48, fallback, None, 5),
            (usize::MAX, fallback, None, 5),
        ] {
            crate::test_support::assert_refusal_order(
                ResourceDimension::WorkUnits,
                &vec!["creo source section search"; usize::try_from(visits).expect("fixture visits")],
                |cap| {
                let arena = DecodeArena::new();
                let policy = policy(cap);
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
                let result = source_section_ref(&ctx, &scan, offset);
                if cap == visits {
                    let name = result.expect("actual visits admitted");
                    assert_eq!(name, expected);
                    if let Some(index) = index {
                        assert!(std::ptr::eq(name, scan.framing.sections[index].name()));
                    }
                    let exhausted = ctx.charge_work_limit(1, "source section exact visit boundary")
                        .expect_err("all admitted visits used");
                    assert_eq!((exhausted.used, exhausted.additional), (visits, 1));
                } else {
                    let Err(CodecError::ResourceLimit(original)) = result else {
                        panic!("one present section visit must refuse")
                    };
                    assert_eq!(original.dimension, ResourceDimension::WorkUnits);
                    assert_eq!(original.operation, "creo source section search");
                    assert_eq!((original.limit, original.used, original.additional), (cap, cap, 1));
                    assert!(matches!(source_section_ref(&ctx, &scan, offset),
                        Err(CodecError::ResourceLimit(actual)) if actual == original));
                    assert_eq!(ctx.resource_refusal(), Some(original));
                    return Err(original.into());
                }
                Ok(())
            });
        }
    }
}
