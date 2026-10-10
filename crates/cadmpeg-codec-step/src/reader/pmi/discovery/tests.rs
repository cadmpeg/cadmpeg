// SPDX-License-Identifier: Apache-2.0
//! Shared annotation text and placement discovery.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::CadIr;

mod original_context;

fn exchange(records: &str) -> crate::parse::Exchange {
    let source = format!("ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;{records}ENDSEC;END-ISO-10303-21;");
    crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
        .expect("annotation exchange")
        .0
}

fn shared_annotations(cyclic: bool) {
    let mut records = String::from("#1=CARTESIAN_POINT('',(0.,0.,0.));#2=DIRECTION('',(0.,0.,1.));#3=DIRECTION('',(1.,0.,0.));#4=AXIS2_PLACEMENT_3D('',#1,#2,#3);#5=TEXT_LITERAL('shared',#4,.LEFT.);#10=ITEM((");
    for id in 11..=74 {
        if id != 11 {
            records.push(',');
        }
        write!(records, "#{id}").expect("write reference");
    }
    records.push_str("));");
    for id in 11..=74 {
        write!(records, "#{id}=ITEM(#5);").expect("write item");
    }
    for id in 100..228 {
        write!(records, "#{id}=ANNOTATION_TEXT('',#10);").expect("write annotation");
    }
    if cyclic {
        records = records.replace("#74=ITEM(#5);", "#74=ITEM(#5,#10);");
    }
    let exchange = exchange(&records);
    let arena = DecodeArena::new();
    let (setup, _) =
        DecodeContext::from_root_bytes(b"", &arena, &DecodePolicy::service()).expect("setup");
    let geometry =
        crate::reader::geometry::decode(&exchange, &mut CadIr::empty(), &setup).expect("geometry");
    let mut policy = DecodePolicy::service();
    // A stage index holds shared queries once; rebuilding both 70-node graphs
    // for every annotation would exceed this item budget.
    policy.limits.max_collection_items = 4000;
    policy.limits.max_retained_bytes = 128 * 6;
    crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
        let mut index = super::AnnotationDiscoveryIndex::new(ctx).expect("index");
        let reports =
            std::cell::RefCell::new(ctx.reserve_scoped(0, "report fixture").expect("scope"));
        let mut losses = Vec::new();
        for id in 100..228 {
            let mut claims = BTreeSet::new();
            let mut claim_storage = ctx.reserve_scoped(0, "claim fixture").expect("scope");
            let text = index.text(
                id,
                &exchange,
                &geometry.value,
                (&mut claims, &mut claim_storage),
                (&mut losses, &reports),
            )
            .expect("selected text");
            assert_eq!(text.as_deref(), Some("shared"));
            assert_eq!(claims, BTreeSet::from([5]));
            let placement = index.placement(exchange.records().get(&id).expect("annotation"), &exchange, &geometry.value).expect("placement selection");
            let super::PlacementSelection::Unique(placement) = placement else { panic!("one carrier"); };
            assert_eq!(placement.rows()[0][3], 0.0);
        }
        assert!(losses.is_empty());
    });
}

#[test]
fn annotation_discovery_reuses_shared_text_and_placement_graphs() {
    shared_annotations(false);
}

#[test]
fn annotation_discovery_reuses_shared_cyclic_graphs() {
    shared_annotations(true);
}

