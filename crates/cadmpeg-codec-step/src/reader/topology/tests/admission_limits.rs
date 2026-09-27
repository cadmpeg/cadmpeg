// SPDX-License-Identifier: Apache-2.0
//! Collection refusals in the STEP topology reader.

use std::collections::BTreeMap;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn relationship_refusal(collection_limit: u64) -> CodecError {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = collection_limit;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
        .expect("empty root fits policy");
    super::super::push_topology_group(
        &mut BTreeMap::new(), 1u64, 2u64, &ctx,
        "step_shape_relationship_groups", "step_shape_relationship_members",
    ).expect_err("relationship exceeds collection limit")
}

#[test]
fn shape_relationship_groups_refuse_collection_limit() {
    assert!(matches!(relationship_refusal(0), CodecError::ResourceLimit(refusal)
        if refusal.dimension == ResourceDimension::CollectionItems
            && refusal.operation == "step_shape_relationship_groups"));
}

#[test]
fn shape_relationship_members_refuse_collection_limit() {
    assert!(matches!(relationship_refusal(1), CodecError::ResourceLimit(refusal)
        if refusal.dimension == ResourceDimension::CollectionItems
            && refusal.operation == "step_shape_relationship_members"));
}

#[test]
fn shape_relationship_graph_preserves_bidirectional_edges() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=SHAPE_REPRESENTATION('',(),$);#2=SHAPE_REPRESENTATION('',(),$);#3=SHAPE_REPRESENTATION_RELATIONSHIP('','',#1,#2);ENDSEC;END-ISO-10303-21;";
    let (exchange, _) = crate::parse::parse(source).expect("valid relationship graph");
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy)
        .expect("source fits policy");
    let graph = super::super::shape_representation_relationships(&exchange, &ctx)
        .expect("relationship graph fits policy");
    assert_eq!(graph.get(&1).map(Vec::as_slice), Some(&[2][..]));
    assert_eq!(graph.get(&2).map(Vec::as_slice), Some(&[1][..]));
}

#[test]
fn topology_losses_refuse_collection_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
        .expect("empty root fits policy");
    let note = crate::loss::StepLossCode::DecodeWarning.note("invalid topology");
    assert!(matches!(
        super::super::push_topology_vec(&mut Vec::new(), note, &ctx, "step_topology_losses"),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_topology_losses"
    ));
}

#[test]
fn topology_loss_merge_refuses_collection_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
        .expect("empty root fits policy");
    assert!(matches!(
        super::super::append_topology_vec(&mut Vec::new(), &mut vec![1u64], &ctx, "step_topology_loss_merge"),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_topology_loss_merge"
    ));
}

#[test]
fn topology_commit_error_text_refuses_retained_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
        .expect("empty root fits policy");
    let error = cadmpeg_ir::draft::DraftError::IdentityCollision("step:data:body#1".into());
    assert!(matches!(
        super::super::topology_commit_error("topology root", &error, &ctx),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RetainedBytes
                && refusal.operation == "step_topology_commit_error_text"
    ));
}

#[test]
fn decoded_topology_pcurves_refuse_collection_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
        .expect("empty root fits policy");
    assert!(matches!(
        super::super::insert_topology_set(&mut std::collections::BTreeSet::new(), 1u64, &ctx, "step_decoded_topology_pcurves"),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_decoded_topology_pcurves"
    ));
}

#[test]
fn associated_pcurves_refuse_collection_limit() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=SURFACE_CURVE('',#4,(#2),.PCURVE_S1.);#2=PCURVE('',#3,#5);#3=DUMMY();#4=DUMMY();#5=DUMMY();ENDSEC;END-ISO-10303-21;";
    let (exchange, _) = crate::parse::parse(source).expect("valid surface curve references");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy)
        .expect("source fits policy");
    assert!(matches!(
        super::super::associated_pcurves(1, 3, &exchange, &std::collections::BTreeSet::from([2]), &ctx),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_associated_pcurves"
    ));
}

#[test]
fn topology_claims_refuse_collection_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
        .expect("empty root fits policy");
    assert!(matches!(
        super::super::insert_topology_hash_set(&mut std::collections::HashSet::new(), 1u64, &ctx, "step_topology_claims"),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_topology_claims"
    ));
}

#[test]
fn built_wire_model_set_refuses_collection_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
        .expect("empty root fits policy");
    assert!(matches!(
        super::super::insert_topology_set(&mut std::collections::BTreeSet::new(), 1u64, &ctx, "step_built_wire_models"),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_built_wire_models"
    ));
}

fn body_id() -> cadmpeg_ir::ids::BodyId {
    cadmpeg_ir::ids::BodyId::mint("step:data:body#1").expect("valid body identity")
}

#[test]
fn topology_body_id_copy_refuses_retained_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
        .expect("empty root fits policy");
    assert!(matches!(
        super::super::copy_topology_body_id(&body_id(), &ctx, "step_topology_root_bodies"),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RetainedBytes
                && refusal.operation == "step_topology_root_bodies"
    ));
}

#[test]
fn topology_root_group_refuses_collection_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
        .expect("empty root fits policy");
    assert!(matches!(
        super::super::push_topology_body_group(&mut BTreeMap::new(), 1, &body_id(), &ctx, "step_topology_root_groups", "step_topology_root_bodies"),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_topology_root_groups"
    ));
}

#[test]
fn topology_root_bodies_refuse_collection_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
        .expect("empty root fits policy");
    assert!(matches!(
        super::super::push_topology_body_group(&mut BTreeMap::new(), 1, &body_id(), &ctx, "step_topology_root_groups", "step_topology_root_bodies"),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_topology_root_bodies"
    ));
}

