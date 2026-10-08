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
        let mut claim_storage = ctx.reserve_scoped(0, "claim fixture").expect("scope");
        let result = super::super::expand_style_targets(
            1,
            &exchange,
            (
                &mut std::collections::BTreeSet::new(),
                &mut claim_storage,
            ),
            &mut BTreeSet::new(),
            (0, 128),
            &mut |_| Ok(()),
            &ctx,
        );
        if let Err(CodecError::ResourceLimit(refusal)) = &result {
            assert_eq!(ctx.resource_refusal().as_ref(), Some(refusal));
        }
        let refused = matches!(
            result,
            Err(CodecError::ResourceLimit(refusal))
                if refusal.operation == operation
                    && refusal.dimension == if depth_limit { ResourceDimension::RecursionDepth } else { ResourceDimension::CollectionItems }
        );
        refused
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
fn presentation_style_target_traversal_refuses_work_limit() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=GEOMETRIC_SET('',(#2));#2=CARTESIAN_POINT('',(0.,0.,0.));ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("style set exchange");
    cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "STEP style target traversal",
        |limit| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = limit;
            let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy)
                .expect("root fits style target policy");
            let mut claim_storage = ctx.reserve_scoped(0, "claim fixture").expect("scope");
            let result = super::super::expand_style_targets(
                1,
                &exchange,
                (&mut BTreeSet::new(), &mut claim_storage),
                &mut BTreeSet::new(),
                (0, 128),
                &mut |_| ctx.charge_work(1, "STEP style target traversal"),
                &ctx,
            );
            if let Err(CodecError::ResourceLimit(refusal)) = &result {
                assert_eq!(ctx.resource_refusal().as_ref(), Some(refusal));
            }
            result
        },
    );
}