#[test]
fn indexed_annotation_text_replays_invalid_warnings_in_dfs_order() {
    let exchange = exchange("#1=ANNOTATION_TEXT('',(#3,#2,#3));#2=TEXT_LITERAL('\\X2\\D83D\\X0\\',$,.LEFT.);#3=TEXT_LITERAL('\\X2\\DE42\\X0\\',$,.LEFT.);");
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &DecodePolicy::service()).expect("context");
    let geometry =
        crate::reader::geometry::decode(&exchange, &mut CadIr::empty(), &ctx).expect("geometry");
    let mut index = super::AnnotationDiscoveryIndex::new(&ctx).expect("index");
    let reports = std::cell::RefCell::new(ctx.reserve_scoped(0, "report fixture").expect("scope"));
    let mut expected = Vec::new();
    let mut used = BTreeSet::new();
    let mut claims = ctx.reserve_scoped(0, "claim fixture").expect("scope");
    assert!(super::super::find_annotation_text(
        1,
        &exchange,
        &mut BTreeSet::new(),
        (&mut used, &mut claims),
        (&mut expected, &reports),
        0,
        &ctx
    )
    .expect("reference text query")
    .is_none());
    assert_eq!(expected.len(), 2);
    assert!(expected[0].message.contains("record #3"));
    assert!(expected[1].message.contains("record #2"));
    for _ in 0..2 {
        let mut actual = Vec::new();
        assert!(index.text(
            1,
            &exchange,
            &geometry.value,
            (&mut BTreeSet::new(), &mut claims),
            (&mut actual, &reports),
        )
        .expect("indexed text query")
        .is_none());
        assert_eq!(actual, expected);
    }
}

#[test]
fn annotation_index_preserves_cyclic_discovery_results() {
    let exchange = exchange(
        "#1=ANNOTATION_TEXT('',(#2));#2=ITEM(#1,#3);#3=TEXT_LITERAL('cycle text',$,.LEFT.);",
    );
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &DecodePolicy::service()).expect("context");
    let geometry =
        crate::reader::geometry::decode(&exchange, &mut CadIr::empty(), &ctx).expect("geometry");
    let mut index = super::AnnotationDiscoveryIndex::new(&ctx).expect("index");
    let reports = std::cell::RefCell::new(ctx.reserve_scoped(0, "report fixture").expect("scope"));
    let mut claims = ctx.reserve_scoped(0, "claim fixture").expect("scope");
    assert_eq!(
        super::super::find_annotation_text(
            1,
            &exchange,
            &mut BTreeSet::new(),
            (&mut BTreeSet::new(), &mut claims),
            (&mut Vec::new(), &reports),
            0,
            &ctx
        )
        .expect("cyclic text query")
        .as_deref(),
        Some("cycle text")
    );
    assert_eq!(
        index.text(
            1,
            &exchange,
            &geometry.value,
            (&mut BTreeSet::new(), &mut claims),
            (&mut Vec::new(), &reports),
        )
        .expect("indexed cyclic text query")
        .as_deref(),
        Some("cycle text")
    );
}

#[test]
fn annotation_index_keeps_each_cyclic_root_query_independent() {
    let exchange = exchange("#1=ITEM(#2,#3);#2=ITEM(#1);#3=TEXT_LITERAL('text',$,.LEFT.);");
    crate::test_support::with_service_context(b"", |_, ctx| {
        let geometry = crate::reader::geometry::decode(&exchange, &mut CadIr::empty(), ctx).expect("geometry");
        let mut index = super::AnnotationDiscoveryIndex::new(ctx).expect("index");
        let reports = std::cell::RefCell::new(ctx.reserve_scoped(0, "test reports").expect("scope"));
        let mut claims = ctx.reserve_scoped(0, "test claims").expect("scope");
        for id in [2, 1, 2] {
            let mut expected_used = BTreeSet::new();
            let mut expected_losses = Vec::new();
            let expected = super::super::find_annotation_text(id, &exchange, &mut BTreeSet::new(), (&mut expected_used, &mut claims), (&mut expected_losses, &reports), 0, ctx).expect("root-specific DFS");
            let mut used = BTreeSet::new();
            let mut losses = Vec::new();
            let actual = index.text(id, &exchange, &geometry.value, (&mut used, &mut claims), (&mut losses, &reports)).expect("cached root query");
            assert_eq!(actual.as_deref(), Some("text"));
            assert_eq!(actual, expected);
            assert_eq!(used, expected_used);
            assert_eq!(losses, expected_losses);
        }
    });
}

