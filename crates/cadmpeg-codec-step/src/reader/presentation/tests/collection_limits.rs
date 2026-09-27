// SPDX-License-Identifier: Apache-2.0
//! Presentation identity index admissions.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

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
