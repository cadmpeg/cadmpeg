// SPDX-License-Identifier: Apache-2.0
//! Presentation identity index admissions.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use std::collections::BTreeSet;
use std::collections::HashSet;

fn index_refuses(operation: &'static str, retained: bool) {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    if retained {
        policy.limits.max_retained_bytes = 16;
    } else {
        policy.limits.max_collection_items = 0;
    }
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits policy");
    let result = super::super::collect_identity_indices(["step:model:item#1"], &ctx, operation);
    assert!(matches!(
        result,
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == (if retained { ResourceDimension::RetainedBytes } else { ResourceDimension::CollectionItems })
                && refusal.operation == operation
    ));
}

fn ordered_set_refuses(operation: &'static str) {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits policy");
    let mut values = BTreeSet::new();
    let result = ctx
        .insert_btree_set(&mut values, 1_u64, operation)
        .map(|_| ());
    assert!(matches!(
        result,
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == operation
    ));
}

fn vector_refuses(operation: &'static str) {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits policy");
    let mut values = Vec::new();
    let result = ctx.push_vec(&mut values, 1_u64, operation);
    assert!(matches!(
        result,
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == operation
    ));
}

fn style_target_refuses(operation: &str, depth_limit: bool) {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=GEOMETRIC_SET('',(#2));#2=CARTESIAN_POINT('',(0.,0.,0.));ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("style set exchange");
    let refused = (0..=8).any(|limit| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        if depth_limit {
            policy.limits.max_recursion_depth = limit;
        } else {
            policy.limits.max_collection_items = limit;
        }
        let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy)
            .expect("root fits style target policy");
        matches!(
            super::super::expand_style_targets(1, &exchange, &mut std::collections::BTreeSet::new(), &mut BTreeSet::new(), 0, 128, &ctx),
            Err(CodecError::ResourceLimit(refusal))
                if refusal.operation == operation
                    && refusal.dimension == if depth_limit { ResourceDimension::RecursionDepth } else { ResourceDimension::CollectionItems }
        )
    });
    assert!(refused, "no refusal for {operation}");
}

#[test]
fn presentation_face_indices_refuse_collection_limit() {
    index_refuses("step_presentation_face_indices", false);
}

#[test]
fn presentation_face_identity_refuses_retained_limit() {
    index_refuses("step_presentation_face_indices", true);
}

#[test]
fn presentation_body_indices_refuse_collection_limit() {
    index_refuses("step_presentation_body_indices", false);
}

#[test]
fn presentation_body_identity_refuses_retained_limit() {
    index_refuses("step_presentation_body_indices", true);
}

#[test]
fn presentation_hidden_layer_ids_refuse_collection_limit() {
    ordered_set_refuses("step_presentation_hidden_layer_ids");
}

#[test]
fn presentation_invisibility_layer_targets_refuse_collection_limit() {
    ordered_set_refuses("step_presentation_invisibility_layer_targets");
}

#[test]
fn presentation_hidden_style_ids_refuse_collection_limit() {
    ordered_set_refuses("step_presentation_hidden_style_ids");
}

#[test]
fn presentation_invisibility_style_targets_refuse_collection_limit() {
    ordered_set_refuses("step_presentation_invisibility_style_targets");
}

#[test]
fn presentation_style_ids_refuse_collection_limit() {
    vector_refuses("step_presentation_style_ids");
}

#[test]
fn presentation_overridden_styles_refuse_collection_limit() {
    ordered_set_refuses("step_presentation_overridden_styles");
}

#[test]
fn presentation_style_references_refuse_collection_limit() {
    vector_refuses("step_presentation_style_references");
}

#[test]
fn presentation_context_style_ids_refuse_collection_limit() {
    ordered_set_refuses("step_presentation_context_style_ids");
}

