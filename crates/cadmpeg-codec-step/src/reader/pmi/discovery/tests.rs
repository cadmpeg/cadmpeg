// SPDX-License-Identifier: Apache-2.0
//! Shared annotation text and placement discovery.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::CadIr;

mod ownership;

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
        let mut index = super::AnnotationDiscoveryIndex {
            graphs: BTreeMap::new(),
            texts: BTreeMap::new(),
            independent_reach: BTreeMap::new(),
            storage: ctx.reserve_scoped(0, "index fixture").expect("scope"),
        };
        let reports =
            std::cell::RefCell::new(ctx.reserve_scoped(0, "report fixture").expect("scope"));
        let mut losses = Vec::new();
        for id in 100..228 {
            assert!(super::index_annotation_graph(
                id,
                0,
                &exchange,
                &geometry.value,
                &mut BTreeSet::new(),
                &mut index,
                ctx
            )
            .expect("shared query"));
            let mut claims = BTreeSet::new();
            let mut claim_storage = ctx.reserve_scoped(0, "claim fixture").expect("scope");
            let text = super::indexed_annotation_text(
                id,
                &exchange,
                &mut index,
                (&mut claims, &mut claim_storage),
                (&mut losses, &reports),
                ctx,
            )
            .expect("selected text");
            assert_eq!(text.as_deref(), Some("shared"));
            assert_eq!(claims, BTreeSet::from([5]));
            let (graph, _) = index.graphs.get(&(id, 0)).expect("graph result");
            let placements = &graph.as_ref().expect("complete graph").placements;
            assert_eq!(placements.len(), 1);
            assert_eq!(
                placements.get(&4).expect("carrier identity").rows()[0][3],
                0.0
            );
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
    let mut index = super::AnnotationDiscoveryIndex {
        graphs: BTreeMap::new(),
        texts: BTreeMap::new(),
        independent_reach: BTreeMap::new(),
        storage: ctx.reserve_scoped(0, "index fixture").expect("scope"),
    };
    assert!(super::index_annotation_graph(
        1,
        0,
        &exchange,
        &geometry.value,
        &mut BTreeSet::new(),
        &mut index,
        &ctx
    )
    .expect("index"));
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
        assert!(super::indexed_annotation_text(
            1,
            &exchange,
            &mut index,
            (&mut BTreeSet::new(), &mut claims),
            (&mut actual, &reports),
            &ctx
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
    let mut index = super::AnnotationDiscoveryIndex {
        graphs: BTreeMap::new(),
        texts: BTreeMap::new(),
        independent_reach: BTreeMap::new(),
        storage: ctx.reserve_scoped(0, "index fixture").expect("scope"),
    };
    assert!(super::index_annotation_graph(
        1,
        0,
        &exchange,
        &geometry.value,
        &mut BTreeSet::new(),
        &mut index,
        &ctx
    )
    .expect("cyclic graph is indexed"));
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
        super::indexed_annotation_text(
            1,
            &exchange,
            &mut index,
            (&mut BTreeSet::new(), &mut claims),
            (&mut Vec::new(), &reports),
            &ctx
        )
        .expect("indexed cyclic text query")
        .as_deref(),
        Some("cycle text")
    );
}

#[test]
fn annotation_index_does_not_reuse_a_cycle_under_a_reachable_ancestor() {
    let exchange = exchange("#1=ITEM(#2,#3);#2=ITEM(#1);#3=TEXT_LITERAL('text',$,.LEFT.);");
    crate::test_support::with_service_context(b"", |_, ctx| {
        let geometry =
            crate::reader::geometry::decode(&exchange, &mut CadIr::empty(), ctx).expect("geometry");
        let mut index = super::AnnotationDiscoveryIndex {
            graphs: BTreeMap::new(),
            texts: BTreeMap::new(),
            independent_reach: BTreeMap::new(),
            storage: ctx.reserve_scoped(0, "index fixture").expect("scope"),
        };
        assert!(super::index_annotation_graph(
            2,
            1,
            &exchange,
            &geometry.value,
            &mut BTreeSet::new(),
            &mut index,
            ctx
        )
        .expect("independent query"));
        assert!(!super::index_annotation_graph(
            2,
            1,
            &exchange,
            &geometry.value,
            &mut BTreeSet::from([1]),
            &mut index,
            ctx
        )
        .expect("ancestor-sensitive query"));
    });
}
