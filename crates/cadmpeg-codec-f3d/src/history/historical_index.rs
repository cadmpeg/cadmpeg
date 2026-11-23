// SPDX-License-Identifier: Apache-2.0
//! Historical bounds and the historical body index.

use super::topology_rows::KeyedRows;
use super::EPS_HISTORY_CIRCLE_EDGE_E7;
use crate::history_records::{
    AsmHistoricalCarrierBinding, AsmHistoricalCoedge, AsmHistoricalEdge, AsmHistoricalPoint,
    AsmHistoricalRelation, AsmHistoricalTopology,
};
use std::collections::{BTreeSet, HashMap};

pub(super) struct HistoricalBounds {
    pub(super) bounds: Option<(cadmpeg_ir::math::Point3, cadmpeg_ir::math::Point3)>,
    pub(super) exact: bool,
    pub(super) missing_evidence: Vec<String>,
}

/// The keyed rows that decide the solidity and extent of one state's bodies.
pub(super) struct HistoricalBodyIndex<'a> {
    pub(super) body_regions: KeyedRows<'a, AsmHistoricalRelation>,
    pub(super) region_shells: KeyedRows<'a, AsmHistoricalRelation>,
    pub(super) shell_faces: KeyedRows<'a, AsmHistoricalRelation>,
    pub(super) shell_wire_edges: KeyedRows<'a, AsmHistoricalRelation>,
    pub(super) shell_free_vertices: KeyedRows<'a, AsmHistoricalRelation>,
    pub(super) face_loops: KeyedRows<'a, AsmHistoricalRelation>,
    pub(super) loop_coedges: KeyedRows<'a, AsmHistoricalRelation>,
    pub(super) coedges: KeyedRows<'a, AsmHistoricalCoedge>,
    pub(super) edges: KeyedRows<'a, AsmHistoricalEdge>,
    pub(super) vertex_points: KeyedRows<'a, AsmHistoricalCarrierBinding>,
    pub(super) point_positions: KeyedRows<'a, AsmHistoricalPoint>,
    pub(super) edge_curves:
        KeyedRows<'a, crate::history_records::AsmHistoricalOptionalCarrierBinding>,
    pub(super) curve_axes: KeyedRows<'a, crate::history_records::AsmHistoricalCurveAxis>,
    pub(super) face_surfaces: KeyedRows<'a, AsmHistoricalCarrierBinding>,
    pub(super) cylinders: KeyedRows<'a, crate::history_records::AsmHistoricalCylinder>,
    pub(super) edge_samples: KeyedRows<'a, crate::history_records::AsmHistoricalEdgeSample>,
    pub(super) planes: KeyedRows<'a, crate::history_records::AsmHistoricalPlane>,
    pub(super) edge_loops: HashMap<i64, Vec<i64>>,
    pub(super) loop_faces: HashMap<i64, i64>,
}

impl<'a> HistoricalBodyIndex<'a> {
    pub(super) fn new(topology: &'a AsmHistoricalTopology) -> Self {
        Self {
            body_regions: KeyedRows::new(&topology.body_regions),
            region_shells: KeyedRows::new(&topology.region_shells),
            shell_faces: KeyedRows::new(&topology.shell_faces),
            shell_wire_edges: KeyedRows::new(&topology.shell_wire_edges),
            shell_free_vertices: KeyedRows::new(&topology.shell_free_vertices),
            face_loops: KeyedRows::new(&topology.face_loops),
            loop_coedges: KeyedRows::new(&topology.loop_coedges),
            coedges: KeyedRows::new(&topology.coedge_topology),
            edges: KeyedRows::new(&topology.edge_vertices),
            vertex_points: KeyedRows::new(&topology.vertex_points),
            point_positions: KeyedRows::new(&topology.point_positions),
            edge_curves: KeyedRows::new(&topology.edge_curves),
            curve_axes: KeyedRows::new(&topology.curve_axes),
            face_surfaces: KeyedRows::new(&topology.face_surfaces),
            cylinders: KeyedRows::new(&topology.surface_cylinders),
            edge_samples: KeyedRows::new(&topology.edge_samples),
            planes: KeyedRows::new(&topology.surface_planes),
            edge_loops: topology.coedge_topology.iter().fold(
                HashMap::new(),
                |mut edge_loops: HashMap<i64, Vec<i64>>, coedge| {
                    edge_loops
                        .entry(coedge.edge)
                        .or_default()
                        .push(coedge.owner_loop);
                    edge_loops
                },
            ),
            loop_faces: topology
                .face_loops
                .iter()
                .flat_map(|face| {
                    face.member_refs
                        .iter()
                        .map(|loop_| (*loop_, face.owner_ref))
                })
                .collect(),
        }
    }