#[test]
fn presentation_typed_claims_refuse_collection_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits policy");
    let result = ctx
        .insert_hash_set(&mut HashSet::new(), 1, "step_presentation_typed_claims")
        .map(|_| ());
    assert!(matches!(
        result,
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_presentation_typed_claims"
    ));
}

#[test]
fn presentation_style_target_active_refuses_collection_limit() {
    style_target_refuses("step_presentation_style_target_active", false);
}

#[test]
fn presentation_style_target_items_refuse_collection_limit() {
    style_target_refuses("step_presentation_style_target_items", false);
}

#[test]
fn presentation_style_target_walk_refuses_depth_limit() {
    style_target_refuses("step_presentation_style_target_walk", true);
}

#[test]
fn presentation_losses_refuse_collection_limit() {
    vector_refuses("step_presentation_losses");
}

fn transparency_refuses(operation: &str, retained: bool) {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=SURFACE_STYLE_RENDERING_WITH_PROPERTIES($,(#2,#3));#2=SURFACE_STYLE_TRANSPARENT(0.2);#3=SURFACE_STYLE_TRANSPARENT(0.5);ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("transparency exchange");
    let record = exchange.records().get(&1).expect("rendering record");
    let refused = if retained {
        {
            let error = cadmpeg_test_support::refusal::resource_limit_at(
                ResourceDimension::RetainedBytes,
                operation,
                |limit| {
                    let arena = DecodeArena::new();
                    let mut policy = DecodePolicy::service();
                    if retained {
                        policy.limits.max_retained_bytes = limit;
                    } else {
                        policy.limits.max_collection_items = limit;
                    }
                    let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy)
                        .expect("root fits policy");

                    super::super::surface_transparency(1, record, &exchange, &mut Vec::new(), &ctx)
                        .map(|_| ())
                },
            );
            matches!(Err::<(), CodecError>(error), Err(CodecError::ResourceLimit(refusal)) if refusal.operation == operation)
        }
    } else {
        (0..=4).any(|limit| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            if retained {
                policy.limits.max_retained_bytes = limit;
            } else {
                policy.limits.max_collection_items = limit;
            }
            let (ctx, _) =
                DecodeContext::from_root_bytes(source, &arena, &policy).expect("root fits policy");
            matches!(
                super::super::surface_transparency(1, record, &exchange, &mut Vec::new(), &ctx),
                Err(CodecError::ResourceLimit(refusal)) if refusal.operation == operation
            )
        })
    };
    assert!(refused, "no refusal for {operation}");
}

#[test]
fn presentation_transparency_candidates_refuse_collection_limit() {
    transparency_refuses("step_presentation_transparency_candidates", false);
}

#[test]
fn presentation_transparency_conflict_text_refuses_retained_limit() {
    transparency_refuses("step_presentation_transparency_conflict_text", true);
}

fn invalid_side_refuses(operation: &str, retained: bool) {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=SURFACE_STYLE_USAGE(.UNKNOWN.,#2);#2=ITEM();ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("surface side exchange");
    let record = exchange.records().get(&1).expect("style usage record");
    let refused = if retained {
        {
            let error = cadmpeg_test_support::refusal::resource_limit_at(
                ResourceDimension::RetainedBytes,
                operation,
                |limit| {
                    let arena = DecodeArena::new();
                    let mut policy = DecodePolicy::service();
                    if retained {
                        policy.limits.max_retained_bytes = limit;
                    } else {
                        policy.limits.max_collection_items = limit;
                    }
                    let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy)
                        .expect("root fits policy");

                    let result = (super::super::surface_side_rank(
                        1,
                        record,
                        &mut Vec::new(),
                        &mut BTreeSet::new(),
                        &std::cell::RefCell::new(
                            ctx.reserve_scoped(0, "step invalid surface side storage")
                                .unwrap(),
                        ),
                        &ctx,
                    ))
                    .map(|_| ());
                    result
                },
            );
            matches!(Err::<(), CodecError>(error), Err(CodecError::ResourceLimit(refusal)) if refusal.operation == operation)
        }
    } else {
        (0..=2).any(|limit| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        if retained {
            policy.limits.max_retained_bytes = limit;
        } else {
            policy.limits.max_collection_items = limit;
        }
        let (ctx, _) =
            DecodeContext::from_root_bytes(source, &arena, &policy).expect("root fits policy");
        let result = matches!(
            super::super::surface_side_rank(1, record, &mut Vec::new(), &mut BTreeSet::new(), &std::cell::RefCell::new(ctx.reserve_scoped(0, "step invalid surface side storage").unwrap()), &ctx),
            Err(CodecError::ResourceLimit(refusal)) if refusal.operation == operation
        );
        result
    })
    };
    assert!(refused, "no refusal for {operation}");
}

