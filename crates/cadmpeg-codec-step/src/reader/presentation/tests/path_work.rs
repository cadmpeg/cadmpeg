// SPDX-License-Identifier: Apache-2.0
//! Diamond graphs preserve path output and fuse traversal refusals.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::CadIr;

use super::super::{ColorCache, ColorSearchState, InvisibleIndex, StyleDomain, StyleDomainIndex};

fn diamond(name: &str, levels: u64, leaves: &str) -> crate::parse::Exchange {
    let mut records = String::new();
    for id in 1..=levels * 2 {
        let next = ((id - 1) / 2 + 1) * 2 + 1;
        write!(records, "#{id}={name}('',(#{next},#{}));", next + 1).expect("diamond edge");
    }
    records.push_str(leaves);
    let source = format!("ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;{records}ENDSEC;END-ISO-10303-21;");
    crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
        .expect("diamond exchange")
        .0
}

fn refuses_step(
    operation: &str,
    mut walk: impl FnMut(&DecodeContext<'_>) -> Result<(), CodecError>,
) {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        operation,
        |limit| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = limit;
            let (ctx, _) =
                DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root");
            let result = walk(&ctx);
            let CodecError::ResourceLimit(original) =
                result.expect_err("diamond traversal refuses")
            else {
                panic!("work refusal")
            };
            assert_eq!(ctx.resource_refusal(), Some(original));
            assert!(
                matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(found)) if found == original)
            );
            Err::<(), _>(CodecError::ResourceLimit(original))
        },
    );
    let CodecError::ResourceLimit(limit) = error else {
        panic!("work refusal")
    };
    assert!(limit.limit < 1_000_000, "modest walk budget: {limit:?}");
}

#[test]
fn diamond_style_targets_refuse_the_visited_edge_and_preserve_duplicates() {
    let exchange = diamond("GEOMETRIC_SET", 8, "#17=ITEM();#18=ITEM();");
    crate::test_support::with_service_context(b"", |_, ctx| {
        let mut claim_storage = ctx.reserve_scoped(0, "diamond claims").expect("scope");
        let mut active_storage = ctx.reserve_scoped(0, "diamond active").expect("scope");
        let mut typed = BTreeSet::new();
        let mut active = BTreeSet::new();
        let mut targets = Vec::new();
        super::super::expand_style_targets(
            1,
            &exchange,
            (&mut typed, &mut claim_storage),
            (&mut active, &mut active_storage),
            (0, 128, None),
            &mut |id| ctx.push_vec(&mut targets, id, "diamond emitted targets"),
            ctx,
        )
        .expect("all diamond paths");
        assert_eq!(targets, [17, 18].repeat(128));
        assert_eq!(typed, (1..=16).filter(|id| *id != 2).collect());
        assert!(active.is_empty());
    });
    refuses_step("STEP style target edge", |ctx| {
        let mut claims = ctx.reserve_scoped(0, "diamond claims")?;
        let mut active = ctx.reserve_scoped(0, "diamond active")?;
        super::super::expand_style_targets(
            1,
            &exchange,
            (&mut BTreeSet::new(), &mut claims),
            (&mut BTreeSet::new(), &mut active),
            (0, 128, None),
            &mut |_| Ok(()),
            ctx,
        )
        .map(|_| ())
    });
}

#[test]
fn diamond_color_walk_refuses_its_parameter_visit() {
    let exchange = diamond(
        "ITEM",
        8,
        "#17=COLOUR_RGB('',1.,0.,0.);#18=SURFACE_STYLE_USAGE($,#17);",
    );
    refuses_step("STEP color parameter traversal", |ctx| {
        let storage = RefCell::new(ctx.reserve_scoped(0, "diamond color")?);
        let reports = RefCell::new(ctx.reserve_scoped(0, "diamond reports")?);
        super::super::find_color(
            1,
            &exchange,
            StyleDomain::Surface,
            ColorSearchState {
                storage: &storage,
                active: &mut BTreeSet::new(),
                cache: &mut ColorCache::default(),
                completed: None,
                losses: (&mut Vec::new(), &reports),
                invalid_surface_sides: &mut BTreeSet::new(),
            },
            0,
            None,
            ctx,
        )
        .map(|_| ())
    });
}

#[test]
fn diamond_null_style_walk_refuses_its_parameter_visit() {
    let exchange = diamond("ITEM", 8, "#17=ITEM();#18=ITEM();");
    refuses_step("STEP record parameter traversal", |ctx| {
        let mut storage = ctx.reserve_scoped(0, "diamond null style")?;
        storage
            .with_storage(|| {
                super::super::contains_null_style(
                    &crate::parse::Value::Reference(1),
                    &exchange,
                    (
                        &mut BTreeSet::new(),
                        &mut BTreeMap::new(),
                        &mut ctx.reserve_scoped(0, "diamond null completions")?,
                    ),
                    0,
                    ctx,
                )
            })
            .map(|_| ())
    });
}

