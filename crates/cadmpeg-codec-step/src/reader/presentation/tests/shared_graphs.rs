// SPDX-License-Identifier: Apache-2.0
//! Shared presentation graph queries.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use cadmpeg_core::decode::DecodePolicy;
use cadmpeg_ir::CadIr;

fn exchange(records: &str) -> crate::parse::Exchange {
    let source = format!("ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;{records}ENDSEC;END-ISO-10303-21;");
    crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
        .expect("graph exchange")
        .0
}

fn layered_graph(name: &str, levels: u64) -> String {
    let mut records = String::new();
    for id in 1..=levels * 2 {
        let next = ((id - 1) / 2 + 1) * 2 + 1;
        writeln!(records, "#{id}={name}('',(#{next},#{}));", next + 1).expect("write graph");
    }
    writeln!(
        records,
        "#{}=CARTESIAN_POINT('',(0.,0.,0.));#{}=CARTESIAN_POINT('',(0.,0.,0.));",
        levels * 2 + 1,
        levels * 2 + 2
    )
    .expect("write leaves");
    records
}

#[test]
fn domain_resolution_reuses_a_shared_dag() {
    let exchange = exchange(&layered_graph("GEOMETRIC_SET", 16));
    let mut policy = DecodePolicy::service();
    // Thirty-three reachable nodes need one active and one completed entry each.
    // The first query transfers its 33-node completion map: 66 total slots.
    // Root #2 needs one active, one pending and one stage entry: 69 total.
    // Repeating all 33 nodes needs 66 + 66 + 1 = 133 slots; publication
    // charges only the new root, because existing map keys add no slots.
    policy.limits.max_collection_items = 128;
    crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
        let mut index = super::super::StyleDomainIndex::new(ctx).expect("stage index");
        assert!(matches!(
            index.domain(1, &exchange).expect("linear domain walk"),
            super::super::StyleDomain::Point
        ));
        assert_eq!(index.complete.len(), 33);
        assert!(matches!(
            index.domain(2, &exchange).expect("distinct root reuses its descendants"),
            super::super::StyleDomain::Point
        ));
        assert_eq!(index.complete.len(), 34);
    });
}

#[test]
fn style_domain_name_substrings_keep_all_classifications() {
    let cases = [
        ("XPOINTX", super::super::StyleDomain::Point),
        ("XVERTEXX", super::super::StyleDomain::Point),
        ("XCURVEX", super::super::StyleDomain::Curve),
        ("XEDGEX", super::super::StyleDomain::Curve),
        ("X_LINE_X", super::super::StyleDomain::Curve),
        ("XFACEX", super::super::StyleDomain::Surface),
        ("XSURFACEX", super::super::StyleDomain::Surface),
        ("XSOLIDX", super::super::StyleDomain::Surface),
        ("XSHELLX", super::super::StyleDomain::Surface),
    ];

    for (name, expected) in cases {
        let exchange = exchange(&format!("#1={name}();"));
        crate::test_support::with_service_context(b"", |_, ctx| {
            let actual = super::super::StyleDomainIndex::new(ctx).and_then(|mut index| index.domain(1, &exchange))
                .expect("style domain classification");
            assert!(actual == expected, "wrong style domain for {name}");
        });
    }
}

#[test]
fn style_domain_point_prefix_does_not_scan_the_name_suffix() {
    let exchange = exchange(&format!("#1=POINT{}();", "X".repeat(4096)));
    let mut policy = DecodePolicy::service();
    // The one-record path has one 8-byte record lookup, a two-step partial
    // probe, a one-window POINT match, a 696-unit active-set insertion, a
    // 240-unit active-set removal, and a 729-unit cache insertion. The cap
    // leaves room for this 1,676-unit route while the old 4,106-unit
    // full-name-plus-pattern scan cannot fit.
    policy.limits.max_work_units = 2048;
    crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
        let actual = super::super::StyleDomainIndex::new(ctx).and_then(|mut index| index.domain(1, &exchange))
            .expect("POINT prefix fits the work limit");
        assert!(actual == super::super::StyleDomain::Point);
        assert!(ctx.resource_refusal().is_none());
    });
}

