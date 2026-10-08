// SPDX-License-Identifier: Apache-2.0

use crate::document::CadIr;
use crate::report::{
    check::{Check, Finding},
    Severity,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn refuses(
    ir: &CadIr,
    check: impl Fn(&DecodeContext<'_>, &CadIr, &mut Vec<Finding>) -> Result<(), CodecError>,
) {
    for dimension in [
        ResourceDimension::MaterializedBytes,
        ResourceDimension::CollectionItems,
        ResourceDimension::WorkUnits,
        ResourceDimension::RetainedBytes,
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = 0,
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
            ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
            ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = 0,
            _ => panic!("test dimension"),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut findings = Vec::new();
        let Err(CodecError::ResourceLimit(limit)) = check(&ctx, ir, &mut findings) else {
            panic!("topology graph must refuse");
        };
        assert_eq!(limit.dimension, dimension);
        assert!(findings.is_empty());
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit)
        );
    }
}

#[test]
fn radial_ring_validation_preserves_index_walk_and_finding_refusals() {
    let mut ir = crate::examples::unit_cube().unwrap();
    ir.model.coedges[0].radial_next = "test:model:coedge#missing".try_into().unwrap();
    refuses(&ir, super::check_coedge_pairing);
}

#[test]
fn wire_topology_validation_preserves_index_walk_and_finding_refusals() {
    let mut ir = crate::examples::unit_cube().unwrap();
    ir.model.shells[0].add_wire_edge(ir.model.coedges[0].edge.clone());
    refuses(&ir, super::check_wire_topology);
}

#[test]
fn shell_connectivity_validation_preserves_index_walk_and_finding_refusals() {
    let mut ir = crate::examples::unit_cube().unwrap();
    ir.model.coedges.clear();
    refuses(&ir, super::check_shell_connectivity);
}

#[test]
fn topology_graph_validation_releases_scopes_without_retaining_identities() {
    let ir = crate::examples::unit_cube().unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 32768;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut findings = Vec::new();
    super::check_coedge_pairing(&ctx, &ir, &mut findings).unwrap();
    super::check_wire_topology(&ctx, &ir, &mut findings).unwrap();
    super::check_shell_connectivity(&ctx, &ir, &mut findings).unwrap();
    assert!(findings.is_empty());
    drop(
        ctx.reserve_scoped(32768, "topology graph scopes released")
            .unwrap(),
    );
    ctx.finish_session().unwrap();
}

#[test]
fn radial_ring_crossing_keeps_error_order_and_owner() {
    let mut ir = crate::examples::unit_cube().unwrap();
    let owner = ir.model.coedges[0].id.clone();
    let other = ir
        .model
        .coedges
        .iter()
        .find(|coedge| coedge.edge != ir.model.coedges[0].edge)
        .unwrap()
        .id
        .clone();
    ir.model.coedges[0].radial_next = other;
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut findings = Vec::new();
    super::check_coedge_pairing(&ctx, &ir, &mut findings).unwrap();
    assert!(findings.len() >= 2);
    for finding in &findings[..2] {
        assert_eq!(finding.check, Check::CoedgePairing);
        assert_eq!(finding.severity, Severity::Error);
        assert_eq!(finding.entity.as_deref(), Some(owner.as_str()));
    }
    assert_eq!(findings[0].message, "radial ring crosses edges");
    assert_eq!(findings[1].message, "radial ring does not close");
}

