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
    // Re-expanding the 2^16 source paths cannot fit this item budget.
    policy.limits.max_collection_items = 128;
    crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
        assert!(matches!(
            super::super::style_domain(1, &exchange, ctx).expect("linear domain walk"),
            super::super::StyleDomain::Point
        ));
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
        let (bodies, supported) =
            super::super::invisible_body_ids(1, &exchange, &topology.value, &indices, ctx)
                .expect("linear invisibility walk");
        assert!(supported);
        assert_eq!(
            bodies
                .iter()
                .map(cadmpeg_ir::ids::BodyId::as_str)
                .collect::<Vec<_>>(),
            ["step:data:body#33", "step:data:body#34"]
        );
    });
}

#[test]
fn domain_cycles_keep_any_and_invisibility_cycles_keep_unsupported() {
    let exchange = exchange(
        "#1=GEOMETRIC_SET('',(#2,#3));#2=GEOMETRIC_SET('',(#1));#3=CARTESIAN_POINT('',(0.,0.,0.));",
    );
    crate::test_support::with_service_context(b"", |_, ctx| {
        assert!(matches!(
            super::super::style_domain(1, &exchange, ctx).expect("cycle walk"),
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
        let (bodies, supported) =
            super::super::invisible_body_ids(1, &exchange, &topology.value, &indices, ctx)
                .expect("cycle walk");
        assert!(!supported);
        assert_eq!(bodies.len(), 1);
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