#[test]
fn style_domain_query_error_does_not_publish_completed_descendants() {
    let exchange = exchange("#1=GEOMETRIC_SET('',(#2,#3));#2=CARTESIAN_POINT('',(0.,0.,0.));#3=CARTESIAN_POINT('',(1.,0.,0.));");
    let mut policy = DecodePolicy::service();
    // Root active, #2 active and #2 pending completion precede #3's active slot.
    policy.limits.max_collection_items = 3;
    crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
        let mut index = super::super::StyleDomainIndex::new(ctx).expect("stage index");
        let error = match index.domain(1, &exchange) {
            Err(error) => error,
            Ok(_) => panic!("the second child needs a fourth collection slot"),
        };
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
                && limit.operation == "step_presentation_style_domain_active"
                && limit.used == 3 && limit.additional == 1
                && ctx.resource_refusal() == Some(limit)));
        assert!(index.complete.is_empty());
    });
}

#[test]
fn cached_style_domain_does_not_enter_skipped_child_depth() {
    let exchange = exchange("#1=GEOMETRIC_SET('',(#2));#2=CARTESIAN_POINT('',(0.,0.,0.));");
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 1;
    crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
        let mut index = super::super::StyleDomainIndex::new(ctx).expect("stage index");
        assert!(matches!(index.domain(2, &exchange).expect("leaf root"),
            super::super::StyleDomain::Point));
        assert!(matches!(index.domain(1, &exchange).expect("cached child has no descent"),
            super::super::StyleDomain::Point));
        assert_eq!(ctx.resource_refusal(), None);
    });
}

#[test]
fn invisible_resolution_reuses_a_shared_dag() {
    let exchange = exchange(&layered_graph("REPRESENTATION", 16));
    let mut ir = CadIr::empty();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (setup, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(b"", &arena, &DecodePolicy::service())
            .expect("setup");
    let carriers = crate::reader::index::CarrierIndex::from_ir(&ir, &setup).expect("carrier index");
    let topology =
        crate::reader::topology::decode(&exchange, &mut ir, &carriers, &setup).expect("topology");
    let indices = BTreeMap::from([
        ("step:data:body#33".to_owned(), 0),
        ("step:data:body#34".to_owned(), 1),
    ]);
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 128;
    crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
        let mut index = super::super::InvisibleIndex::new(ctx).expect("stage index");
        let mut bodies = [test_body(33), test_body(34)];
        let mut prepared = index.prepare(1, &exchange, &topology.value, &indices)
            .expect("linear invisibility walk");
        assert_eq!(prepared.summary, super::super::InvisibleSummary::Supported { hidden: true });
        assert_eq!(
            prepared.body_ids.keys().map(cadmpeg_ir::ids::BodyId::as_str).collect::<Vec<_>>(),
            ["step:data:body#33", "step:data:body#34"]
        );
        for (body_id, index) in std::mem::take(&mut prepared.body_ids) {
            let index = index.expect("resolved test body index");
            assert_eq!(body_id, bodies[index].id);
            bodies[index].visible = Some(false);
        }
        index.publish(prepared).expect("completed effect publication");
        assert_eq!(index.complete.len(), 33);
        let prepared = index.prepare(2, &exchange, &topology.value, &indices)
            .expect("distinct root reuses descendants");
        assert_eq!(prepared.summary, super::super::InvisibleSummary::Supported { hidden: true });
        assert!(prepared.body_ids.is_empty());
        index.publish(prepared).expect("second completed root");
        assert_eq!(index.complete.len(), 34);
        assert!(bodies.iter().all(|body| body.visible == Some(false)));
    });
}