    pub(super) fn vertex_position(&self, vertex: i64) -> Option<cadmpeg_ir::math::Point3> {
        self.vertex_points
            .get(vertex)
            .and_then(|binding| self.point_positions.get(binding.carrier))
            .map(|point| point.position)
            .filter(|pos| pos.x.is_finite() && pos.y.is_finite() && pos.z.is_finite())
    }

    /// The centre, unit axis and radius of `edge` when it is proven circular:
    /// its curve has an axis, and a face that uses the edge lies on a cylinder
    /// with that axis whose radius is the start vertex's distance from it.
    pub(super) fn circle_edge(
        &self,
        edge: i64,
        start: cadmpeg_ir::math::Point3,
    ) -> Option<(cadmpeg_ir::math::Point3, cadmpeg_ir::math::Vector3, f64)> {
        let curve = self.edge_curves.get(edge)?.carrier?;
        let axis = self.curve_axes.get(curve)?;
        let normal = axis.direction.scale(axis.direction.norm().recip());
        let offset = start.vector_from(axis.origin);
        let center = axis.origin.translated(normal, offset.dot(normal));
        let radius = start.vector_from(center).norm();
        let tolerance = EPS_HISTORY_CIRCLE_EDGE_E7 * radius.max(1.0);
        let on_cylinder = self
            .edge_loops
            .get(&edge)
            .map_or(&[][..], Vec::as_slice)
            .iter()
            .filter_map(|loop_| self.loop_faces.get(loop_))
            .filter_map(|face| self.face_surfaces.get(*face))
            .filter_map(|binding| self.cylinders.get(binding.carrier))
            .any(|cylinder| {
                let cylinder_axis = cylinder.axis.scale(cylinder.axis.norm().recip());
                let apart = center.vector_from(cylinder.origin);
                cylinder_axis.cross(normal).norm() <= EPS_HISTORY_CIRCLE_EDGE_E7
                    && apart.cross(cylinder_axis).norm() <= tolerance
                    && (cylinder.radius - radius).abs() <= tolerance
            });
        (on_cylinder && radius > tolerance).then_some((center, normal, radius))
    }