#[test]
fn diamond_style_domain_walk_refuses_its_member_visit() {
    let exchange = diamond(
        "GEOMETRIC_SET",
        8,
        "#17=CARTESIAN_POINT('',(0.,0.,0.));#18=CARTESIAN_POINT('',(0.,0.,0.));",
    );
    refuses_step("STEP style domain member traversal", |ctx| {
        StyleDomainIndex::new(ctx)?.domain(1, &exchange).map(|_| ())
    });
}

#[test]
fn diamond_invisibility_walk_refuses_its_item_visit() {
    let exchange = diamond("REPRESENTATION", 8, "#17=ITEM();#18=ITEM();");
    let mut ir = CadIr::empty();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (topology_ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root");
    let carriers =
        crate::reader::index::CarrierIndex::from_ir(&ir, &topology_ctx).expect("carriers");
    let topology = crate::reader::topology::decode(&exchange, &mut ir, &carriers, &topology_ctx)
        .expect("topology")
        .value;
    refuses_step("STEP representation item traversal", |ctx| {
        InvisibleIndex::new(ctx)?
            .resolve(1, &exchange, &topology, &BTreeMap::new())
            .map(|_| ())
    });
}

#[test]
fn empty_diamond_targets_reuse_completions_without_emitting_paths() {
    let exchange = diamond(
        "GEOMETRIC_SET",
        16,
        "#33=GEOMETRIC_SET('',());#34=GEOMETRIC_SET('',());",
    );
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 128;
    crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
        let mut claims = ctx
            .reserve_scoped(0, "empty diamond claims")
            .expect("scope");
        let mut complete_storage = ctx
            .reserve_scoped(0, "empty diamond completions")
            .expect("scope");
        let mut active_storage = ctx
            .reserve_scoped(0, "empty diamond active")
            .expect("scope");
        let mut typed = BTreeSet::new();
        let mut complete = BTreeMap::new();
        let mut active = BTreeSet::new();
        for id in [1, 2, 1] {
            super::super::expand_style_targets(
                id,
                &exchange,
                (&mut typed, &mut claims),
                (&mut active, &mut active_storage),
                (0, 128, Some((&mut complete, &mut complete_storage))),
                &mut |_| panic!("empty diamond emits no terminal"),
                ctx,
            )
            .expect("empty graph costs nodes and edges");
        }
        assert_eq!(typed, (1..=34).collect());
        assert_eq!(complete.len(), 34);
        assert!(active.is_empty());
    });
}

#[test]
fn no_null_diamond_reuses_completed_negative_searches() {
    let exchange = diamond("ITEM", 16, "#33=ITEM();#34=ITEM();");
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 128;
    crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
        let mut complete_storage = ctx
            .reserve_scoped(0, "null diamond completions")
            .expect("scope");
        let mut complete = BTreeMap::new();
        for id in [1, 2, 1] {
            let mut visited_storage = ctx
                .reserve_scoped(0, "null diamond visited")
                .expect("scope");
            let mut visited = BTreeSet::new();
            let found = visited_storage
                .with_storage(|| {
                    super::super::contains_null_style(
                        &crate::parse::Value::Reference(id),
                        &exchange,
                        (&mut visited, &mut complete, &mut complete_storage),
                        0,
                        ctx,
                    )
                })
                .expect("shared negative search")
                .0;
            assert!(!found);
        }
        assert_eq!(complete.len(), 34);
    });
}

