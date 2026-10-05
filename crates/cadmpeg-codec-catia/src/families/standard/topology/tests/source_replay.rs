use super::super::{
    orient_face_cycles, reconstruct_mesh_selection, BoundaryDraft, CoedgeUse, EdgeBoundaryLayout,
    EdgeRow, FaceTopologyDraft, StandardTopologyDraft,
};
use crate::solve::missing_edge::{MeshBoundaryEdgeCandidate, MeshFaceBoundaryAssignment};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::NonEmptyMembers;

fn one_edge_topology() -> StandardTopologyDraft {
    StandardTopologyDraft {
        faces: vec![FaceTopologyDraft {
            boundaries: vec![BoundaryDraft {
                coedges: NonEmptyMembers::one(CoedgeUse {
                    edge_row: 0,
                    reversed: false,
                    start_vertex: 0,
                    end_vertex: 1,
                }),
            }],
        }],
        edge_rows: vec![
            EdgeRow::new(1, vec![7, 7], EdgeBoundaryLayout::CompleteBoundaryRun)
                .expect("admitted edge row"),
        ],
        vertex_points: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
        logical_vertex_count: 2,
    }
}

fn assert_saved_work_refusal(
    operation: &'static str,
    mut run: impl FnMut(&DecodeContext<'_>) -> Result<(), CodecError>,
) {
    crate::test_support::with_service_context(|ctx| run(ctx)).expect("service source fixture");
    let result = crate::test_support::with_work_refusal(operation, |ctx| {
        let result = run(ctx);
        if let Err(CodecError::ResourceLimit(limit)) = &result {
            assert_eq!(ctx.resource_refusal(), Some(*limit));
        }
        result
    });
    assert!(matches!(result,
        Err(CodecError::ResourceLimit(limit)) if limit.operation == operation));
}

#[test]
fn standard_body_orientation_kind_source_preserves_work_refusal() {
    assert_saved_work_refusal("catia_standard_body_orientation_kinds", |ctx| {
        one_edge_topology().orient_solid_body_cycles(ctx, &[1])?;
        Ok(())
    });
}

#[test]
fn standard_vertex_domain_initialization_source_preserves_work_refusal() {
    assert_saved_work_refusal("catia_standard_vertex_domain_initialization", |ctx| {
        one_edge_topology().bind_vertex_points(ctx, &[[0, 1]])?;
        Ok(())
    });
}

fn assert_native_rewrite_refusal(operation: &'static str) {
    assert_saved_work_refusal(operation, |ctx| {
        one_edge_topology().with_native_edge_vertices(ctx, &[[4, 5]])?;
        Ok(())
    });
}

#[test]
fn standard_native_vertex_rewrite_face_source_preserves_work_refusal() {
    assert_native_rewrite_refusal("catia_standard_native_vertex_rewrite_faces");
}

#[test]
fn standard_native_vertex_rewrite_boundary_source_preserves_work_refusal() {
    assert_native_rewrite_refusal("catia_standard_native_vertex_rewrite_boundaries");
}

#[test]
fn standard_native_vertex_rewrite_coedge_source_preserves_work_refusal() {
    assert_native_rewrite_refusal("catia_standard_native_vertex_rewrite_coedges");
}

#[test]
fn standard_orientation_flip_source_preserves_work_refusal() {
    assert_saved_work_refusal("catia_standard_orientation_flip_scan", |ctx| {
        let mut topology = one_edge_topology();
        topology.faces.push(topology.faces[0].clone());
        orient_face_cycles(ctx, &mut topology.faces)?;
        Ok(())
    });
}

fn assert_mesh_rewrite_refusal(operation: &'static str) {
    let topology = one_edge_topology();
    let selected = [MeshFaceBoundaryAssignment {
        boundaries: vec![vec![MeshBoundaryEdgeCandidate {
            edge: 0,
            start: 0,
            end: 1,
            reversed: Some(false),
        }]],
    }];
    let directions = [vec![vec![false]]];
    assert_saved_work_refusal(operation, |ctx| {
        reconstruct_mesh_selection(ctx, &topology.edge_rows, &[], &selected, &directions)?;
        Ok(())
    });
}

#[test]
fn mesh_selection_rewrite_face_source_preserves_work_refusal() {
    assert_mesh_rewrite_refusal("catia_mesh_selection_rewrite_faces");
}

#[test]
fn mesh_selection_rewrite_boundary_source_preserves_work_refusal() {
    assert_mesh_rewrite_refusal("catia_mesh_selection_rewrite_boundaries");
}

#[test]
fn mesh_selection_rewrite_coedge_source_preserves_work_refusal() {
    assert_mesh_rewrite_refusal("catia_mesh_selection_rewrite_coedges");
}