#[test]
fn shell_connectivity_preserves_incidence_and_shell_ownership() {
    let mut ir = crate::examples::unit_cube().unwrap();
    ir.model.coedges.clear();
    let vertices = ["test:model:vertex#a", "test:model:vertex#b"];
    for (index, loop_) in ir.model.loops.iter_mut().enumerate() {
        loop_.boundary = crate::topology::LoopBoundary::Vertex {
            vertex: vertices[usize::from(index > 1)].try_into().unwrap(),
            pcurves: Vec::new(),
        };
    }
    let mut bridge = ir.model.loops[1].clone();
    bridge.id = "test:model:loop#bridge".try_into().unwrap();
    bridge.boundary = crate::topology::LoopBoundary::Vertex {
        vertex: vertices[1].try_into().unwrap(),
        pcurves: Vec::new(),
    };
    ir.model.loops.push(bridge.clone());
    // Repeated incidences do not change connectivity.
    bridge.id = "test:model:loop#bridge-repeat".try_into().unwrap();
    ir.model.loops.push(bridge);
    let original = ir.model.shells[0].clone();
    for (members, disconnected) in [
        (vec![ir.model.faces[0].id.clone(), ir.model.faces[2].id.clone()], true),
        (vec![ir.model.faces[0].id.clone(), ir.model.faces[1].id.clone(), ir.model.faces[2].id.clone()], false),
        (original.faces().to_vec(), false),
    ] {
        ir.model.shells[0] = crate::topology::Shell::with_faces(
            original.id.clone(), original.region.clone(), members,
        ).unwrap();
        let ctx = cadmpeg_test_support::service_decode_context();
        let mut findings = Vec::new();
        super::check_shell_connectivity(&ctx, &ir, &mut findings).unwrap();
        assert_eq!(findings.len(), usize::from(disconnected));
        if disconnected {
            assert_eq!(findings[0].check, Check::ShellTopology);
            assert_eq!(findings[0].severity, Severity::Error);
            assert_eq!(findings[0].entity.as_deref(), Some(original.id.as_str()));
            assert_eq!(findings[0].message, "shell faces are disconnected through shared edges or vertices");
        }
    }
}

#[test]
fn shell_connectivity_uses_shared_edges_without_endpoint_records() {
    let mut ir = crate::examples::unit_cube().unwrap();
    let template = ir.model.coedges[0].clone();
    ir.model.coedges.clear();
    ir.model.edges.clear();
    for (index, face) in ir.model.faces.iter().enumerate() {
        let mut coedge = template.clone();
        coedge.id = format!("test:model:coedge#{index}").try_into().unwrap();
        coedge.owner_loop = ir.model.loops.iter().find(|loop_| loop_.face == face.id).unwrap().id.clone();
        ir.model.coedges.push(coedge);
    }
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut findings = Vec::new();
    super::check_shell_connectivity(&ctx, &ir, &mut findings).unwrap();
    assert!(findings.is_empty());
}

#[test]
fn shell_connectivity_keeps_shared_vertex_storage_proportional_to_faces() {
    const FACE_COUNT: usize = 64;
    let mut ir = crate::examples::unit_cube().unwrap();
    let face = ir.model.faces[0].clone();
    let loop_ = ir.model.loops[0].clone();
    ir.model.faces.clear();
    ir.model.loops.clear();
    ir.model.coedges.clear();
    ir.model.edges.clear();
    for index in 0..FACE_COUNT {
        let mut face = face.clone();
        let mut loop_ = loop_.clone();
        face.id = format!("test:model:face#{index}").try_into().unwrap();
        loop_.id = format!("test:model:loop#{index}").try_into().unwrap();
        loop_.face = face.id.clone();
        loop_.boundary = crate::topology::LoopBoundary::Vertex {
            vertex: "test:model:vertex#shared".try_into().unwrap(),
            pcurves: Vec::new(),
        };
        face.loops = crate::topology::FaceLoops::classified(loop_.id.clone(), Vec::new());
        ir.model.faces.push(face);
        ir.model.loops.push(loop_);
    }
    let shell = &ir.model.shells[0];
    ir.model.shells[0] = crate::topology::Shell::with_faces(
        shell.id.clone(),
        shell.region.clone(),
        ir.model.faces.iter().map(|face| face.id.clone()).collect(),
    ).unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // One KiB per face covers the borrowed indexes, incidence slots,
    // traversal sets, pending stack, and their vector growth overlap.
    // A 64-face clique alone needs 64 * 63 * 24 bytes of neighbor slots.
    let scratch_bound = cadmpeg_core::decode::u64_from_index(FACE_COUNT) * 1024;
    policy.limits.max_materialized_bytes = scratch_bound;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut findings = Vec::new();
    super::check_shell_connectivity(&ctx, &ir, &mut findings).unwrap();
    assert!(findings.is_empty());
    drop(ctx.reserve_scoped(scratch_bound, "shell incidence scopes released").unwrap());
    ctx.finish_session().unwrap();
}
