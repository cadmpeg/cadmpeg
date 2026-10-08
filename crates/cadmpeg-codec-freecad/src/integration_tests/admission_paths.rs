// SPDX-License-Identifier: Apache-2.0
//! Validation and GUI binding admission at orchestration boundaries.

use std::collections::{BTreeSet, HashSet};

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;

#[test]
fn native_comparison_stops_at_the_first_difference() {
    for count in [1, 4096] {
        let stored = vec![1_u8; count];
        let derived = vec![2_u8; count];
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // One pair visit and one byte from each operand.
        policy.limits.max_work_units = 3;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        assert!(matches!(
            crate::first_difference(&ctx, &stored, &derived, "first native difference")
                .expect("first pair"),
            Some(crate::SliceDifference::Pair(0)),
        ));
        assert_eq!(ctx.resource_refusal(), None);
    }
}

#[test]
fn native_comparison_does_not_charge_exact_size_exhaustion() {
    for (stored, derived, budget, different_length) in [
        (&[][..], &[][..], 0, false),
        (&[1_u8][..], &[1_u8][..], 3, false),
        (&[1_u8][..], &[1_u8, 2][..], 3, true),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = budget;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let difference = crate::first_difference(&ctx, stored, derived, "native equal prefix")
            .expect("only actual pairs are charged");
        if different_length {
            assert!(matches!(difference, Some(crate::SliceDifference::Length)));
        } else {
            assert!(difference.is_none());
        }
        assert_eq!(ctx.resource_refusal(), None);
    }
}

#[test]
fn empty_orchestration_paths_preserve_a_fused_refusal() {
    let ir = cadmpeg_ir::document::CadIr::empty();
    let mut entries = Vec::new();
    let gui = crate::gui::Graph::default();
    let affected = BTreeSet::new();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    assert!(crate::validate_native(&ctx, &ir).expect("no native namespace").is_empty());
    assert!(crate::first_difference(&ctx, &[] as &[u8], &[], "empty comparison")
        .expect("empty comparison").is_none());
    crate::bind_gui_entry_references(&ctx, &mut entries, &gui).expect("empty binding");
    assert!(crate::semantic_losses(&ctx, &ir, &affected, Vec::new())
        .expect("no semantic sources").is_empty());
    let CodecError::ResourceLimit(original) = ctx.charge_work(1, "prior orchestration refusal")
        .expect_err("work limit") else {
            panic!("resource refusal")
        };
    for result in [
        crate::validate_native(&ctx, &ir).map(|_| ()),
        crate::first_difference(&ctx, &[] as &[u8], &[], "empty comparison").map(|_| ()),
        crate::bind_gui_entry_references(&ctx, &mut entries, &gui),
        crate::semantic_losses(&ctx, &ir, &affected, Vec::new()).map(|_| ()),
    ] {
        assert!(matches!(result, Err(CodecError::ResourceLimit(repeated)) if repeated == original));
    }
}

fn gui_property(side_entries: Vec<String>) -> crate::native::GuiPropertyRecord {
    crate::native::GuiPropertyRecord {
        id: "fcstd:gui:property#Owner:Data".into(),
        owner: "fcstd:gui:view-provider#Owner".into(),
        name: "Data".into(), type_name: "App::PropertyFileIncluded".into(),
        status: None, order: 0, values: Vec::new(), side_entries,
        xml: crate::native::RetainedXml::from_text("<Property/>".into(), 0).expect("XML span"),
    }
}

#[test]
fn gui_binding_without_side_references_skips_the_entry_index() {
    let mut entries: Vec<_> = (0..128).map(|index| crate::test_support::entry_record(
        format!("fcstd:native:entry#Data{index}.bin"), format!("Data{index}.bin"),
        cadmpeg_core::container::ContainerRole::Auxiliary, Vec::new(), Vec::new(),
    )).collect();
    let gui = crate::gui::Graph { properties: vec![gui_property(Vec::new())], ..Default::default() };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    crate::bind_gui_entry_references(&ctx, &mut entries, &gui).expect("no entry-index consumer");
    assert!(entries.iter().all(|entry| entry.referenced_by().is_empty()));
    assert_eq!(ctx.resource_refusal(), None);

    let gui = crate::gui::Graph {
        properties: vec![gui_property(vec![entries[0].name().into()])], ..Default::default()
    };
    crate::test_support::assert_collection_refusal_at(&[], "FCStd GUI entry references", |ctx| {
        let mut entries = entries.clone();
        crate::bind_gui_entry_references(ctx, &mut entries, &gui)
    });
    crate::test_support::with_service_context(&[], |ctx| {
        crate::bind_gui_entry_references(ctx, &mut entries, &gui).expect("real side-reference consumer");
    });
    assert_eq!(entries[0].referenced_by(), [gui.properties[0].id.as_str()]);
    assert!(entries[1..].iter().all(|entry| entry.referenced_by().is_empty()));
}