#[test]
fn presentation_invalid_surface_sides_refuse_collection_limit() {
    invalid_side_refuses("step_presentation_invalid_surface_sides", false);
}

#[test]
fn presentation_invalid_surface_side_text_refuses_retained_limit() {
    invalid_side_refuses("step_presentation_invalid_surface_side_text", true);
}

fn style_graph_refuses(
    operation: &str,
    depth: bool,
    run: impl Fn(&crate::parse::Exchange, &DecodeContext<'_>) -> Result<(), CodecError>,
) {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=OVER_RIDING_STYLED_ITEM('',(),#2,#3);#2=CARTESIAN_POINT('',(0.,0.,0.));#3=STYLED_ITEM('',(),#2);#4=GEOMETRIC_SET('',(#2));#5=NULL_STYLE();ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("style graph exchange");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    if depth {
        policy.limits.max_recursion_depth = 0;
    } else {
        policy.limits.max_collection_items = 0;
    }
    let (ctx, _) =
        DecodeContext::from_root_bytes(source, &arena, &policy).expect("root fits graph policy");
    assert!(matches!(
        run(&exchange, &ctx),
        Err(CodecError::ResourceLimit(refusal)) if refusal.operation == operation
    ));
}

#[test]
fn presentation_style_depth_active_refuses_collection_limit() {
    style_graph_refuses(
        "step_presentation_style_depth_active",
        false,
        |exchange, ctx| super::super::style_application_order(1, exchange, 128, ctx).map(|_| ()),
    );
}

#[test]
fn presentation_style_depth_walk_refuses_depth_limit() {
    style_graph_refuses(
        "step_presentation_style_depth_walk",
        true,
        |exchange, ctx| super::super::style_application_order(1, exchange, 128, ctx).map(|_| ()),
    );
}

#[test]
fn presentation_style_domain_active_refuses_collection_limit() {
    style_graph_refuses(
        "step_presentation_style_domain_active",
        false,
        |exchange, ctx| super::super::style_domain(4, exchange, ctx).map(|_| ()),
    );
}

#[test]
fn presentation_style_domain_walk_refuses_depth_limit() {
    style_graph_refuses(
        "step_presentation_style_domain_walk",
        true,
        |exchange, ctx| super::super::style_domain(4, exchange, ctx).map(|_| ()),
    );
}

#[test]
fn presentation_hidden_style_active_refuses_collection_limit() {
    style_graph_refuses(
        "step_presentation_hidden_style_active",
        false,
        |exchange, ctx| {
            super::super::style_is_hidden(1, &BTreeSet::new(), exchange, &mut BTreeSet::new(), ctx)
                .map(|_| ())
        },
    );
}

#[test]
fn presentation_hidden_style_walk_refuses_depth_limit() {
    style_graph_refuses(
        "step_presentation_hidden_style_walk",
        true,
        |exchange, ctx| {
            super::super::style_is_hidden(1, &BTreeSet::new(), exchange, &mut BTreeSet::new(), ctx)
                .map(|_| ())
        },
    );
}

#[test]
fn presentation_style_inheritance_active_refuses_collection_limit() {
    style_graph_refuses(
        "step_presentation_style_inheritance_active",
        false,
        |exchange, ctx| {
            super::super::style_inherits_from(1, 3, exchange, &mut BTreeSet::new(), ctx).map(|_| ())
        },
    );
}

#[test]
fn presentation_style_inheritance_walk_refuses_depth_limit() {
    style_graph_refuses(
        "step_presentation_style_inheritance_walk",
        true,
        |exchange, ctx| {
            super::super::style_inherits_from(1, 3, exchange, &mut BTreeSet::new(), ctx).map(|_| ())
        },
    );
}

#[test]
fn presentation_null_style_visited_refuses_collection_limit() {
    style_graph_refuses(
        "step_presentation_null_style_visited",
        false,
        |exchange, ctx| {
            super::super::contains_null_style(
                &crate::parse::Value::Reference(5),
                exchange,
                &mut BTreeSet::new(),
                0,
                ctx,
            )
            .map(|_| ())
        },
    );
}

#[test]
fn presentation_null_style_walk_refuses_depth_limit() {
    style_graph_refuses(
        "step_presentation_null_style_walk",
        true,
        |exchange, ctx| {
            super::super::contains_null_style(
                &crate::parse::Value::Reference(5),
                exchange,
                &mut BTreeSet::new(),
                0,
                ctx,
            )
            .map(|_| ())
        },
    );
}

#[test]
fn presentation_predefined_color_matches_ascii_case_without_copy() {
    assert_eq!(
        super::super::predefined(&cadmpeg_test_support::service_decode_context(), "ReD").unwrap(),
        super::super::predefined(&cadmpeg_test_support::service_decode_context(), "red").unwrap()
    );
}

#[test]
fn presentation_deferred_invisibility_refuses_collection_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits policy");
    let result = ctx
        .insert_btree_map(
            &mut std::collections::BTreeMap::new(),
            1_u64,
            true,
            "step_presentation_deferred_invisibility",
        )
        .map(|_| ());
    assert!(matches!(
        result,
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_presentation_deferred_invisibility"
    ));
}

