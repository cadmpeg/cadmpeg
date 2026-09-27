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