#[test]
fn diamond_styles_preserve_binding_count_and_warning_multiplicity() {
    let exchange = diamond("GEOMETRIC_SET", 8, "#17=SOURCE_SURFACE();#18=SOURCE_SURFACE();#90=COLOUR_RGB('',1.,0.,0.);#91=STYLED_ITEM('',(#90),#1);");
    crate::test_support::with_service_context(b"", |_, ctx| {
        let mut ir = CadIr::empty();
        let carriers = crate::reader::index::CarrierIndex::from_ir(&ir, ctx).expect("carriers");
        let topology = crate::reader::topology::decode(&exchange, &mut ir, &carriers, ctx)
            .expect("topology")
            .value;
        let mut admitted = cadmpeg_core::decode::u64_from_index(ir.model.entity_count());
        let outcome = super::super::decode(&exchange, &topology, &mut ir, &BTreeMap::new(), ctx, &mut admitted)
            .expect("diamond style");
        assert_eq!(ir.model.appearances.len(), 1);
        assert_eq!(ir.model.appearance_bindings.len(), 256);
        assert!(outcome.losses.is_empty());
        let targets: Vec<_> = ir
            .model
            .appearance_bindings
            .iter()
            .map(|binding| &binding.target)
            .collect();
        let expected = [
            cadmpeg_ir::appearance::AppearanceTarget::Source {
                source_id: "#17".to_owned(),
            },
            cadmpeg_ir::appearance::AppearanceTarget::Source {
                source_id: "#18".to_owned(),
            },
        ];
        assert_eq!(targets, [&expected[0], &expected[1]].repeat(128));
    });
    let exchange = diamond("ITEM", 8, "#17=SURFACE_STYLE_USAGE($,#18);#18=COLOUR_RGB('',1.,0.,0.);#90=STYLED_ITEM('',(#1),#100);#91=STYLED_ITEM('',(#2),#101);#100=SOURCE_SURFACE();#101=SOURCE_SURFACE();");
    crate::test_support::with_service_context(b"", |_, ctx| {
        let mut ir = CadIr::empty();
        let carriers = crate::reader::index::CarrierIndex::from_ir(&ir, ctx).expect("carriers");
        let topology = crate::reader::topology::decode(&exchange, &mut ir, &carriers, ctx)
            .expect("topology")
            .value;
        let mut admitted = cadmpeg_core::decode::u64_from_index(ir.model.entity_count());
        let outcome = super::super::decode(&exchange, &topology, &mut ir, &BTreeMap::new(), ctx, &mut admitted)
            .expect("shared exceptional style");
        assert_eq!(ir.model.appearances.len(), 1);
        assert_eq!(ir.model.appearance_bindings.len(), 2);
        assert_eq!(outcome.losses.len(), 2);
        assert_eq!(outcome.losses[0], outcome.losses[1]);
        assert_eq!(
            outcome.losses[0].message,
            "SURFACE_STYLE_USAGE #17 has no valid surface_side; style omitted"
        );
    });
}

fn diamond_style_entities_refuse_before_publication(max_entities: u64) {
    let exchange = diamond("GEOMETRIC_SET", 8, "#17=SOURCE_SURFACE();#18=SOURCE_SURFACE();#90=COLOUR_RGB('',1.,0.,0.);#91=STYLED_ITEM('',(#90),#1);");
    let setup = cadmpeg_test_support::service_decode_context();
    let mut ir = CadIr::empty();
    let carriers = crate::reader::index::CarrierIndex::from_ir(&ir, &setup).expect("carriers");
    let topology = crate::reader::topology::decode(&exchange, &mut ir, &carriers, &setup)
        .expect("topology")
        .value;
    assert_eq!(ir.model.entity_count(), 0);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_entities = max_entities;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root");
    let mut admitted = 0;
    let result = super::super::decode(
        &exchange,
        &topology,
        &mut ir,
        &BTreeMap::new(),
        &ctx,
        &mut admitted,
    );
    let CodecError::ResourceLimit(original) = result.map(|_| ()).expect_err("next entity refuses") else {
        panic!("entity refusal");
    };
    assert_eq!(original.dimension, ResourceDimension::Entities);
    assert_eq!(original.operation, if max_entities == 0 {
        "step_presentation_appearance_entity"
    } else {
        "step_presentation_binding_entity"
    });
    assert_eq!((original.used, original.additional), (max_entities, 1));
    assert_eq!(admitted, max_entities);
    assert_eq!(ir.model.appearances.len(), usize::from(max_entities > 0));
    let expected_bindings = usize::try_from(max_entities).expect("small entity limit")
        - usize::from(max_entities > 0);
    assert_eq!(ir.model.appearance_bindings.len(), expected_bindings);
    for (ordinal, binding) in ir.model.appearance_bindings.iter().enumerate() {
        assert_eq!(binding.id.as_str(), format!("step:presentation:binding#91:{ordinal}-0"));
        assert_eq!(binding.target, cadmpeg_ir::appearance::AppearanceTarget::Source {
            source_id: format!("#{}", 17 + ordinal % 2),
        });
    }
    assert_eq!(ctx.resource_refusal(), Some(original));
    assert!(matches!(ctx.finish_session(),
        Err(CodecError::ResourceLimit(found)) if found == original));
}

#[test]
fn diamond_appearance_entity_refuses_before_publication() {
    diamond_style_entities_refuse_before_publication(0);
}

