// SPDX-License-Identifier: Apache-2.0
//! Body intersection, touching, and closure walks.

use super::topology_rows::{KeyedRows, ShellRanks};
use super::{EPS_HISTORY_SAME_AXIS_LINE_E8, EPS_HISTORY_SAME_AXIS_LINE_E9};
use crate::history_records::{AsmHistoricalTopology, AsmHistoricalTopologyDelta};
use std::collections::BTreeSet;

pub(crate) fn stable_ref(id: &str) -> Option<i64> {
    id.rsplit_once('#')?
        .1
        .split(':')
        .next()?
        .parse::<i64>()
        .ok()
}

pub(crate) fn same_axis_line(
    left: (cadmpeg_ir::math::Point3, cadmpeg_ir::math::Vector3),
    right: (cadmpeg_ir::math::Point3, cadmpeg_ir::math::Vector3),
) -> bool {
    let direction_dot = left.1.dot(right.1);
    if (direction_dot.abs() - 1.0).abs() > EPS_HISTORY_SAME_AXIS_LINE_E9 {
        return false;
    }
    let distance = right.0.vector_from(left.0).cross(left.1).norm();
    distance.is_finite() && distance <= EPS_HISTORY_SAME_AXIS_LINE_E8
}

pub(crate) fn changed_topology_family_refs(
    delta: &AsmHistoricalTopologyDelta,
    deleted: bool,
) -> BTreeSet<i64> {
    let families = [
        &delta.bodies,
        &delta.regions,
        &delta.shells,
        &delta.faces,
        &delta.loops,
        &delta.coedges,
        &delta.edges,
        &delta.vertices,
        &delta.points,
        &delta.surfaces,
        &delta.curves,
        &delta.pcurves,
    ];
    let mut changed = BTreeSet::new();
    for family in families {
        let members = if deleted {
            family.deleted.iter().chain([].iter())
        } else {
            family.inserted.iter().chain(family.updated.iter())
        };
        for &member in members {
            changed.insert(member);
        }
    }
    changed
}

#[cfg(test)]
pub(super) fn bodies_intersecting(
    topology: &AsmHistoricalTopology,
    changed: &BTreeSet<i64>,
) -> Option<BTreeSet<i64>> {
    bodies_touching(topology, &[changed]).and_then(|touched| touched.into_iter().next())
}

/// The bodies whose topological closure contains an entity of each query,
/// from one walk of the closures.
pub(super) fn bodies_touching(
    topology: &AsmHistoricalTopology,
    queries: &[&BTreeSet<i64>],
) -> Option<Vec<BTreeSet<i64>>> {
    let mut queried = queries
        .iter()
        .enumerate()
        .flat_map(|(query, entities)| entities.iter().map(move |&entity| (entity, query)))
        .collect::<Vec<_>>();
    queried.sort_unstable();
    let mut touched = vec![BTreeSet::new(); queries.len()];
    walk_body_closures(topology, |body, entity| {
        let start = queried.partition_point(|&(queried, _)| queried < entity);
        for &(_, query) in queried[start..]
            .iter()
            .take_while(|&&(queried, _)| queried == entity)
        {
            touched[query].insert(body);
        }
    })?;
    Some(touched)
}

/// Visit every entity of each body's topological closure, one body after
/// another. `None` when the closure reads a relation or carrier the topology
/// lacks.
pub(super) fn walk_body_closures(
    topology: &AsmHistoricalTopology,
    mut reach: impl FnMut(i64, i64),
) -> Option<()> {
    let body_regions = KeyedRows::new(&topology.body_regions);
    let region_shells = KeyedRows::new(&topology.region_shells);
    let shell_faces = KeyedRows::new(&topology.shell_faces);
    let shell_wire_edges = KeyedRows::new(&topology.shell_wire_edges);
    let shell_free_vertices = KeyedRows::new(&topology.shell_free_vertices);
    let face_loops = KeyedRows::new(&topology.face_loops);
    let loop_coedges = KeyedRows::new(&topology.loop_coedges);
    let coedges = KeyedRows::new(&topology.coedge_topology);
    let edges = KeyedRows::new(&topology.edge_vertices);
    let face_surfaces = KeyedRows::new(&topology.face_surfaces);
    let edge_curves = KeyedRows::new(&topology.edge_curves);
    let coedge_pcurves = KeyedRows::new(&topology.coedge_pcurves);
    let vertex_points = KeyedRows::new(&topology.vertex_points);
    let mut shell_edges = ShellRanks::new(&edges);
    let mut shell_vertices = ShellRanks::new(&vertex_points);
    for &body in &topology.bodies {
        reach(body, body);
        for &region in body_regions.members(body)? {
            reach(body, region);
            for &shell in region_shells.members(region)? {
                reach(body, shell);
                shell_edges.start_shell();
                shell_vertices.start_shell();
                for &edge in shell_wire_edges.members(shell)? {
                    shell_edges.visit(edges.rank(edge)?);
                }
                for &vertex in shell_free_vertices.members(shell)? {
                    shell_vertices.visit(vertex_points.rank(vertex)?);
                }
                for &face in shell_faces.members(shell)? {
                    reach(body, face);
                    reach(body, face_surfaces.get(face)?.carrier);
                    for &loop_ in face_loops.members(face)? {
                        reach(body, loop_);
                        for &coedge in loop_coedges.members(loop_)? {
                            reach(body, coedge);
                            shell_edges.visit(edges.rank(coedges.get(coedge)?.edge)?);
                            if let Some(pcurve) = coedge_pcurves
                                .get(coedge)
                                .and_then(|binding| binding.carrier)
                            {
                                reach(body, pcurve);
                            }
                        }
                    }
                }
                for &rank in &shell_edges.ranks {
                    let edge = edges.at(rank);
                    reach(body, edge.edge);
                    for vertex in [edge.start_vertex, edge.end_vertex] {
                        shell_vertices.visit(vertex_points.rank(vertex)?);
                    }
                    if let Some(curve) = edge_curves
                        .get(edge.edge)
                        .and_then(|binding| binding.carrier)
                    {
                        reach(body, curve);
                    }
                }
                for &rank in &shell_vertices.ranks {
                    let point = vertex_points.at(rank);
                    reach(body, point.entity);
                    reach(body, point.carrier);
                }
            }
        }
    }
    Some(())
}