#[test]
fn annotation_discovery_shares_wide_child_selection_summaries() {
    for count in [16_u64, 32, 64] {
        let mut records = String::from("#1=CARTESIAN_POINT('',(0.,0.,0.));#2=DIRECTION('',(0.,0.,1.));#3=DIRECTION('',(1.,0.,0.));#5=ITEM((");
        for id in 10..10 + count {
            if id != 10 { records.push(','); }
            write!(records, "#{id}").expect("text reference");
        }
        records.push_str("));");
        for id in 10..10 + count {
            write!(records, "#{id}=TEXT_LITERAL('text {id}',#{placement},.LEFT.);#{placement}=AXIS2_PLACEMENT_3D('',#1,#2,#3);", placement = id + 500).expect("text and placement");
        }
        for id in 1000..1000 + count {
            write!(records, "#{id}=ANNOTATION_TEXT_OCCURRENCE('',(),#5);").expect("annotation root");
        }
        let exchange = exchange(&records);
        let setup = cadmpeg_test_support::service_decode_context();
        let geometry = crate::reader::geometry::decode(&exchange, &mut CadIr::empty(), &setup).expect("geometry");
        let mut policy = DecodePolicy::service();
        // At 64 roots and carriers, copied root closures need at least 4096
        // text slots. The complete text and placement queries fit in 1280 items.
        policy.limits.max_collection_items = 8 * (2 * count) + 256;
        policy.limits.max_work_units = 10_000 * (2 * count);
        crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
            let mut index = super::AnnotationDiscoveryIndex::new(ctx).expect("index");
            let reports = std::cell::RefCell::new(ctx.reserve_scoped(0, "test reports").expect("scope"));
            let mut claims = ctx.reserve_scoped(0, "test claims").expect("scope");
            let mut losses = Vec::new();
            let mut used = BTreeSet::new();
            for id in 1000..1000 + count {
                assert!(index.text(id, &exchange, &geometry.value, (&mut used, &mut claims), (&mut losses, &reports)).expect("shared text summary").is_none());
                let placement = index.placement(exchange.records().get(&id).expect("root"), &exchange, &geometry.value).expect("shared placement summary");
                assert!(matches!(placement, super::PlacementSelection::Ambiguous(found) if u64::try_from(found).expect("carrier count") == count));
            }
            assert!(used.is_empty());
            assert_eq!(u64::try_from(losses.len()).expect("loss count"), count);
            assert!(losses.iter().all(|loss| loss.code == crate::loss::StepLossCode::PresentationAnnotationTextUnordered.kind()));
            assert_eq!(index.summaries.len(), 1);
            assert_eq!(index.placements.len(), 1);
        });
    }
}

#[test]
fn annotation_discovery_releases_cached_materialization() {
    let exchange = exchange("#1=ITEM(#2,#3);#2=TEXT_LITERAL('a',$,.LEFT.);#3=TEXT_LITERAL('b',$,.LEFT.);");
    let setup = cadmpeg_test_support::service_decode_context();
    let geometry = crate::reader::geometry::decode(&exchange, &mut CadIr::empty(), &setup).expect("geometry");
    crate::test_support::with_service_context(b"", |_, ctx| {
        {
            let mut index = super::AnnotationDiscoveryIndex::new(ctx).expect("index");
            let reports = std::cell::RefCell::new(ctx.reserve_scoped(0, "test reports").expect("scope"));
            let mut claims = ctx.reserve_scoped(0, "test claims").expect("scope");
            let mut used = BTreeSet::new();
            let mut losses = Vec::new();
            assert!(index.text(1, &exchange, &geometry.value, (&mut used, &mut claims), (&mut losses, &reports)).expect("ambiguous selection").is_none());
            assert_eq!(losses.len(), 1);
        }
        let cadmpeg_core::CodecError::ResourceLimit(refusal) = ctx.reserve_scoped(u64::MAX, "test cache release observation").expect_err("observation exceeds limit") else { panic!("resource refusal"); };
        assert_eq!(refusal.dimension, cadmpeg_core::decode::ResourceDimension::MaterializedBytes);
        assert_eq!(refusal.used, 0, "cached strings, summary nodes and report buffers were released");
    });
}