#[test]
fn presentation_layer_items_refuse_collection_limit() {
    vector_refuses("step_presentation_layer_items");
}

#[test]
fn presentation_layer_records_refuse_collection_limit() {
    vector_refuses("step_presentation_layer_records");
}

fn invisible_body_refuses(operation: &str, depth: bool) {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("invisibility exchange");
    let setup_arena = DecodeArena::new();
    let (setup_ctx, _) =
        DecodeContext::from_root_bytes(source, &setup_arena, &DecodePolicy::default())
            .expect("setup root");
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    let index = crate::reader::index::CarrierIndex::from_ir(&ir, &setup_ctx).expect("setup index");
    let topology = crate::reader::topology::decode(&exchange, &mut ir, &index, &setup_ctx)
        .expect("setup topology");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    if depth {
        policy.limits.max_recursion_depth = 0;
    } else {
        policy.limits.max_collection_items = 0;
    }
    let (ctx, _) =
        DecodeContext::from_root_bytes(source, &arena, &policy).expect("root fits policy");
    assert!(matches!(
        super::super::invisible_body_ids(1, &exchange, &topology.value, &std::collections::BTreeMap::new(), &ctx),
        Err(CodecError::ResourceLimit(refusal)) if refusal.operation == operation
    ));
}

#[test]
fn presentation_invisible_body_active_refuses_collection_limit() {
    invisible_body_refuses("step_presentation_invisible_body_active", false);
}

#[test]
fn presentation_invisible_body_walk_refuses_depth_limit() {
    invisible_body_refuses("step_presentation_invisible_body_walk", true);
}

#[test]
fn presentation_invisible_body_ids_refuse_collection_limit() {
    ordered_set_refuses("step_presentation_invisible_body_ids");
}

#[test]
fn presentation_appearance_targets_refuse_collection_limit() {
    vector_refuses("step_presentation_appearance_targets");
}