#[test]
fn diamond_binding_entities_refuse_before_unadmitted_publication() {
    for limit in [1, 2, 64, 256] {
        diamond_style_entities_refuse_before_publication(limit);
    }
}

#[test]
fn presentation_layer_entity_refuses_before_publication() {
    let exchange = diamond("GEOMETRIC_SET", 0,
        "#1=SOURCE_SURFACE();#2=PRESENTATION_LAYER_ASSIGNMENT('layer','',(#1));");
    let setup = cadmpeg_test_support::service_decode_context();
    let mut ir = CadIr::empty();
    let carriers = crate::reader::index::CarrierIndex::from_ir(&ir, &setup).expect("carriers");
    let topology = crate::reader::topology::decode(&exchange, &mut ir, &carriers, &setup)
        .expect("topology")
        .value;
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_entities = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root");
    let mut admitted = 0;
    let result = super::super::decode(
        &exchange, &topology, &mut ir, &BTreeMap::new(), &ctx, &mut admitted,
    );
    let CodecError::ResourceLimit(original) = result.map(|_| ()).expect_err("layer entity refuses") else {
        panic!("entity refusal");
    };
    assert_eq!(original.dimension, ResourceDimension::Entities);
    assert_eq!(original.operation, "step_presentation_layer_entity");
    assert_eq!((original.used, original.additional), (0, 1));
    assert_eq!(admitted, 0);
    assert!(ir.model.presentation_layers.is_empty());
    assert_eq!(ctx.resource_refusal(), Some(original));
    assert!(matches!(ctx.finish_session(),
        Err(CodecError::ResourceLimit(found)) if found == original));
}

fn cutoff_history_refuses(operation: &str, references: [u64; 2]) {
    let exchange = super::color_context::cutoff_history_exchange();
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        operation,
        |limit| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_recursion_depth = 1024;
            policy.limits.max_work_units = limit;
            let (ctx, _) =
                DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root");
            let mut colors = super::color_context::shared(&exchange, &ctx);
            let reports = RefCell::new(ctx.reserve_scoped(0, "history reports").expect("scope"));
            let mut losses = Vec::new();
            let result = colors
                .resolve(&[20], StyleDomain::Surface, (&mut losses, &reports))
                .map(|_| ())
                .and_then(|()| {
                    colors
                        .resolve(&references, StyleDomain::Surface, (&mut losses, &reports))
                        .map(|_| ())
                });
            let CodecError::ResourceLimit(original) = result.expect_err("history walk refuses")
            else {
                panic!("work refusal")
            };
            assert_eq!(ctx.resource_refusal(), Some(original));
            drop(losses);
            drop(reports);
            drop(colors);
            assert!(
                matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(found)) if found == original)
            );
            Err::<(), _>(CodecError::ResourceLimit(original))
        },
    );
    let CodecError::ResourceLimit(limit) = error else {
        panic!("work refusal")
    };
    // The fixed-depth chain owns its admitted frame storage. The exponential
    // diamond path count never enters this work limit.
    assert!(limit.limit < 32_000_000, "bounded history work: {limit:?}");
}

#[test]
fn diamond_color_history_walk_refuses_its_visited_ancestor() {
    cutoff_history_refuses("STEP shared color history step", [20, 10]);
}

#[test]
fn diamond_contextual_color_invalidation_refuses_its_visited_ancestor() {
    cutoff_history_refuses("STEP contextual color ancestor step", [10, 20]);
}

#[test]
fn diamond_color_dependency_index_refuses_its_actual_node_scan() {
    cutoff_history_refuses("STEP color dependency nodes", [20, 10]);
}

#[test]
fn diamond_shared_color_claim_walk_refuses_its_visited_node() {
    let exchange = diamond(
        "ITEM",
        8,
        "#17=COLOUR_RGB('',1.,0.,0.);#18=COLOUR_RGB('',1.,0.,0.);",
    );
    refuses_step("STEP shared color claim step", |ctx| {
        let mut colors = super::color_context::shared(&exchange, ctx);
        let reports = RefCell::new(ctx.reserve_scoped(0, "claim diamond reports")?);
        let mut losses = Vec::new();
        // The second root reuses the first root's completed descendants.
        colors
            .resolve(&[1], StyleDomain::Surface, (&mut losses, &reports))
            .map(|_| ())?;
        let query = colors
            .resolve(&[2], StyleDomain::Surface, (&mut losses, &reports))?
            .0;
        let mut claims = ctx.reserve_scoped(0, "diamond typed claims")?;
        colors.claim(
            query,
            StyleDomain::Surface,
            (&mut BTreeSet::new(), &mut claims),
        )
    });
}