#[test]
fn empty_logical_ledger_skips_entry_and_owner_indexes() {
    let entries = [crate::test_support::entry_record(
        "fcstd:native:entry#Empty.bin".into(), "Empty.bin".into(),
        cadmpeg_core::container::ContainerRole::Auxiliary, Vec::new(), Vec::new(),
    )];
    let properties: Vec<_> = (0..128).map(|_| gui_property(Vec::new())).collect();
    let owners = crate::LedgerOwners {
        entries: &entries, gui_properties: &properties, gui_documents: &[],
        shape_payloads: &[], string_tables: &[], element_maps: &[],
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let mut findings = Vec::new();
    crate::validate_logical_ledger(&ctx, &[], &owners, &HashSet::new(), &mut findings)
        .expect("no ledger index consumer");
    assert!(findings.is_empty());
    let CodecError::ResourceLimit(original) = ctx.charge_work(1, "prior ledger refusal")
        .expect_err("work limit") else {
            panic!("resource refusal")
        };
    assert!(matches!(crate::validate_logical_ledger(
        &ctx, &[], &owners, &HashSet::new(), &mut findings,
    ), Err(CodecError::ResourceLimit(repeated)) if repeated == original));
}

#[test]
fn logical_ledger_releases_a_consumed_group_before_the_larger_sort() {
    let counts = [128_usize, 1024];
    let entries: Vec<_> = ["A.bin", "B.bin"].into_iter().zip(counts).map(|(name, count)| {
        crate::test_support::entry_record(
            format!("fcstd:native:entry#{name}"), name.into(),
            cadmpeg_core::container::ContainerRole::Auxiliary, Vec::new(), vec![0; count],
        )
    }).collect();
    let spans: Vec<_> = entries.iter().zip(counts).flat_map(|(entry, count)| {
        (0..count).rev().map(move |index| crate::native::LogicalSpan {
            id: format!("fcstd:native:logical#{}:{index}", entry.name()),
            entry: entry.name().into(),
            span: crate::native::ByteSpan::try_new(
                cadmpeg_core::decode::u64_from_index(index),
                cadmpeg_core::decode::u64_from_index(index + 1),
            ).expect("nonempty interval"),
            classification: crate::native::LogicalClassification::Structural,
        })
    }).collect();
    // Core allocation operations define the live index storage. The larger
    // group then holds one reference buffer and two usize sort buffers. The
    // earlier group has already released its buffer; map nodes remain live.
    let index_bytes = crate::test_support::with_service_context(&[], |ctx| {
        let mut storage = ctx.reserve_scoped(0, "ledger index oracle").expect("storage");
        let _lengths = storage.with_storage(|| ctx.collect_hash_map(
            entries.iter().map(|entry| (entry.name(), entry.byte_len())), "ledger lengths oracle",
        )).expect("entry lengths");
        let _owners = storage.with_storage(|| ctx.collect_hash_set(
            entries.iter().map(crate::native::EntryRecord::id), "ledger owners oracle",
        )).expect("entry owners");
        let _groups = ctx.collect_scoped_btree_map(
            entries.iter().map(|entry| (entry.name(), None::<crate::LedgerSpanGroup<'_, '_>>)),
            "ledger groups oracle",
        ).expect("group tree");
        let CodecError::ResourceLimit(limit) = ctx.reserve_scoped(u64::MAX, "measure ledger indexes")
            .err().expect("materialized overflow") else {
                panic!("materialized refusal")
            };
        limit.used
    });
    let owners = crate::LedgerOwners {
        entries: &entries, gui_properties: &[], gui_documents: &[], shape_payloads: &[],
        string_tables: &[], element_maps: &[],
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = index_bytes + cadmpeg_core::decode::u64_from_index(
        counts[1] * (std::mem::size_of::<&crate::native::LogicalSpan>()
            + 2 * std::mem::size_of::<usize>()),
    );
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let mut findings = Vec::new();
    crate::validate_logical_ledger(&ctx, &spans, &owners, &HashSet::new(), &mut findings)
        .expect("the consumed group no longer contributes to the later sort peak");
    assert!(findings.is_empty());
    assert_eq!(ctx.resource_refusal(), None);
}