    pub(super) fn shells(&self, body: i64) -> impl Iterator<Item = i64> + '_ {
        self.body_regions
            .members(body)
            .unwrap_or_default()
            .iter()
            .flat_map(move |&region| self.region_shells.members(region).unwrap_or_default())
            .copied()
    }

    /// The edge of each coedge in the loops of `face`, once per coedge.
    pub(super) fn face_edges(&self, face: i64) -> impl Iterator<Item = i64> + '_ {
        self.face_loops
            .members(face)
            .unwrap_or_default()
            .iter()
            .flat_map(move |&loop_| self.loop_coedges.members(loop_).unwrap_or_default())
            .filter_map(move |&coedge| self.coedges.get(coedge))
            .map(|coedge| coedge.edge)
    }

    /// The edges of a solid `body`, or `None` when the body has no shell or
    /// has a shell that is not closed.
    pub(super) fn solid_edges(&self, body: i64) -> Option<Vec<i64>> {
        let mut solid_edges = Vec::new();
        let mut has_shell = false;
        for shell in self.shells(body) {
            let occupied = |rows: &KeyedRows<'_, AsmHistoricalRelation>| {
                rows.members(shell)
                    .is_some_and(|members| !members.is_empty())
            };
            if occupied(&self.shell_wire_edges) || occupied(&self.shell_free_vertices) {
                return None;
            }
            let faces = self.shell_faces.members(shell).unwrap_or_default();
            if faces.is_empty() {
                return None;
            }
            let mut coedge_edges = faces
                .iter()
                .flat_map(|&face| self.face_edges(face))
                .collect::<Vec<_>>();
            coedge_edges.sort_unstable();
            if coedge_edges
                .chunk_by(|left, right| left == right)
                .any(|uses| uses.len() < 2)
            {
                return None;
            }
            coedge_edges.dedup();
            solid_edges.append(&mut coedge_edges);
            has_shell = true;
        }
        has_shell.then_some(solid_edges)
    }

    pub(super) fn is_solid(&self, body: i64) -> bool {
        self.solid_edges(body).is_some()
    }

    /// Whether `edge` is a proven straight segment: its curve has an axis
    /// line that holds both of its distinct end points.
    pub(super) fn line_edge(
        &self,
        edge: i64,
        start: cadmpeg_ir::math::Point3,
        end: cadmpeg_ir::math::Point3,
    ) -> bool {
        let Some(axis) = self
            .edge_curves
            .get(edge)
            .and_then(|binding| binding.carrier)
            .and_then(|curve| self.curve_axes.get(curve))
        else {
            return false;
        };
        let length = axis.direction.norm();
        let chord = end.vector_from(start);
        let tolerance = EPS_HISTORY_CIRCLE_EDGE_E7 * chord.norm().max(1.0);
        let off_axis = |point: cadmpeg_ir::math::Point3| {
            point.vector_from(axis.origin).cross(axis.direction).norm() / length
        };
        length > 0.0
            && chord.norm() > tolerance
            && off_axis(start) <= tolerance
            && off_axis(end) <= tolerance
    }

    /// Whether every face of `body` lies on a plane or a cylinder, so that
    /// the body's box is the box of its edges.
    pub(super) fn faces_bound_by_edges(&self, body: i64) -> bool {
        let mut faces = self
            .shells(body)
            .flat_map(|shell| self.shell_faces.members(shell).unwrap_or_default())
            .peekable();
        faces.peek().is_some()
            && faces.all(|face| {
                self.face_surfaces.get(*face).is_some_and(|binding| {
                    self.planes.get(binding.carrier).is_some()
                        || self.cylinders.get(binding.carrier).is_some()
                })
            })
    }

    /// The box of a solid `body` from its `edges`, and whether that box is
    /// exact: every edge is a proven segment or circular arc and every face a
    /// plane or a cylinder.
    pub(super) fn body_bounds(&self, body: i64, edges: &[i64]) -> HistoricalBounds {
        let mut bounds = self.edge_bounds(edges);
        bounds.exact = bounds.missing_evidence.is_empty()
            && self.faces_bound_by_edges(body)
            && edges.iter().all(|&edge| {
                let Some(row) = self.edges.get(edge) else {
                    return false;
                };
                let (Some(start), Some(end)) = (
                    self.vertex_position(row.start_vertex),
                    self.vertex_position(row.end_vertex),
                ) else {
                    return false;
                };
                self.line_edge(edge, start, end)
                    || (self.circle_edge(edge, start).is_some()
                        && (row.start_vertex == row.end_vertex
                            || self.edge_samples.get(edge).is_some()))
            });
        bounds
    }

    pub(super) fn has_complete_solid_topology(&self, body: i64) -> bool {
        let Some(regions) = self.body_regions.members(body) else {
            return false;
        };
        if regions.is_empty() {
            return false;
        }
        for &region in regions {
            let Some(shells) = self.region_shells.members(region) else {
                return false;
            };
            if shells.is_empty() {
                return false;
            }
            for &shell in shells {
                let occupied = |rows: &KeyedRows<'_, AsmHistoricalRelation>| {
                    rows.members(shell)
                        .is_some_and(|members| !members.is_empty())
                };
                if occupied(&self.shell_wire_edges) || occupied(&self.shell_free_vertices) {
                    return false;
                }
                let Some(faces) = self.shell_faces.members(shell) else {
                    return false;
                };
                if faces.is_empty() {
                    return false;
                }
                for &face in faces {
                    let Some(loops) = self.face_loops.members(face) else {
                        return false;
                    };
                    if loops.is_empty() {
                        return false;
                    }
                    for &loop_ in loops {
                        let Some(coedges) = self.loop_coedges.members(loop_) else {
                            return false;
                        };
                        if coedges.is_empty()
                            || coedges
                                .iter()
                                .any(|&coedge| self.coedges.get(coedge).is_none())
                        {
                            return false;
                        }
                    }
                }
            }
        }
        true
    }

    pub(super) fn edge_vertex_positions(
        &self,
        edges: &[i64],
    ) -> Option<Vec<cadmpeg_ir::math::Point3>> {
        if edges.is_empty() {
            return None;
        }
        let mut vertices = Vec::with_capacity(edges.len() * 2);
        for &edge in edges {
            let edge = self.edges.get(edge)?;
            vertices.push(edge.start_vertex);
            vertices.push(edge.end_vertex);
        }
        vertices.sort_unstable();
        vertices.dedup();
        let mut positions = Vec::with_capacity(vertices.len());
        for vertex in vertices {
            positions.push(self.vertex_position(vertex)?);
        }
        Some(positions)
    }

    /// The box of the edges: their vertices, their interior samples, and
    /// the axis extremes of every proven circular arc among them.
    pub(super) fn edge_bounds(&self, edges: &[i64]) -> HistoricalBounds {
        let mut points = Vec::new();
        let mut missing_evidence = Vec::new();
        for &edge_id in edges {
            let Some(edge) = self.edges.get(edge_id) else {
                missing_evidence.push(format!("edge {edge_id} has no topology"));
                continue;
            };
            let start = self.vertex_position(edge.start_vertex);
            let end = self.vertex_position(edge.end_vertex);
            for (vertex, position) in [(edge.start_vertex, start), (edge.end_vertex, end)] {
                if position.is_none() {
                    missing_evidence
                        .push(format!("edge {edge_id} vertex {vertex} has no position"));
                }
            }
            let sample = self.edge_samples.get(edge_id);
            let interior = sample.map(|sample| sample.interior);
            let circle = start.and_then(|start| self.circle_edge(edge_id, start));
            let closed = edge.start_vertex == edge.end_vertex;
            let sampled_bounds = sample.and_then(|sample| sample.bounds);
            let arc_resolved = circle.is_some() && (closed || interior.is_some());
            let segment = start
                .zip(end)
                .is_some_and(|(start, end)| self.line_edge(edge_id, start, end));
            if sampled_bounds.is_none() && !segment && !arc_resolved {
                missing_evidence.push(format!("edge {edge_id} has no complete interval samples"));
            }
            let mut edge_points = Vec::new();
            edge_points.extend(start);
            edge_points.extend(end);
            edge_points.extend(interior);
            edge_points.extend(sampled_bounds.into_iter().flatten());
            if let (Some((center, normal, radius)), Some(start), Some(end)) = (circle, start, end) {
                edge_points.extend(super::arc_extremes::circle_arc_extremes(
                    center, normal, radius, start, interior, end, closed,
                ));
            }
            if edge_points
                .iter()
                .any(|p| !p.x.is_finite() || !p.y.is_finite() || !p.z.is_finite())
            {
                missing_evidence.push(format!("edge {edge_id} has nonfinite samples"));
                edge_points.retain(|p| p.x.is_finite() && p.y.is_finite() && p.z.is_finite());
            }
            points.extend(edge_points);
        }
        let bounds = points.first().copied().map(|first| {
            points.iter().fold((first, first), |(low, high), point| {
                (
                    cadmpeg_ir::math::Point3::new(
                        low.x.min(point.x),
                        low.y.min(point.y),
                        low.z.min(point.z),
                    ),
                    cadmpeg_ir::math::Point3::new(
                        high.x.max(point.x),
                        high.y.max(point.y),
                        high.z.max(point.z),
                    ),
                )
            })
        });
        HistoricalBounds {
            bounds,
            exact: false,
            missing_evidence,
        }
    }

    pub(super) fn body_faces(&self, body: i64) -> Option<BTreeSet<i64>> {
        let regions = self.body_regions.members(body)?;
        let mut faces = BTreeSet::new();
        for &region in regions {
            let shells = self.region_shells.members(region)?;
            for &shell in shells {
                let shell_faces = self.shell_faces.members(shell)?;
                faces.extend(shell_faces.iter().copied());
            }
        }
        (!faces.is_empty()).then_some(faces)
    }
}
