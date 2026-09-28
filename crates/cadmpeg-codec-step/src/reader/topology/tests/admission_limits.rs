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

#[test]
fn built_outcome_refuses_collection_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
        .expect("empty root fits policy");
    let mut outcome = super::super::BuildOutcome::Built(Vec::new());
    let built = super::super::Built {
        typed: std::collections::HashSet::new(),
        draft: cadmpeg_ir::draft::ModelDraft::new(),
        body_id: body_id(),
        shell_sources: std::collections::BTreeSet::new(),
        pcurve_admissions: Vec::new(),
    };
    assert!(matches!(
        outcome.push(built, &ctx),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_topology_built_outcome"
    ));
}

#[test]
fn connected_wire_typed_claims_refuse_collection_limit() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=CONNECTED_EDGE_SET('',(#2));#2=DUMMY();ENDSEC;END-ISO-10303-21;";
    let (exchange, _) = crate::parse::parse(source).expect("valid connected set");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy)
        .expect("source fits policy");
    let carriers = crate::reader::index::CarrierIndex::from_ir(&cadmpeg_ir::CadIr::empty(), &ctx)
        .expect("empty carrier index fits policy");
    assert!(matches!(
        super::super::build_wire_set(3, 1, &exchange, &BTreeMap::new(), &BTreeMap::new(), &carriers, false, &mut Vec::new(), &ctx),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_wire_typed"
    ));
}

#[test]
fn shell_wire_typed_claims_refuse_collection_limit() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=WIRE_SHELL('',(#2));#2=DUMMY();ENDSEC;END-ISO-10303-21;";
    let (exchange, _) = crate::parse::parse(source).expect("valid wire shell");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy)
        .expect("source fits policy");
    let carriers = crate::reader::index::CarrierIndex::from_ir(&cadmpeg_ir::CadIr::empty(), &ctx)
        .expect("empty carrier index fits policy");
    assert!(matches!(
        super::super::build_shell_wire_set(3, 1, &exchange, &BTreeMap::new(), &BTreeMap::new(), &carriers, false, false, &mut Vec::new(), &ctx),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_wire_typed"
    ));
}

#[test]
fn subset_parent_loss_refuses_collection_limit() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=CONNECTED_EDGE_SUB_SET('',(),$);ENDSEC;END-ISO-10303-21;";
    let (exchange, _) = crate::parse::parse(source).expect("valid subset syntax");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy)
        .expect("source fits policy");
    assert!(matches!(
        super::super::validate_subset_parent(1, exchange.records().get(&1).expect("subset"), "CONNECTED_EDGE_SUB_SET", &exchange, &mut Vec::new(), &ctx),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_topology_losses"
    ));
}

#[test]
fn curve_less_wire_edge_loss_refuses_collection_limit() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;ENDSEC;END-ISO-10303-21;";
    let (exchange, _) = crate::parse::parse(source).expect("valid empty exchange");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy)
        .expect("source fits policy");
    let edge = super::super::EdgeDef::Bare { start: 1, end: 2 };
    assert!(matches!(
        super::super::edge_curve_id_reported(1, &edge, &exchange, &mut Vec::new(), &ctx),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_topology_losses"
    ));
}

#[test]
fn vertex_definitions_refuse_collection_limit() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=VERTEX_POINT('',#2);#2=DUMMY();ENDSEC;END-ISO-10303-21;";
    let (exchange, _) = crate::parse::parse(source).expect("valid vertex reference");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy)
        .expect("source fits policy");
    assert!(matches!(
        super::super::vertex_defs(&exchange, &ctx),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_vertex_definitions"
    ));
}

#[test]
fn oriented_edge_definitions_refuse_collection_limit() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ORIENTED_EDGE('',*,*,#2,.T.);#2=DUMMY();ENDSEC;END-ISO-10303-21;";
    let (exchange, _) = crate::parse::parse(source).expect("valid oriented edge reference");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy)
        .expect("source fits policy");
    assert!(matches!(
        super::super::oriented_defs(&exchange, &ctx),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_oriented_edge_definitions"
    ));
}

