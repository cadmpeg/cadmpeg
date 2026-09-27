// SPDX-License-Identifier: Apache-2.0
//! Presentation identity index admissions.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use std::collections::BTreeSet;
use std::collections::HashSet;

fn set_refuses(operation: &'static str) {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
        .expect("empty root fits policy");
    let result = super::super::collect_borrowed_identity_set(["step:model:item#1"], Some(&ctx), operation);
    assert!(matches!(
        result,
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == operation
    ));
}

fn index_refuses(operation: &'static str, retained: bool) {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    if retained {
        policy.limits.max_retained_bytes = 16;
    } else {
        policy.limits.max_collection_items = 0;
    }
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
        .expect("empty root fits policy");
    let result = super::super::collect_identity_indices(["step:model:item#1"], Some(&ctx), operation);
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
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
        .expect("empty root fits policy");
    let mut values = BTreeSet::new();
    let result = super::super::insert_presentation_set(&mut values, 1_u64, Some(&ctx), operation);
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
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
        .expect("empty root fits policy");
    let mut values = Vec::new();
    let result = super::super::push_presentation_vec(&mut values, 1_u64, Some(&ctx), operation);
    assert!(matches!(
        result,
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == operation
    ));
}

fn style_target_refuses(operation: &str, depth_limit: bool) {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=GEOMETRIC_SET('',(#2));#2=CARTESIAN_POINT('',(0.,0.,0.));ENDSEC;END-ISO-10303-21;";
    let (exchange, _) = crate::parse::parse(source).expect("style set exchange");
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
            super::super::expand_style_targets(
                1,
                &exchange,
                &mut HashSet::new(),
                &mut BTreeSet::new(),
                0,
                128,
                Some(&ctx),
            ),
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
fn presentation_edge_ids_refuse_collection_limit() {
    set_refuses("step_presentation_edge_ids");
}

#[test]
fn presentation_vertex_ids_refuse_collection_limit() {
    set_refuses("step_presentation_vertex_ids");
}

#[test]
fn presentation_point_ids_refuse_collection_limit() {
    set_refuses("step_presentation_point_ids");
}

#[test]
fn presentation_curve_ids_refuse_collection_limit() {
    set_refuses("step_presentation_curve_ids");
}

#[test]
fn presentation_surface_ids_refuse_collection_limit() {
    set_refuses("step_presentation_surface_ids");
}

#[test]
fn presentation_occurrence_ids_refuse_collection_limit() {
    set_refuses("step_presentation_occurrence_ids");
}

#[test]
fn presentation_pmi_ids_refuse_collection_limit() {
    set_refuses("step_presentation_pmi_ids");
}

#[test]
fn presentation_tessellation_ids_refuse_collection_limit() {
    set_refuses("step_presentation_tessellation_ids");
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
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
        .expect("empty root fits policy");
    let result = super::super::claim_presentation_typed(&mut HashSet::new(), 1, Some(&ctx));
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
    let (exchange, _) = crate::parse::parse(source).expect("transparency exchange");
    let record = exchange.records().get(&1).expect("rendering record");
    let refused = (0..=4).any(|limit| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        if retained {
            policy.limits.max_retained_bytes = limit;
        } else {
            policy.limits.max_collection_items = limit;
        }
        let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy)
            .expect("root fits policy");
        matches!(
            super::super::surface_transparency(1, record, &exchange, &mut Vec::new(), Some(&ctx)),
            Err(CodecError::ResourceLimit(refusal)) if refusal.operation == operation
        )
    });
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
    let (exchange, _) = crate::parse::parse(source).expect("surface side exchange");
    let record = exchange.records().get(&1).expect("style usage record");
    let refused = (0..=2).any(|limit| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        if retained {
            policy.limits.max_retained_bytes = limit;
        } else {
            policy.limits.max_collection_items = limit;
        }
        let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy)
            .expect("root fits policy");
        matches!(
            super::super::surface_side_rank(
                1, record, &mut Vec::new(), &mut BTreeSet::new(), Some(&ctx),
            ),
            Err(CodecError::ResourceLimit(refusal)) if refusal.operation == operation
        )
    });
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
    let (exchange, _) = crate::parse::parse(source).expect("style graph exchange");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    if depth {
        policy.limits.max_recursion_depth = 0;
    } else {
        policy.limits.max_collection_items = 0;
    }
    let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy)
        .expect("root fits graph policy");
    assert!(matches!(
        run(&exchange, &ctx),
        Err(CodecError::ResourceLimit(refusal)) if refusal.operation == operation
    ));
}

