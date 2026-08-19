//! Behavioral tests for standard B-rep topology solvers and parsers.

pub(super) use std::{
    cell::RefCell,
    collections::{BTreeMap, HashMap, HashSet},
    sync::Arc,
};

pub(super) use cadmpeg_ir::topology::BodyKind;

pub(super) use crate::families::standard::fbb::{
    parse_edge_tables_at, parse_edge_tables_scoped_at, parse_fbb_edge_tables,
    parse_fbb_edge_tables_width, parse_trim_chain, parse_trim_record, parse_trim_record_layout,
    parse_vertex_table, prune_edge_candidates_by_port_domains, standard_edge_count,
    standard_face_count, standard_fbb_groups, EDGE_DELIMITER,
};
pub(super) use crate::families::standard::topology::{
    complete_duplicate_face_slots, reconstruct_incidence, solve_boundary_orientation_constraints,
    Boundary, CoedgeUse, EdgeBoundaryLayout, EdgeRow, FaceTopology, StandardTopology, TrimRecord,
};
pub(super) use crate::solve::incidence::{
    compact_boundary_domain_viable, prepare_face_configuration_domains,
    prune_face_configuration_singleton_support, prune_face_configuration_support,
    prune_incidence_choices_with_deferred_support, prune_ordered_face_endpoint_support,
    reconstruct_incidence_candidates,
};
pub(super) use crate::solve::matching::unique_coordinate_bijection;
pub(super) use crate::solve::mesh_gauge::{
    canonicalize_mesh_vertex_labels, mesh_candidates_equivalent,
    mesh_candidates_equivalent_with_gauge,
};
pub(super) use crate::solve::mesh_quotient::{
    deduplicate_mesh_quotient_assignments, initial_mesh_quotient, mesh_assignment_can_merge,
    mesh_assignment_endpoint_cycles_viable, mesh_edge_points_compatible,
    mesh_face_endpoint_configurations, possible_face_choices, possible_face_choices_with_limit,
    possible_face_equations, prune_mesh_endpoint_pair_support,
    prune_mesh_endpoint_pair_support_with_limit, MeshPartialEndpointConstraint, MeshQuotient,
    MeshSelectionSearch, MAX_FACE_EQUATION_CACHE_ENTRIES, MAX_MESH_CONSTRAINT_OPERATIONS,
};
pub(super) use crate::solve::missing_edge::{
    bind_edge_port_candidates, bounded_endpoint_cycle_orders, bounded_oriented_trail_orders,
    face_endpoint_candidates_close, motif_port_points, propagate_edge_port_points,
    propagate_edge_port_points_with_ordered_seeds,
    propagate_partial_edge_port_points_with_ordered_seeds, resolve_edge_faces_from_runs,
    same_unordered_pair, unique_duplicate_face_assignment, FaceEndpointClosureOutcome,
    MeshBoundaryEdgeCandidate, MeshEdgeRun, MeshFaceBoundaryAssignment, MeshFaceBoundaryDomain,
};
pub(super) use crate::solve::UnionFind;
pub(super) use cadmpeg_core::decode::WorkBudget;

fn repeated_domain(domain: HashSet<usize>, count: usize) -> Vec<Arc<HashSet<usize>>> {
    let domain = Arc::new(domain);
    vec![domain; count]
}

fn sparse_degrees(faces: &[&[u8]]) -> Vec<BTreeMap<usize, u8>> {
    faces
        .iter()
        .map(|degrees| {
            degrees
                .iter()
                .copied()
                .enumerate()
                .filter_map(|(point, degree)| (degree != 0).then_some((point, degree)))
                .collect()
        })
        .collect()
}

fn triangle_packet(handles: [u16; 3]) -> Vec<u8> {
    let mut bytes = vec![0x01, 0x41, 0x01, 0xff, 0x03, 0x00, 0x00, 0x00];
    for handle in handles {
        bytes.extend_from_slice(&handle.to_be_bytes());
    }
    bytes
}

mod coordinate_closure;
mod incidence_components;
mod incidence_search;
mod mesh_quotient;
mod record_decoders;
mod trim;