#[test]
fn presentation_style_target_visits_first_child_before_large_set_suffix() {
    let mut members = String::from("#2");
    for _ in 0..4096 {
        members.push_str(",$");
    }
    let source = format!(
        "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=GEOMETRIC_SET('',({members}));#2=GEOMETRIC_SET('',(#3));#3=CARTESIAN_POINT('',(0.,0.,0.));ENDSEC;END-ISO-10303-21;"
    );
    let (exchange, _) = crate::test_support::with_service_context(
        source.as_bytes(),
        crate::parse::parse_inner,
    )
    .expect("large style set exchange");

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // The 4,097 values exceed this cap if pre-admitted. The root's active and
    // typed B-tree insertions cost 696 work units each (three 232-byte node
    // passes); the first child then reaches the configured depth refusal.
    policy.limits.max_work_units = 2_048;
    policy.limits.max_recursion_depth = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
        .expect("empty root fits style target policy");
    let mut claim_storage = ctx.reserve_scoped(0, "claim fixture").expect("scope");
    let result = super::super::expand_style_targets(
        1,
        &exchange,
        (
            &mut BTreeSet::new(),
            &mut claim_storage,
        ),
        &mut BTreeSet::new(),
        (0, 128),
        &mut |_| Ok(()),
        &ctx,
    );
    let CodecError::ResourceLimit(refusal) = result.expect_err("first child exceeds depth") else {
        panic!("style child must refuse on depth");
    };
    assert_eq!(refusal.dimension, ResourceDimension::RecursionDepth);
    assert_eq!(refusal.operation, "step_presentation_style_target_walk");
    assert_eq!(ctx.resource_refusal(), Some(refusal));
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

                    let reports = std::cell::RefCell::new(
                        ctx.reserve_scoped(0, "report fixture").expect("scope"),
                    );
                    super::super::surface_transparency(
                        1,
                        record,
                        &exchange,
                        (&mut Vec::new(), &reports),
                        &ctx,
                    )
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
            let reports = std::cell::RefCell::new(ctx.reserve_scoped(0, "report fixture").expect("scope"));
            matches!(
                super::super::surface_transparency(1, record, &exchange, (&mut Vec::new(), &reports), &ctx),
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

#[test]
fn presentation_surface_transparency_visits_first_property_before_large_parameter_suffix() {
    let mut parameters = String::from("#2");
    for _ in 0..1024 {
        parameters.push_str(",$");
    }
    let source = format!(
        "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=SURFACE_STYLE_RENDERING_WITH_PROPERTIES({parameters});#2=SURFACE_STYLE_TRANSPARENT(0.2);ENDSEC;END-ISO-10303-21;"
    );
    let (exchange, _) = crate::test_support::with_service_context(
        source.as_bytes(),
        crate::parse::parse_inner,
    )
    .expect("large transparency parameter exchange");
    let record = exchange.records().get(&1).expect("rendering record");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // The source has 1,025 top-level parameters, so bulk admission exceeds the
    // work cap. The first parameter reaches a nested property visit first.
    policy.limits.max_work_units = 256;
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
        .expect("empty root fits transparency policy");
    let reports = std::cell::RefCell::new(
        ctx.reserve_scoped(0, "report fixture")
            .expect("report scope"),
    );
    let result = super::super::surface_transparency(
        1,
        record,
        &exchange,
        (&mut Vec::new(), &reports),
        &ctx,
    );
    let refusal = match result {
        Err(CodecError::ResourceLimit(refusal)) => refusal,
        Err(error) => panic!("unexpected transparency property refusal: {error:?}"),
        Ok(_) => panic!("transparency property must refuse on depth"),
    };
    assert_eq!(refusal.dimension, ResourceDimension::RecursionDepth);
    assert_eq!(refusal.operation, "step_reference_value_walk");
    assert_eq!(ctx.resource_refusal(), Some(refusal));
}

#[test]
fn presentation_transparency_details_charge_first_property_before_large_suffix() {
    let mut parameters = String::from("#2");
    for _ in 0..1024 {
        parameters.push_str(",#2");
    }
    let source = format!(
        "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=SURFACE_STYLE_RENDERING_WITH_PROPERTIES($,({parameters}));#2=SURFACE_STYLE_TRANSPARENT(0.2);ENDSEC;END-ISO-10303-21;"
    );
    let (exchange, _) = crate::test_support::with_service_context(
        source.as_bytes(),
        crate::parse::parse_inner,
    )
    .expect("large transparency candidate exchange");
    let record = exchange.records().get(&1).expect("rendering record");
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "STEP transparency detail traversal",
        |limit| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = limit;
            let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
                .expect("empty root fits transparency policy");
            let reports = std::cell::RefCell::new(
                ctx.reserve_scoped(0, "report fixture")
                    .expect("report scope"),
            );
            super::super::surface_transparency(
                1,
                record,
                &exchange,
                (&mut Vec::new(), &reports),
                &ctx,
            )
            .map(|_| ())
        },
    );
    let CodecError::ResourceLimit(refusal) = error else {
        panic!("first transparency detail must expose its work boundary");
    };
    assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
    assert_eq!(refusal.operation, "STEP transparency detail traversal");
    assert_eq!(refusal.additional, 1);
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
                        (
                            &mut Vec::new(),
                            &std::cell::RefCell::new(
                                ctx.reserve_scoped(0, "report fixture").expect("scope"),
                            ),
                        ),
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
            super::super::surface_side_rank(1, record, (&mut Vec::new(), &std::cell::RefCell::new(ctx.reserve_scoped(0, "report fixture").expect("scope"))), &mut BTreeSet::new(), &std::cell::RefCell::new(ctx.reserve_scoped(0, "step invalid surface side storage").unwrap()), &ctx),
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
fn presentation_style_domain_visits_first_child_before_large_set_suffix() {
    let mut members = String::from("#2");
    for _ in 0..4096 {
        members.push_str(",$");
    }
    let source = format!(
        "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=GEOMETRIC_SET('',({members}));#2=GEOMETRIC_SET('',(#3));#3=CARTESIAN_POINT('',(0.,0.,0.));ENDSEC;END-ISO-10303-21;"
    );
    let (exchange, _) = crate::test_support::with_service_context(
        source.as_bytes(),
        crate::parse::parse_inner,
    )
    .expect("large style domain exchange");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // Pre-admitting 4,097 values cannot fit. The root active-set insertion
    // costs 696 work units; its first child then reaches the depth refusal.
    policy.limits.max_work_units = 2_048;
    policy.limits.max_recursion_depth = 1;
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits domain policy");
    let refusal = match super::super::style_domain(1, &exchange, &ctx) {
        Err(CodecError::ResourceLimit(refusal)) => refusal,
        Err(error) => panic!("unexpected style domain refusal: {error:?}"),
        Ok(_) => panic!("style domain child must refuse on depth"),
    };
    assert_eq!(refusal.dimension, ResourceDimension::RecursionDepth);
    assert_eq!(refusal.operation, "step_presentation_style_domain_walk");
    assert_eq!(ctx.resource_refusal(), Some(refusal));
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
        super::super::predefined("ReD"),
        super::super::predefined("red")
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
fn presentation_appearance_target_copies_visit_prefix_before_large_suffix() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
    let (exchange, _) = crate::test_support::with_service_context(source, crate::parse::parse_inner)
        .expect("appearance exchange");
    let setup_arena = DecodeArena::new();
    let (setup_ctx, _) = DecodeContext::from_root_bytes(source, &setup_arena, &DecodePolicy::default())
        .expect("setup root");
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    let carriers = crate::reader::index::CarrierIndex::from_ir(&ir, &setup_ctx)
        .expect("setup carriers");
    let mut topology = crate::reader::topology::decode(&exchange, &mut ir, &carriers, &setup_ctx)
        .expect("setup topology");
    let body = cadmpeg_ir::ids::BodyId::mint("step:model:body#1").expect("body ID");
    const TARGETS: usize = 1_025;
    topology
        .body_by_root
        .insert(1, std::iter::repeat(body.clone()).take(TARGETS).collect());

    let products = std::collections::BTreeMap::new();
    let entity_ids = super::super::EntityIds {
        edges: BTreeSet::new(),
        vertices: BTreeSet::new(),
        points: BTreeSet::new(),
        curves: BTreeSet::new(),
        surfaces: BTreeSet::new(),
        products: &products,
        occurrences: BTreeSet::new(),
        pmi: BTreeSet::new(),
        tessellations: BTreeSet::new(),
    };
    let faces = std::collections::BTreeMap::new();
    let bodies = std::collections::BTreeMap::from([(body.as_str().to_owned(), 0)]);
    let indices = super::super::PresentationIndices {
        faces: &faces,
        bodies: &bodies,
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // The source collection has 1,025 targets, above this work cap by itself.
    // The first two identity copies reach the retained cap first.
    policy.limits.max_work_units = 1_024;
    policy.limits.max_retained_bytes = body.as_str().len() as u64;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
        .expect("empty root fits target policy");
    let mut storage = ctx.reserve_scoped(0, "target fixture").expect("scope");
    let result = super::super::appearance_targets(
        1,
        &exchange,
        &topology,
        &entity_ids,
        indices,
        &mut storage,
        &ctx,
    );
    let CodecError::ResourceLimit(refusal) = result.expect_err("second copy exceeds retained cap")
    else {
        panic!("appearance target copy must refuse");
    };
    assert_eq!(refusal.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(refusal.operation, "step_presentation_appearance_body_identity");
    assert_eq!(ctx.resource_refusal(), Some(refusal));
}

#[test]
fn presentation_layer_item_copies_visit_prefix_before_large_suffix() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
    let (exchange, _) = crate::test_support::with_service_context(source, crate::parse::parse_inner)
        .expect("presentation exchange");
    let setup_arena = DecodeArena::new();
    let (setup_ctx, _) = DecodeContext::from_root_bytes(source, &setup_arena, &DecodePolicy::default())
        .expect("setup root");
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    let carriers = crate::reader::index::CarrierIndex::from_ir(&ir, &setup_ctx)
        .expect("setup carriers");
    let mut topology = crate::reader::topology::decode(&exchange, &mut ir, &carriers, &setup_ctx)
        .expect("setup topology");
    let body = cadmpeg_ir::ids::BodyId::mint("step:model:body#1").expect("body ID");
    const ITEMS: usize = 1_025;
    topology
        .body_by_root
        .insert(1, std::iter::repeat(body.clone()).take(ITEMS).collect());
    let products = std::collections::BTreeMap::new();
    let entity_ids = super::super::EntityIds {
        edges: BTreeSet::new(),
        vertices: BTreeSet::new(),
        points: BTreeSet::new(),
        curves: BTreeSet::new(),
        surfaces: BTreeSet::new(),
        products: &products,
        occurrences: BTreeSet::new(),
        pmi: BTreeSet::new(),
        tessellations: BTreeSet::new(),
    };
    let faces = std::collections::BTreeMap::new();
    let bodies = std::collections::BTreeMap::from([(body.as_str().to_owned(), 0)]);
    let indices = super::super::PresentationIndices {
        faces: &faces,
        bodies: &bodies,
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // The source collection exceeds the work cap. A per-item walk reaches the
    // second identity copy after the first item's initial Vec backing fits.
    policy.limits.max_work_units = 1_024;
    let item_size = std::mem::size_of::<cadmpeg_ir::PresentationItem>();
    let initial_capacity = match item_size {
        0 => 0,
        1 => 8,
        2..=1024 => 4,
        _ => 1,
    };
    let initial_backing = initial_capacity * item_size;
    policy.limits.max_retained_bytes =
        (initial_backing + body.as_str().len()) as u64;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
        .expect("empty root fits layer-item policy");
    let mut items = Vec::new();
    let result = super::super::append_presentation_items(
        1,
        &exchange,
        &topology,
        &entity_ids,
        indices,
        &mut items,
        &ctx,
    );
    let CodecError::ResourceLimit(refusal) = result.expect_err("second copy exceeds retained cap")
    else {
        panic!("layer item identity copy must refuse");
    };
    assert_eq!(refusal.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(refusal.operation, "step_presentation_layer_body_identity");
    assert_eq!(items.len(), 1);
    assert_eq!(ctx.resource_refusal(), Some(refusal));
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
        &mut std::array::from_fn(|_| std::collections::BTreeMap::new()),
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
                losses: (
                    &mut Vec::new(),
                    &std::cell::RefCell::new(
                        ctx.reserve_scoped(0, "report fixture").expect("scope"),
                    ),
                ),
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
fn presentation_color_walk_frames_refuse_collection_limit() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM(#2);#2=COLOUR_RGB('red',1.,0.,0.);ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("color graph exchange");
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::CollectionItems,
        "step_presentation_color_walk_frames",
        |limit| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = limit;
            let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
                .expect("empty root fits frame policy");
            let result = super::super::find_color(
                1,
                &exchange,
                super::super::StyleDomain::Any,
                super::super::ColorSearchState {
                    storage: &std::cell::RefCell::new(
                        ctx.reserve_scoped(0, "color search fixture").expect("scope"),
                    ),
                    active: &mut BTreeSet::new(),
                    cache: &mut std::collections::BTreeMap::new(),
                    losses: (
                        &mut Vec::new(),
                        &std::cell::RefCell::new(
                            ctx.reserve_scoped(0, "report fixture").expect("scope"),
                        ),
                    ),
                    invalid_surface_sides: &mut BTreeSet::new(),
                },
                0,
                &ctx,
            );
            if let Err(CodecError::ResourceLimit(refusal)) = &result {
                assert_eq!(ctx.resource_refusal().as_ref(), Some(refusal));
            }
            result
        },
    );
    assert!(matches!(
        error,
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_presentation_color_walk_frames"
    ));
}

#[test]
fn presentation_color_walk_refuses_depth_limit() {
    color_search_refuses("step_presentation_color_walk", 100, 100, 0);
}

#[test]
fn presentation_color_walk_visits_first_child_before_large_parameter_suffix() {
    let mut parameters = String::from("T(#2)");
    for _ in 0..4096 {
        parameters.push_str(",$");
    }
    let source = format!(
        "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM({parameters});#2=ITEM(#3);#3=ITEM();ENDSEC;END-ISO-10303-21;"
    );
    let (exchange, _) = crate::test_support::with_service_context(
        source.as_bytes(),
        crate::parse::parse_inner,
    )
    .expect("large color graph exchange");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // The root record alone has 4,097 parameters. Pre-admitting them cannot fit.
    // The first typed value descends to #2 before a second color frame is needed.
    policy.limits.max_work_units = 2_048;
    policy.limits.max_recursion_depth = 2;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
        .expect("empty root fits color policy");
    let storage = std::cell::RefCell::new(ctx.reserve_scoped(0, "color search fixture").expect("scope"));
    let reports = std::cell::RefCell::new(ctx.reserve_scoped(0, "report fixture").expect("scope"));
    let mut losses = Vec::new();
    let result = super::super::find_color(
        1,
        &exchange,
        super::super::StyleDomain::Any,
        super::super::ColorSearchState {
            storage: &storage,
            active: &mut BTreeSet::new(),
            cache: &mut std::collections::BTreeMap::new(),
            losses: (&mut losses, &reports),
            invalid_surface_sides: &mut BTreeSet::new(),
        },
        0,
        &ctx,
    );
    let refusal = match result {
        Err(CodecError::ResourceLimit(refusal)) => refusal,
        Err(error) => panic!("unexpected color child refusal: {error:?}"),
        Ok(_) => panic!("color child must refuse on depth"),
    };
    assert_eq!(refusal.dimension, ResourceDimension::RecursionDepth);
    assert_eq!(refusal.operation, "step_reference_value_walk");
    assert_eq!(ctx.resource_refusal(), Some(refusal));
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
fn presentation_color_cache_copy_refuses_materialized_limit() {
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
    // The cached candidate copy reserves the three bytes of its name as scratch.
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::MaterializedBytes,
        "step_presentation_color_cache_copy",
        |limit| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_materialized_bytes = limit;
            let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy).expect("root");
            let result = super::super::find_color(
                1,
                &exchange,
                super::super::StyleDomain::Any,
                super::super::ColorSearchState {
                    storage: &std::cell::RefCell::new(
                        ctx.reserve_scoped(0, "color search fixture")
                            .expect("scope"),
                    ),
                    active: &mut BTreeSet::new(),
                    cache: &mut cache,
                    losses: (
                        &mut Vec::new(),
                        &std::cell::RefCell::new(
                            ctx.reserve_scoped(0, "report fixture").expect("scope"),
                        ),
                    ),
                    invalid_surface_sides: &mut BTreeSet::new(),
                },
                0,
                &ctx,
            );
            result
        },
    );
    assert!(
        matches!(error, CodecError::ResourceLimit(refusal) if refusal.dimension == ResourceDimension::MaterializedBytes && refusal.operation == "step_presentation_color_cache_copy")
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