#[test]
fn presentation_appearance_records_refuse_collection_limit() {
    vector_refuses("step_presentation_appearance_records");
}

#[test]
fn presentation_appearance_ids_refuse_collection_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits policy");
    let result = ctx
        .insert_btree_map(
            &mut std::collections::BTreeMap::new(),
            1_u64,
            true,
            "step_presentation_appearance_ids",
        )
        .map(|_| ());
    assert!(matches!(
        result,
        Err(CodecError::ResourceLimit(refusal))
            if refusal.operation == "step_presentation_appearance_ids"
    ));
}

#[test]
fn presentation_appearance_bindings_refuse_collection_limit() {
    vector_refuses("step_presentation_appearance_bindings");
}

fn scalar_candidate_refuses(operation: &str, retained: bool) {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    if retained {
        policy.limits.max_retained_bytes = 0;
    } else if operation == "step_presentation_scalar_color_members" {
        policy.limits.max_collection_items = 1;
    } else {
        policy.limits.max_collection_items = 0;
    }
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits policy");
    let body = cadmpeg_ir::ids::BodyId::mint("step:model:body#1").expect("body ID");
    let color = cadmpeg_ir::topology::Color::new(1.0, 0.0, 0.0, 1.0).expect("color");
    let result = super::super::push_scalar_candidate(
        &mut std::collections::HashMap::new(),
        &cadmpeg_ir::appearance::AppearanceTarget::Body(body),
        1,
        color,
        &ctx,
    );
    assert!(matches!(
        result,
        Err(CodecError::ResourceLimit(refusal)) if refusal.operation == operation
    ));
}

#[test]
fn presentation_scalar_target_identity_refuses_retained_limit() {
    scalar_candidate_refuses("step_presentation_scalar_target_identity", true);
}

#[test]
fn presentation_scalar_color_groups_refuse_collection_limit() {
    scalar_candidate_refuses("step_presentation_scalar_color_groups", false);
}

#[test]
fn presentation_scalar_color_members_refuse_collection_limit() {
    scalar_candidate_refuses("step_presentation_scalar_color_members", false);
}

#[test]
fn presentation_distinct_colors_refuse_collection_limit() {
    vector_refuses("step_presentation_distinct_colors");
}

#[test]
fn presentation_context_style_text_refuses_retained_limit() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=PRESENTATION_STYLE_BY_CONTEXT(#2);#2=ITEM();ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("context style exchange");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 1;
    let (ctx, _) =
        DecodeContext::from_root_bytes(source, &arena, &policy).expect("root fits policy");
    assert!(matches!(
        super::super::context_style_message(3, &BTreeSet::from([1]), &exchange, &ctx),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RetainedBytes
                && refusal.operation == "step_presentation_context_style_text"
    ));
    assert!(crate::test_support::with_service_context(b"", |_, ctx| {
        super::super::context_style_message(3, &BTreeSet::from([1]), &exchange, ctx)
    })
    .expect("local context text")
    .contains("#1 in #2"));
}

#[test]
fn presentation_scalar_conflict_text_refuses_retained_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 1;
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits policy");
    let target = cadmpeg_ir::appearance::AppearanceTarget::Body(
        cadmpeg_ir::ids::BodyId::mint("step:model:body#1").expect("body ID"),
    );
    let color = cadmpeg_ir::topology::Color::new(1.0, 0.0, 0.0, 1.0).expect("color");
    let candidates = [(2, color), (3, color)];
    assert!(matches!(
        super::super::scalar_conflict_message(&candidates, &target, &ctx),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RetainedBytes
                && refusal.operation == "step_presentation_scalar_conflict_text"
    ));
    assert!(crate::test_support::with_service_context(b"", |_, ctx| {
        super::super::scalar_conflict_message(&candidates, &target, ctx)
    })
    .expect("local conflict text")
    .contains("#2, #3"));
}