#[test]
fn topology_shell_group_refuses_collection_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
        .expect("empty root fits policy");
    assert!(matches!(
        super::super::insert_topology_body_group(&mut BTreeMap::new(), 1, &body_id(), &ctx, "step_topology_shell_groups", "step_topology_shell_bodies"),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_topology_shell_groups"
    ));
}

#[test]
fn topology_shell_bodies_refuse_collection_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
        .expect("empty root fits policy");
    assert!(matches!(
        super::super::insert_topology_body_group(&mut BTreeMap::new(), 1, &body_id(), &ctx, "step_topology_shell_groups", "step_topology_shell_bodies"),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_topology_shell_bodies"
    ));
}

#[test]
fn topology_built_roots_refuse_collection_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
        .expect("empty root fits policy");
    assert!(matches!(
        super::super::insert_topology_map(&mut BTreeMap::new(), 1u64, 2u64, &ctx, "step_topology_built_roots"),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_topology_built_roots"
    ));
}

fn source_index_refusal<T: TryFrom<String, Error = cadmpeg_ir::ids::IdentityError>>(
    group_operation: &'static str,
    member_operation: &'static str,
    collection_limit: u64,
    retained_limit: u64,
) -> CodecError {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = collection_limit;
    policy.limits.max_retained_bytes = retained_limit;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
        .expect("empty root fits policy");
    super::super::push_topology_id_group::<T>(
        &mut BTreeMap::new(), 1, "step:data:body#1", &ctx,
        group_operation, member_operation,
    )
    .expect_err("source index exceeds limit")
}

#[test]
fn topology_source_faces_refuse_limits() {
    use cadmpeg_ir::ids::FaceId;
    assert!(matches!(
        source_index_refusal::<FaceId>("step_topology_source_face_groups", "step_topology_source_faces", 0, u64::MAX),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_topology_source_face_groups"
    ));
    assert!(matches!(
        source_index_refusal::<FaceId>("step_topology_source_face_groups", "step_topology_source_faces", 1, u64::MAX),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_topology_source_faces"
    ));
    assert!(matches!(
        source_index_refusal::<FaceId>("step_topology_source_face_groups", "step_topology_source_faces", u64::MAX, 0),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::RetainedBytes
                && refusal.operation == "step_topology_source_faces"
    ));
}

#[test]
fn topology_source_edges_refuse_limits() {
    use cadmpeg_ir::ids::EdgeId;
    assert!(matches!(
        source_index_refusal::<EdgeId>("step_topology_source_edge_groups", "step_topology_source_edges", 0, u64::MAX),
        CodecError::ResourceLimit(refusal)
            if refusal.operation == "step_topology_source_edge_groups"
    ));
    assert!(matches!(
        source_index_refusal::<EdgeId>("step_topology_source_edge_groups", "step_topology_source_edges", 1, u64::MAX),
        CodecError::ResourceLimit(refusal)
            if refusal.operation == "step_topology_source_edges"
    ));
    assert!(matches!(
        source_index_refusal::<EdgeId>("step_topology_source_edge_groups", "step_topology_source_edges", u64::MAX, 0),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::RetainedBytes
                && refusal.operation == "step_topology_source_edges"
    ));
}

#[test]
fn topology_source_vertices_refuse_limits() {
    use cadmpeg_ir::ids::VertexId;
    assert!(matches!(
        source_index_refusal::<VertexId>("step_topology_source_vertex_groups", "step_topology_source_vertices", 0, u64::MAX),
        CodecError::ResourceLimit(refusal)
            if refusal.operation == "step_topology_source_vertex_groups"
    ));
    assert!(matches!(
        source_index_refusal::<VertexId>("step_topology_source_vertex_groups", "step_topology_source_vertices", 1, u64::MAX),
        CodecError::ResourceLimit(refusal)
            if refusal.operation == "step_topology_source_vertices"
    ));
    assert!(matches!(
        source_index_refusal::<VertexId>("step_topology_source_vertex_groups", "step_topology_source_vertices", u64::MAX, 0),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::RetainedBytes
                && refusal.operation == "step_topology_source_vertices"
    ));
}

#[test]
fn topology_admissions_refuse_collection_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
        .expect("empty root fits policy");
    assert!(matches!(
        super::super::append_topology_vec(&mut Vec::new(), &mut vec![1u64], &ctx, "step_topology_admissions"),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_topology_admissions"
    ));
}

#[test]
fn geometric_set_omissions_refuse_collection_limit() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION('',(#2),#3);#2=GEOMETRIC_SET('',(#4));#3=DUMMY();#4=DUMMY();ENDSEC;END-ISO-10303-21;";
    let (exchange, _) = crate::parse::parse(source).expect("valid geometric set references");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy)
        .expect("source fits policy");
    let carriers = crate::reader::index::CarrierIndex::from_ir(&cadmpeg_ir::CadIr::empty(), &ctx)
        .expect("empty carrier index fits policy");
    assert!(matches!(
        super::super::geometric_set_omissions(exchange.records().get(&1).expect("representation"), &exchange, &carriers, &ctx),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_geometric_set_omissions"
    ));
}

#[test]
fn geometric_set_omission_text_refuses_retained_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
        .expect("empty root fits policy");
    assert!(matches!(
        super::super::geometric_set_omission_message("GEOMETRIC_SET", 1, &[2, 3], &ctx),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RetainedBytes
                && refusal.operation == "step_geometric_set_omission_text"
    ));
}