#[test]
fn domain_cycles_keep_any_and_invisibility_cycles_keep_unsupported() {
    let exchange = exchange(
        "#1=GEOMETRIC_SET('',(#2,#3));#2=GEOMETRIC_SET('',(#1));#3=CARTESIAN_POINT('',(0.,0.,0.));",
    );
    crate::test_support::with_service_context(b"", |_, ctx| {
        let mut index = super::super::StyleDomainIndex::new(ctx).expect("stage index");
        assert!(matches!(
            index.domain(1, &exchange).expect("cycle walk"),
            super::super::StyleDomain::Any
        ));
        assert!(matches!(
            index.domain(2, &exchange).expect("completed cycle remains Any"),
            super::super::StyleDomain::Any
        ));
    });
    let exchange =
        self::exchange("#1=REPRESENTATION('',(#2,#3),$);#2=REPRESENTATION('',(#1),$);#3=ITEM();");
    let mut ir = CadIr::empty();
    crate::test_support::with_service_context(b"", |_, ctx| {
        let carriers =
            crate::reader::index::CarrierIndex::from_ir(&ir, ctx).expect("carrier index");
        let topology =
            crate::reader::topology::decode(&exchange, &mut ir, &carriers, ctx).expect("topology");
        let indices = BTreeMap::from([("step:data:body#3".to_owned(), 0)]);
        let mut index = super::super::InvisibleIndex::new(ctx).expect("stage index");
        let mut body = test_body(3);
        let mut prepared = index.prepare(1, &exchange, &topology.value, &indices)
            .expect("cycle walk");
        assert_eq!(prepared.summary, super::super::InvisibleSummary::Unsupported);
        assert_eq!(prepared.body_ids.len(), 1);
        assert!(index.complete.is_empty());
        for (id, index) in std::mem::take(&mut prepared.body_ids) {
            assert_eq!(index, Some(0));
            assert_eq!(id, body.id);
            body.visible = Some(false);
        }
        index.publish(prepared).expect("completed cycle effects");
        let prepared = index.prepare(2, &exchange, &topology.value, &indices)
            .expect("completed cycle descendant");
        assert_eq!(prepared.summary, super::super::InvisibleSummary::Unsupported);
        assert!(prepared.body_ids.is_empty());
        assert_eq!(body.visible, Some(false));
    });
}

#[test]
fn color_resolution_reuses_warning_free_queries_across_styles() {
    let mut records = String::from("#1=COLOUR_RGB('red',1.,0.,0.);#2=ITEM((");
    for id in 3..=66 {
        if id != 3 {
            records.push(',');
        }
        write!(records, "#{id}").expect("write reference");
    }
    records.push_str("));");
    for id in 3..=66 {
        write!(records, "#{id}=ITEM(#1);").expect("write item");
    }
    let exchange = exchange(&records);
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 1000;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_work_units = 300_000;
    crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
        let mut shared = super::super::StyleColors {
            prefixes: BTreeMap::new(),
            values: BTreeMap::new(),
            storage: ctx.reserve_scoped(0, "shared fixture").expect("scope"),
        };
        let reports =
            std::cell::RefCell::new(ctx.reserve_scoped(0, "report fixture").expect("scope"));
        let mut losses = Vec::new();
        let mut claims = std::collections::BTreeSet::new();
        let mut claim_storage = ctx.reserve_scoped(0, "claim fixture").expect("scope");
        for _ in 0..128 {
            let storage =
                std::cell::RefCell::new(ctx.reserve_scoped(0, "query fixture").expect("scope"));
            let (query, cached) = shared
                .resolve(
                    &[2],
                    &exchange,
                    super::super::StyleDomain::Surface,
                    &storage,
                    (&mut losses, &reports),
                    ctx,
                )
                .expect("shared color query");
            let result = cached.color.as_ref().expect("color");
            let super::super::ColorResolution::Candidate(candidate) = result else {
                panic!("singleton color expected")
            };
            assert_eq!(
                (
                    candidate.color.r(),
                    candidate.color.g(),
                    candidate.color.b()
                ),
                (1.0, 0.0, 0.0)
            );
            shared
                .claim(
                    query,
                    super::super::StyleDomain::Surface,
                    (&mut claims, &mut claim_storage),
                    ctx,
                )
                .expect("shared graph claims");
        }
        assert_eq!(claims, (1..=66).collect());
        assert!(losses.is_empty());
    });
}

