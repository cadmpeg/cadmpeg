// SPDX-License-Identifier: Apache-2.0
//! Cache the topology indexes used by historical face and edge recipes.
use super::cache::SnapshotCache;
use super::loop_index::LoopIndex;
use crate::history_records::AsmHistoricalTopology;
use std::collections::{HashMap, HashSet};

pub(super) type EdgeEndpoints = HashMap<i64, Option<[i64; 2]>>;

pub(super) type BoundaryEdges = HashMap<i64, HashSet<i64>>;

#[derive(Default)]
pub(super) struct TopologyQueryCache<'a> {
    pub(super) vertices: SnapshotCache<'a, AsmHistoricalTopology, VertexBoundaryIndex>,
    pub(super) endpoints: SnapshotCache<'a, AsmHistoricalTopology, EdgeEndpoints>,
    pub(super) boundaries: SnapshotCache<'a, AsmHistoricalTopology, BoundaryEdges>,
    pub(super) loops: SnapshotCache<'a, AsmHistoricalTopology, LoopIndex<'a>>,
}

pub(super) struct VertexBoundaryIndex {
    pub(super) boundaries: BoundaryEdges,
    pub(super) vertices: HashSet<i64>,
    pub(super) faces: HashSet<i64>,
}
impl VertexBoundaryIndex {
    pub(super) fn new(topology: &AsmHistoricalTopology) -> Self {
        let boundaries = super::face_boundary_edge_index(topology);
        let vertices = topology
            .vertices
            .iter()
            .copied()
            .collect::<std::collections::HashSet<_>>();

        let faces = topology
            .faces
            .iter()
            .copied()
            .collect::<std::collections::HashSet<_>>();
        Self {
            boundaries,
            vertices,
            faces,
        }
    }
}

pub(super) fn endpoint_index(topology: &AsmHistoricalTopology) -> EdgeEndpoints {
    let mut endpoints = HashMap::new();
    for edge in &topology.edge_vertices {
        endpoints
            .entry(edge.edge)
            .and_modify(|value| *value = None)
            .or_insert(Some([edge.start_vertex, edge.end_vertex]));
    }
    endpoints
}

pub(super) fn aggregate_boundaries(topology: &AsmHistoricalTopology) -> BoundaryEdges {
    let mut loop_coedges: HashMap<_, Vec<_>> = HashMap::new();
    for relation in &topology.loop_coedges {
        for member in &relation.member_refs {
            loop_coedges
                .entry(relation.owner_ref)
                .or_default()
                .push(*member);
        }
    }
    let mut coedge_edges: HashMap<_, Vec<_>> = HashMap::new();
    for coedge in &topology.coedge_topology {
        coedge_edges
            .entry(coedge.coedge)
            .or_default()
            .push(coedge.edge);
    }
    let mut boundaries = HashMap::<i64, HashSet<i64>>::new();
    for relation in &topology.face_loops {
        let edges = boundaries.entry(relation.owner_ref).or_default();
        for loop_slot in &relation.member_refs {
            for coedge in loop_coedges.get(loop_slot).into_iter().flatten() {
                for edge in coedge_edges.get(coedge).into_iter().flatten() {
                    edges.insert(*edge);
                }
            }
        }
    }
    boundaries
}