fn color_search_refuses(
    operation: &str,
    collection_limit: u64,
    retained_limit: u64,
    depth_limit: u64,
) {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=COLOUR_RGB('red',1.,0.,0.);ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("colour exchange");
    let dimension = if operation == "step_presentation_color_cache_value" {
        ResourceDimension::MaterializedBytes
    } else if retained_limit != 100 {
        ResourceDimension::RetainedBytes
    } else if depth_limit == 0 {
        ResourceDimension::RecursionDepth
    } else {
        ResourceDimension::CollectionItems
    };
    let error = cadmpeg_test_support::refusal::resource_limit_at(dimension, operation, |cap| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = collection_limit;
        if dimension == ResourceDimension::MaterializedBytes {
            policy.limits.max_materialized_bytes = cap;
        } else if dimension == ResourceDimension::RetainedBytes {
            policy.limits.max_retained_bytes = cap;
        } else {
            policy.limits.max_retained_bytes = u64::MAX;
            policy.limits.max_collection_items = cap;
        }
        policy.limits.max_recursion_depth = if dimension == ResourceDimension::RecursionDepth {
            cap
        } else {
            depth_limit
        };
        let (ctx, _) =
            DecodeContext::from_root_bytes(source, &arena, &policy).expect("root fits policy");
        let result = (super::super::find_color(
            1,
            &exchange,
            super::super::StyleDomain::Any,
            super::super::ColorSearchState {
                storage: &std::cell::RefCell::new(
                    ctx.reserve_scoped(0, "color search fixture")
                        .expect("scope"),
                ),
                active: &mut BTreeSet::new(),
                cache: &mut std::collections::BTreeMap::new(),
                losses: &mut Vec::new(),
                invalid_surface_sides: &mut BTreeSet::new(),
            },
            0,
            &ctx,
        ))
        .map(|_| ());
        result
    });
    assert!(
        matches!(Err::<(), CodecError>(error), Err(CodecError::ResourceLimit(refusal)) if refusal.operation == operation)
    );
}

#[test]
fn presentation_color_active_refuses_collection_limit() {
    color_search_refuses("step_presentation_color_active", 0, 100, 128);
}

#[test]
fn presentation_color_walk_refuses_depth_limit() {
    color_search_refuses("step_presentation_color_walk", 100, 100, 0);
}

#[test]
fn presentation_color_cache_entries_refuse_collection_limit() {
    color_search_refuses("step_presentation_color_cache_entries", 1, 100, 128);
}

#[test]
fn presentation_color_cache_value_refuses_retained_limit() {
    color_search_refuses("step_presentation_color_cache_value", 100, 3, 128);
}

#[test]
fn presentation_color_cache_copy_refuses_retained_limit() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("cache exchange");
    let color = cadmpeg_ir::topology::Color::new(1.0, 0.0, 0.0, 1.0).expect("color");
    let mut cache = std::collections::BTreeMap::new();
    cache.insert(
        (1, super::super::StyleDomain::Any),
        Some(super::super::ColorResolution::Candidate(
            super::super::ColorCandidate {
                rank: super::super::SurfaceSideRank::NoUsage,
                id: 1,
                color,
                name: Some("red".into()),
            },
        )),
    );
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 2;
    let (ctx, _) =
        DecodeContext::from_root_bytes(source, &arena, &policy).expect("root fits policy");
    assert!(matches!(
        super::super::find_color(1, &exchange, super::super::StyleDomain::Any, super::super::ColorSearchState { storage: &std::cell::RefCell::new(ctx.reserve_scoped(0, "color search fixture").expect("scope")), active: &mut BTreeSet::new(), cache: &mut cache, losses: &mut Vec::new(), invalid_surface_sides: &mut BTreeSet::new() }, 0, &ctx),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RetainedBytes
                && refusal.operation == "step_presentation_color_cache_copy"
    ));
}