fn edge_definition_refusal(collection_limit: u64, retained_limit: u64, depth_limit: u64) -> CodecError {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=DUMMY();#2=DUMMY();#3=EDGE('',#1,#2);ENDSEC;END-ISO-10303-21;";
    let (exchange, _) = crate::parse::parse(source).expect("valid edge reference");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = collection_limit;
    policy.limits.max_retained_bytes = retained_limit;
    policy.limits.max_recursion_depth = depth_limit;
    let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy)
        .expect("source fits policy");
    super::super::edge_defs(&exchange, &ctx)
        .err()
        .expect("edge definitions exceed limit")
}

#[test]
fn edge_definition_active_set_refuses_collection_limit() {
    assert!(matches!(edge_definition_refusal(0, u64::MAX, u64::MAX),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_edge_definition_active"));
}

#[test]
fn edge_definition_cache_refuses_collection_limit() {
    assert!(matches!(edge_definition_refusal(1, u64::MAX, u64::MAX),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_edge_definition_cache"));
}

#[test]
fn edge_definitions_refuse_collection_limit() {
    assert!(matches!(edge_definition_refusal(2, u64::MAX, u64::MAX),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_edge_definitions"));
}

#[test]
fn edge_definition_node_refuses_retained_limit() {
    assert!(matches!(edge_definition_refusal(u64::MAX, 0, u64::MAX),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::RetainedBytes
                && refusal.operation == "step_edge_definition_node"));
}

#[test]
fn edge_definition_recursion_refuses_depth_limit() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=DUMMY();#2=DUMMY();#3=SUBEDGE('',#1,#2,#4);#4=EDGE('',#1,#2);ENDSEC;END-ISO-10303-21;";
    let (exchange, _) = crate::parse::parse(source).expect("valid subedge reference");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy)
        .expect("source fits policy");
    assert!(matches!(super::super::edge_defs(&exchange, &ctx),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RecursionDepth
                && refusal.operation == "step_edge_definition_recursion"));
}

fn shell_definition_refusal(collection_limit: u64) -> CodecError {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=OPEN_SHELL('',());ENDSEC;END-ISO-10303-21;";
    let (exchange, _) = crate::parse::parse(source).expect("valid shell reference");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = collection_limit;
    let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy)
        .expect("source fits policy");
    super::super::shell_defs(&exchange, &ctx)
        .err()
        .expect("shell definitions exceed limit")
}

#[test]
fn shell_definition_active_set_refuses_collection_limit() {
    assert!(matches!(shell_definition_refusal(0),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_shell_definition_active"));
}

#[test]
fn shell_definition_cache_refuses_collection_limit() {
    assert!(matches!(shell_definition_refusal(1),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_shell_definition_cache"));
}

#[test]
fn shell_definitions_refuse_collection_limit() {
    assert!(matches!(shell_definition_refusal(2),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_shell_definitions"));
}

#[test]
fn shell_definition_typed_copy_refuses_collection_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
        .expect("empty root fits policy");
    let definition = super::super::ShellDef {
        base: 1,
        forward: true,
        typed: std::collections::HashSet::from([2]),
    };
    assert!(matches!(super::super::copy_shell_def(&definition, &ctx),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_shell_definition_typed_copy"));
}

#[test]
fn shell_definition_claims_refuse_collection_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
        .expect("empty root fits policy");
    let definition = super::super::ShellDef {
        base: 1,
        forward: true,
        typed: std::collections::HashSet::from([2]),
    };
    let shells = BTreeMap::from([(1, definition)]);
    assert!(matches!(super::super::shell_def_for(1, &shells, &mut std::collections::HashSet::new(), &ctx),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_shell_definition_claims"));
}

#[test]
fn shell_definition_recursion_refuses_depth_limit() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=OPEN_SHELL('',());#2=ORIENTED_OPEN_SHELL('',*,#1,.T.);ENDSEC;END-ISO-10303-21;";
    let (exchange, _) = crate::parse::parse(source).expect("valid oriented shell reference");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy)
        .expect("source fits policy");
    assert!(matches!(super::super::shell_def_cached(2, &exchange, &mut std::collections::BTreeSet::new(), &mut BTreeMap::new(), &ctx),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RecursionDepth
                && refusal.operation == "step_shell_definition_recursion"));
}

fn topology_root_refusal(collection_limit: u64, include_distinct: bool) -> CodecError {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=OPEN_SHELL('',());#2=SHELL_BASED_SURFACE_MODEL('',(#1));ENDSEC;END-ISO-10303-21;";
    let (exchange, _) = crate::parse::parse(source).expect("valid shell model reference");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = collection_limit;
    let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy)
        .expect("source fits policy");
    let root = exchange.records().get(&2).expect("shell model");
    let shells = BTreeMap::from([(1, super::super::ShellDef {
        base: 1,
        forward: true,
        typed: std::collections::HashSet::new(),
    })]);
    let key = super::super::root_key(root, &exchange, &shells, &ctx)
        .and_then(|key| key.ok_or_else(|| CodecError::malformed("missing root key")));
    if !include_distinct {
        return key.err().expect("root key exceeds limit");
    }
    let mut distinct = std::collections::BTreeSet::new();
    super::super::insert_topology_set(
        &mut distinct,
        key.expect("root key fits limit"),
        &ctx,
        "step_distinct_topology_roots",
    )
    .err()
    .expect("distinct root set exceeds limit")
}

#[test]
fn topology_root_shell_steps_refuse_collection_limit() {
    assert!(matches!(topology_root_refusal(0, false),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_root_shell_steps"));
}

#[test]
fn topology_root_shell_keys_refuse_collection_limit() {
    assert!(matches!(topology_root_refusal(1, false),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_root_shell_keys"));
}

#[test]
fn topology_distinct_roots_refuse_collection_limit() {
    assert!(matches!(topology_root_refusal(2, true),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_distinct_topology_roots"));
}

fn geometric_set_refusal(collection_limit: u64, has_surface: bool) -> CodecError {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION('',(#2),#3);#2=GEOMETRIC_SET('',(#4));#3=DUMMY();#4=DUMMY();ENDSEC;END-ISO-10303-21;";
    let (exchange, _) = crate::parse::parse(source).expect("valid geometric set references");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = collection_limit;
    let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy)
        .expect("source fits policy");
    let mut carriers = crate::reader::index::CarrierIndex::from_ir(&cadmpeg_ir::CadIr::empty(), &ctx)
        .expect("empty carrier index fits policy");
    if has_surface {
        carriers.surfaces.insert(4, crate::reader::index::SurfaceIndex(0));
    }
    super::super::build_geometric_set(
        1,
        exchange.records().get(&1).expect("representation"),
        &exchange,
        &carriers,
        &mut Vec::new(),
        &ctx,
    )
    .err()
    .expect("geometric set exceeds collection limit")
}

#[test]
fn geometric_set_typed_refuses_collection_limit() {
    assert!(matches!(geometric_set_refusal(0, false),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_geometric_set_typed"));
}

#[test]
fn geometric_set_shell_faces_refuse_collection_limit() {
    assert!(matches!(geometric_set_refusal(2, true),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_geometric_set_shell_faces"));
}

#[test]
fn geometric_set_faces_refuse_collection_limit() {
    assert!(matches!(geometric_set_refusal(3, true),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_geometric_set_faces"));
}

fn staged_topology_refusal(
    collection_limit: u64,
    retained_limit: u64,
    surface_count: usize,
) -> super::super::StageError {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = collection_limit;
    policy.limits.max_retained_bytes = retained_limit;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
        .expect("empty root fits policy");
    let body_id = body_id();
    let region_id = cadmpeg_ir::ids::RegionId::mint("step:data:region#1")
        .expect("valid region identity");
    let surfaces = (0..surface_count)
        .map(|index| cadmpeg_ir::geometry::Surface {
            id: cadmpeg_ir::ids::SurfaceId::mint(format!("step:data:surface#{index}"))
                .expect("valid surface identity"),
            geometry: cadmpeg_ir::geometry::SurfaceGeometry::Solved(
                cadmpeg_ir::geometry::SolvedSurfaceGeometry::Unknown { record: None },
            ),
            source_object: None,
        })
        .collect();
    super::super::staged_topology(
        std::collections::HashSet::new(),
        Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new(), surfaces, Vec::new(),
        cadmpeg_ir::topology::Region {
            id: region_id.clone(),
            body: body_id.clone(),
            shells: Vec::new(),
        },
        cadmpeg_ir::topology::Body {
            id: body_id,
            kind: cadmpeg_ir::topology::BodyKind::Sheet,
            regions: vec![region_id],
            transform: None,
            name: None,
            color: None,
            visible: None,
        },
        &ctx,
    )
    .err()
    .expect("staging exceeds limit")
}

#[test]
fn staged_surface_ids_refuse_collection_limit() {
    assert!(matches!(staged_topology_refusal(0, u64::MAX, 1),
        super::super::StageError::Resource(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_staged_surface_ids"));
}

#[test]
fn staged_surface_ids_refuse_retained_limit() {
    assert!(matches!(staged_topology_refusal(u64::MAX, 0, 1),
        super::super::StageError::Resource(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RetainedBytes
                && refusal.operation == "step_staged_surface_ids"));
}

#[test]
fn staged_surfaces_refuse_collection_limit() {
    assert!(matches!(staged_topology_refusal(1, u64::MAX, 1),
        super::super::StageError::Resource(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_staged_surfaces"));
}

#[test]
fn staged_regions_refuse_collection_limit() {
    assert!(matches!(staged_topology_refusal(0, u64::MAX, 0),
        super::super::StageError::Resource(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_staged_regions"));
}

#[test]
fn staged_bodies_refuse_collection_limit() {
    assert!(matches!(staged_topology_refusal(1, u64::MAX, 0),
        super::super::StageError::Resource(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_staged_bodies"));
}

fn brep_builder_refusal(collection_limit: u64) -> super::super::BuildError {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=OPEN_SHELL('',(#2));#2=FACE('',());#3=SHELL_BASED_SURFACE_MODEL('',(#1));ENDSEC;END-ISO-10303-21;";
    let (exchange, _) = crate::parse::parse(source).expect("valid shell model references");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = collection_limit;
    let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy)
        .expect("source fits policy");
    let carriers = crate::reader::index::CarrierIndex::from_ir(&cadmpeg_ir::CadIr::empty(), &ctx)
        .expect("empty carrier index fits policy");
    let shells = BTreeMap::from([(1, super::super::ShellDef {
        base: 1,
        forward: true,
        typed: std::collections::HashSet::new(),
    })]);
    let region = cadmpeg_ir::ids::RegionId::mint("step:data:region#3")
        .expect("valid region identity");
    super::super::build_one(
        3,
        exchange.records().get(&3).expect("model"),
        &exchange,
        &cadmpeg_ir::CadIr::empty(),
        &BTreeMap::new(), &BTreeMap::new(), &BTreeMap::new(), &shells,
        &std::collections::BTreeSet::new(), &carriers, &[1],
        body_id(), &region, false, false, false,
        &mut Vec::new(), &mut None, &ctx,
    )
    .err()
    .expect("builder exceeds limit")
}

#[test]
fn brep_typed_refuses_collection_limit() {
    assert!(matches!(brep_builder_refusal(0),
        super::super::BuildError::Resource(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_brep_typed"));
}

#[test]
fn brep_body_regions_refuse_collection_limit() {
    assert!(matches!(brep_builder_refusal(1),
        super::super::BuildError::Resource(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_brep_body_regions"));
}

#[test]
fn brep_used_shells_refuse_collection_limit() {
    assert!(matches!(brep_builder_refusal(2),
        super::super::BuildError::Resource(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_brep_used_shells"));
}

#[test]
fn brep_used_faces_refuse_collection_limit() {
    assert!(matches!(brep_builder_refusal(3),
        super::super::BuildError::Resource(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_brep_used_faces"));
}

fn face_attribute_refusal(collection_limit: u64, depth_limit: u64, face_id: u64) -> CodecError {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=FACE('',(#2));#2=FACE_BOUND('',#3,.T.);#3=EDGE_LOOP('',());#4=ORIENTED_FACE('',*,#1,.T.);ENDSEC;END-ISO-10303-21;";
    let (exchange, _) = crate::parse::parse(source).expect("valid face references");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = collection_limit;
    policy.limits.max_recursion_depth = depth_limit;
    let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy)
        .expect("source fits policy");
    super::super::face_attributes(
        face_id,
        exchange.records().get(&face_id).expect("face record"),
        &exchange,
        &mut std::collections::BTreeSet::new(),
        &ctx,
    )
    .err()
    .expect("face attributes exceed limit")
}

#[test]
fn face_attribute_active_refuses_collection_limit() {
    assert!(matches!(face_attribute_refusal(0, u64::MAX, 1),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_face_attribute_active"));
}

#[test]
fn face_attribute_bounds_refuse_collection_limit() {
    assert!(matches!(face_attribute_refusal(1, u64::MAX, 1),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_face_attribute_bounds"));
}

#[test]
fn face_attribute_typed_refuses_collection_limit() {
    assert!(matches!(face_attribute_refusal(3, u64::MAX, 4),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_face_attribute_typed"));
}

#[test]
fn face_attribute_recursion_refuses_depth_limit() {
    assert!(matches!(face_attribute_refusal(u64::MAX, 1, 4),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::RecursionDepth
                && refusal.operation == "step_face_attribute_recursion"));
}

fn implicit_face_refusal(collection_limit: u64, plane: bool) -> CodecError {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=FACE_BOUND('',#2,.T.);#2=POLY_LOOP('',(#3,#4,#5));#3=DUMMY();#4=DUMMY();#5=DUMMY();ENDSEC;END-ISO-10303-21;";
    let (exchange, _) = crate::parse::parse(source).expect("valid polygon loop references");
    let mut ir = cadmpeg_ir::CadIr::empty();
    for (id, position) in [
        (3, cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0)),
        (4, cadmpeg_ir::math::Point3::new(1.0, 0.0, 0.0)),
        (5, cadmpeg_ir::math::Point3::new(0.0, 1.0, 0.0)),
    ] {
        ir.model.points.push(cadmpeg_ir::topology::Point::new(
            cadmpeg_ir::ids::PointId::from(crate::ids::data(crate::ids::kind!("point"), id)),
            cadmpeg_ir::features::FinitePoint3::new(position).expect("finite point"),
            None,
        ));
    }
    let setup_arena = DecodeArena::new();
    let setup_policy = DecodePolicy::service();
    let (setup_ctx, _) = DecodeContext::from_root_bytes(source, &setup_arena, &setup_policy)
        .expect("source fits setup policy");
    let carriers = crate::reader::index::CarrierIndex::from_ir(&ir, &setup_ctx)
        .expect("point carriers fit setup policy");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = collection_limit;
    let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy)
        .expect("source fits policy");
    let result = if plane {
        super::super::implicit_face_plane(&[1], &exchange, &BTreeMap::new(), &carriers, &ctx)
            .map(|_| ())
    } else {
        super::super::implicit_face_points(&[1], &exchange, &BTreeMap::new(), &carriers, &ctx)
            .map(|_| ())
    };
    result.err().expect("implicit face exceeds collection limit")
}

#[test]
fn implicit_face_point_steps_refuse_collection_limit() {
    assert!(matches!(implicit_face_refusal(0, false),
        CodecError::ResourceLimit(refusal)
            if refusal.operation == "step_implicit_face_point_steps"));
}

#[test]
fn implicit_face_distinct_points_refuse_collection_limit() {
    assert!(matches!(implicit_face_refusal(3, false),
        CodecError::ResourceLimit(refusal)
            if refusal.operation == "step_implicit_face_distinct_points"));
}

#[test]
fn implicit_face_points_refuse_collection_limit() {
    assert!(matches!(implicit_face_refusal(6, false),
        CodecError::ResourceLimit(refusal)
            if refusal.operation == "step_implicit_face_points"));
}

#[test]
fn implicit_face_loops_refuse_collection_limit() {
    assert!(matches!(implicit_face_refusal(9, false),
        CodecError::ResourceLimit(refusal)
            if refusal.operation == "step_implicit_face_loops"));
}

#[test]
fn implicit_face_plane_points_refuse_collection_limit() {
    assert!(matches!(implicit_face_refusal(10, true),
        CodecError::ResourceLimit(refusal)
            if refusal.operation == "step_implicit_face_plane_points"));
}

#[test]
fn implicit_face_loop_normals_refuse_collection_limit() {
    assert!(matches!(implicit_face_refusal(13, true),
        CodecError::ResourceLimit(refusal)
            if refusal.operation == "step_implicit_face_loop_normals"));
}