#[test]
fn presentation_style_order_items_refuse_collection_limit() {
    vector_refuses("step_presentation_style_order_items");
}

#[test]
fn presentation_style_depth_active_refuses_collection_limit() {
    style_graph_refuses("step_presentation_style_depth_active", false, |exchange, ctx| {
        super::super::style_application_order(1, exchange, 128, Some(ctx)).map(|_| ())
    });
}

#[test]
fn presentation_style_depth_walk_refuses_depth_limit() {
    style_graph_refuses("step_presentation_style_depth_walk", true, |exchange, ctx| {
        super::super::style_application_order(1, exchange, 128, Some(ctx)).map(|_| ())
    });
}

#[test]
fn presentation_style_domain_active_refuses_collection_limit() {
    style_graph_refuses("step_presentation_style_domain_active", false, |exchange, ctx| {
        super::super::style_domain(4, exchange, Some(ctx)).map(|_| ())
    });
}

#[test]
fn presentation_style_domain_walk_refuses_depth_limit() {
    style_graph_refuses("step_presentation_style_domain_walk", true, |exchange, ctx| {
        super::super::style_domain(4, exchange, Some(ctx)).map(|_| ())
    });
}

#[test]
fn presentation_hidden_style_active_refuses_collection_limit() {
    style_graph_refuses("step_presentation_hidden_style_active", false, |exchange, ctx| {
        super::super::style_is_hidden(1, &BTreeSet::new(), exchange, &mut BTreeSet::new(), Some(ctx)).map(|_| ())
    });
}

#[test]
fn presentation_hidden_style_walk_refuses_depth_limit() {
    style_graph_refuses("step_presentation_hidden_style_walk", true, |exchange, ctx| {
        super::super::style_is_hidden(1, &BTreeSet::new(), exchange, &mut BTreeSet::new(), Some(ctx)).map(|_| ())
    });
}

#[test]
fn presentation_style_inheritance_active_refuses_collection_limit() {
    style_graph_refuses("step_presentation_style_inheritance_active", false, |exchange, ctx| {
        super::super::style_inherits_from(1, 3, exchange, &mut BTreeSet::new(), Some(ctx)).map(|_| ())
    });
}

#[test]
fn presentation_style_inheritance_walk_refuses_depth_limit() {
    style_graph_refuses("step_presentation_style_inheritance_walk", true, |exchange, ctx| {
        super::super::style_inherits_from(1, 3, exchange, &mut BTreeSet::new(), Some(ctx)).map(|_| ())
    });
}

#[test]
fn presentation_null_style_visited_refuses_collection_limit() {
    style_graph_refuses("step_presentation_null_style_visited", false, |exchange, ctx| {
        super::super::contains_null_style(&crate::parse::Value::Reference(5), exchange, &mut BTreeSet::new(), 0, Some(ctx)).map(|_| ())
    });
}

#[test]
fn presentation_null_style_walk_refuses_depth_limit() {
    style_graph_refuses("step_presentation_null_style_walk", true, |exchange, ctx| {
        super::super::contains_null_style(&crate::parse::Value::Reference(5), exchange, &mut BTreeSet::new(), 0, Some(ctx)).map(|_| ())
    });
}

#[test]
fn presentation_predefined_color_matches_ascii_case_without_copy() {
    assert_eq!(super::super::predefined("ReD"), super::super::predefined("red"));
}