#[test]
fn predefined_color_case_equality_preserves_refusal() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
    let error = super::super::predefined(&ctx, "ReD").unwrap_err();
    let CodecError::ResourceLimit(refusal) = error else {
        panic!("color classification must return the refusal");
    };
    assert_eq!(
        refusal.operation,
        "STEP predefined color name case equality"
    );
    assert_eq!(ctx.resource_refusal(), Some(refusal));
}

#[test]
fn binding_style_number_parse_preserves_refusal() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=CARTESIAN_POINT('origin',(0.,0.,0.));#2=DIRECTION('normal',(0.,0.,1.));#3=DIRECTION('u',(1.,0.,0.));#4=AXIS2_PLACEMENT_3D('placement',#1,#2,#3);#5=PLANE('surface',#4);#6=COLOUR_RGB('red',1.,0.,0.);#7=SURFACE_STYLE_RENDERING(#6,$,$,$,$,$);#8=PRESENTATION_STYLE_ASSIGNMENT((#7));#9=STYLED_ITEM('',(#8),#5);#10=INVISIBILITY((#9));ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner).unwrap();
    let setup = cadmpeg_test_support::service_decode_context();
    let mut ir = cadmpeg_ir::CadIr::empty();
    crate::reader::geometry::decode(&exchange, &mut ir, &setup).unwrap();
    let index = crate::reader::index::CarrierIndex::from_ir(&ir, &setup).unwrap();
    let topology = crate::reader::topology::decode(&exchange, &mut ir, &index, &setup).unwrap();
    cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "STEP binding style number parse",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy).unwrap();
            let result = super::super::decode(
                &exchange,
                &topology.value,
                &mut ir.clone(),
                &std::collections::BTreeMap::new(),
                &ctx,
            )
            .map(|_| ());
            if let Err(CodecError::ResourceLimit(refusal)) = &result {
                assert_eq!(ctx.resource_refusal().as_ref(), Some(refusal));
            }
            result
        },
    );
}

#[test]
fn style_domain_point_containment_preserves_refusal() {
    let (exchange, _) = crate::test_support::with_service_context(b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=A_POINT();ENDSEC;END-ISO-10303-21;", crate::parse::parse_inner).unwrap();
    cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "STEP style domain point containment",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
            let result = super::super::style_domain(1, &exchange, &ctx).map(|_| ());
            if let Err(CodecError::ResourceLimit(refusal)) = &result {
                assert_eq!(ctx.resource_refusal().as_ref(), Some(refusal));
            }
            result
        },
    );
}

#[test]
fn style_domain_vertex_containment_preserves_refusal() {
    let (exchange, _) = crate::test_support::with_service_context(b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=A_VERTEX();ENDSEC;END-ISO-10303-21;", crate::parse::parse_inner).unwrap();
    cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "STEP style domain vertex containment",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
            let result = super::super::style_domain(1, &exchange, &ctx).map(|_| ());
            if let Err(CodecError::ResourceLimit(refusal)) = &result {
                assert_eq!(ctx.resource_refusal().as_ref(), Some(refusal));
            }
            result
        },
    );
}

#[test]
fn style_domain_curve_containment_preserves_refusal() {
    let (exchange, _) = crate::test_support::with_service_context(b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=A_CURVE();ENDSEC;END-ISO-10303-21;", crate::parse::parse_inner).unwrap();
    cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "STEP style domain curve containment",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
            let result = super::super::style_domain(1, &exchange, &ctx).map(|_| ());
            if let Err(CodecError::ResourceLimit(refusal)) = &result {
                assert_eq!(ctx.resource_refusal().as_ref(), Some(refusal));
            }
            result
        },
    );
}