#[test]
fn shared_color_cache_keeps_depth_cutoffs_and_per_style_warnings() {
    use std::collections::BTreeSet;
    let exchange = exchange("#1=COLOUR_RGB('red',1.,0.,0.);#2=ITEM(#1);#3=SURFACE_SIDE_STYLE('',(#1));#4=SURFACE_STYLE_USAGE(.MIDDLE.,#3);");
    crate::test_support::with_service_context(b"", |_, ctx| {
        let mut shared = super::super::StyleColors {
            prefixes: BTreeMap::new(),
            values: BTreeMap::new(),
            storage: ctx.reserve_scoped(0, "shared fixture").expect("scope"),
        };
        let reports =
            std::cell::RefCell::new(ctx.reserve_scoped(0, "report fixture").expect("scope"));
        let mut losses = Vec::new();
        for (depth, has_color) in [(255, false), (0, true)] {
            let storage =
                std::cell::RefCell::new(ctx.reserve_scoped(0, "query fixture").expect("scope"));
            let result = super::super::find_color(
                2,
                &exchange,
                super::super::StyleDomain::Surface,
                super::super::ColorSearchState {
                    storage: &storage,
                    active: &mut BTreeSet::new(),
                    cache: &mut BTreeMap::new(),
                    losses: (&mut losses, &reports),
                    invalid_surface_sides: &mut BTreeSet::new(),
                },
                depth,
                ctx,
            )
            .expect("bounded color query");
            assert_eq!(result.is_some(), has_color);
        }
        for _ in 0..2 {
            let storage =
                std::cell::RefCell::new(ctx.reserve_scoped(0, "query fixture").expect("scope"));
            assert!(shared
                .resolve(
                    &[4],
                    &exchange,
                    super::super::StyleDomain::Surface,
                    &storage,
                    (&mut losses, &reports),
                    ctx
                )
                .expect("invalid side query")
                .1
                .color
                .is_none());
        }
        assert_eq!(losses.len(), 2);
        assert_eq!(losses[0], losses[1]);
    });
}

#[test]
fn shared_style_color_query_keeps_ordered_local_cache_history() {
    // Exercise the full cutoff path on an ordinary thread stack.
    std::thread::spawn(|| {
            let mut records = String::from("#1=COLOUR_RGB('red',1.,0.,0.);#2=ITEM(#1);");
            for id in 10..265 {
                write!(
                    records,
                    "#{id}=ITEM(#{});",
                    if id == 264 { 2 } else { id + 1 }
                )
                .expect("write deep graph");
            }
            let exchange = exchange(&records);
            let mut policy = DecodePolicy::service();
            policy.limits.max_recursion_depth = 1024;
            crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
                let reports = std::cell::RefCell::new(
                    ctx.reserve_scoped(0, "report fixture").expect("scope"),
                );
                let mut shared = super::super::StyleColors {
                    prefixes: BTreeMap::new(),
                    values: BTreeMap::new(),
                    storage: ctx.reserve_scoped(0, "shared fixture").expect("scope"),
                };
                for references in [[2, 10], [10, 2], [2, 10], [10, 2]] {
                    let storage = std::cell::RefCell::new(
                        ctx.reserve_scoped(0, "query fixture").expect("scope"),
                    );
                    let mut losses = Vec::new();
                    let result = &shared
                        .resolve(
                            &references,
                            &exchange,
                            super::super::StyleDomain::Surface,
                            &storage,
                            (&mut losses, &reports),
                            ctx,
                        )
                        .expect("ordered query")
                        .1
                        .color;
                    // The deep root reaches ITEM #2 at depth 255. Resolving #2 first
                    // seeds its full local result; the reverse order seeds None.
                    assert_eq!(result.is_some(), references[0] == 2);
                    if let Some(super::super::ColorResolution::Candidate(candidate)) = result {
                        assert_eq!(candidate.color.r(), 1.0);
                    }
                    assert!(losses.is_empty());
                }
                assert_eq!(shared.values.len(), 2);
            });
        })
        .join()
        .expect("color query assertions");
}

fn test_body(id: u64) -> cadmpeg_ir::topology::Body {
    cadmpeg_ir::topology::Body {
        id: cadmpeg_ir::ids::BodyId::from(crate::ids::data(crate::ids::kind!("body"), id)),
        kind: cadmpeg_ir::topology::BodyKind::default(),
        regions: Vec::new(), transform: None, name: None, color: None, visible: None,
    }
}