#[test]
fn style_domain_edge_containment_preserves_refusal() {
    let (exchange, _) = crate::test_support::with_service_context(b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=A_EDGE();ENDSEC;END-ISO-10303-21;", crate::parse::parse_inner).unwrap();
    cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "STEP style domain edge containment",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
            let result = super::super::style_domain(1, &exchange, &ctx).map(|_| ());
            if let Err(CodecError::ResourceLimit(refusal)) = &result {
                assert_eq!(ctx.resource_refusal().as_ref(), Some(refusal));
            }
            result
        },
    );
}

#[test]
fn style_domain_line_containment_preserves_refusal() {
    let (exchange, _) = crate::test_support::with_service_context(b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=A__LINE();ENDSEC;END-ISO-10303-21;", crate::parse::parse_inner).unwrap();
    cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "STEP style domain line containment",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
            let result = super::super::style_domain(1, &exchange, &ctx).map(|_| ());
            if let Err(CodecError::ResourceLimit(refusal)) = &result {
                assert_eq!(ctx.resource_refusal().as_ref(), Some(refusal));
            }
            result
        },
    );
}

#[test]
fn style_domain_face_containment_preserves_refusal() {
    let (exchange, _) = crate::test_support::with_service_context(b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=A_FACE();ENDSEC;END-ISO-10303-21;", crate::parse::parse_inner).unwrap();
    cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "STEP style domain face containment",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
            let result = super::super::style_domain(1, &exchange, &ctx).map(|_| ());
            if let Err(CodecError::ResourceLimit(refusal)) = &result {
                assert_eq!(ctx.resource_refusal().as_ref(), Some(refusal));
            }
            result
        },
    );
}

#[test]
fn style_domain_surface_containment_preserves_refusal() {
    let (exchange, _) = crate::test_support::with_service_context(b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=A_SOLID();ENDSEC;END-ISO-10303-21;", crate::parse::parse_inner).unwrap();
    cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "STEP style domain surface containment",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
            let result = super::super::style_domain(1, &exchange, &ctx).map(|_| ());
            if let Err(CodecError::ResourceLimit(refusal)) = &result {
                assert_eq!(ctx.resource_refusal().as_ref(), Some(refusal));
            }
            result
        },
    );
}

#[test]
fn style_domain_surface_name_stops_at_face_containment() {
    let (exchange, _) = crate::test_support::with_service_context(b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=A_SURFACE();ENDSEC;END-ISO-10303-21;", crate::parse::parse_inner).unwrap();
    cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "STEP style domain face containment",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
            let result = super::super::style_domain(1, &exchange, &ctx).map(|_| ());
            if let Err(CodecError::ResourceLimit(refusal)) = &result {
                assert_eq!(ctx.resource_refusal().as_ref(), Some(refusal));
            }
            result
        },
    );
    assert!(matches!(
        super::super::style_domain(
            1,
            &exchange,
            &cadmpeg_test_support::service_decode_context()
        )
        .unwrap(),
        super::super::StyleDomain::Surface
    ));
}

#[test]
fn style_domain_solid_containment_preserves_refusal() {
    let (exchange, _) = crate::test_support::with_service_context(b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=A_SOLID();ENDSEC;END-ISO-10303-21;", crate::parse::parse_inner).unwrap();
    cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "STEP style domain solid containment",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
            let result = super::super::style_domain(1, &exchange, &ctx).map(|_| ());
            if let Err(CodecError::ResourceLimit(refusal)) = &result {
                assert_eq!(ctx.resource_refusal().as_ref(), Some(refusal));
            }
            result
        },
    );
}

#[test]
fn style_domain_shell_containment_preserves_refusal() {
    let (exchange, _) = crate::test_support::with_service_context(b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=A_SHELL();ENDSEC;END-ISO-10303-21;", crate::parse::parse_inner).unwrap();
    cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "STEP style domain shell containment",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
            let result = super::super::style_domain(1, &exchange, &ctx).map(|_| ());
            if let Err(CodecError::ResourceLimit(refusal)) = &result {
                assert_eq!(ctx.resource_refusal().as_ref(), Some(refusal));
            }
            result
        },
    );
}
