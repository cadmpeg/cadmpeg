// SPDX-License-Identifier: Apache-2.0
//! Certify a Join whose blind end is buried in existing material.

use super::{
    face_cap, oriented_loops, outlines_agree, parallel_plane, sample_regions, trimmed_selection,
    unique_state, Agreement, Cap, ParallelPlane, Piece, PlaneFrame, PARALLEL_CAP,
};
use crate::design::geometry::{atomic_sketch_regions, ProfileGeometryWork};
use crate::design::profile_select::{ResolvedProfileSelection, ScopedExtrudeProfileResolution};
use crate::history_records::{
    AsmHistoricalEnvelope, AsmHistoricalTopology, AsmHistoricalTransition,
};
use cadmpeg_core::decode::WorkBudget;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::operation_settings::BooleanOp;
use cadmpeg_ir::features::profiles::PlanarProfileRef;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::sketches::{Sketch, SketchEntityId, SketchGeometryDefinition};
use std::collections::{HashMap, HashSet};

const STRAIGHT_EDGE: f64 = 1.0e-9;
const MAX_JOIN_ENVELOPE_COMPARISONS: usize = 50_000_000;

type LoopReadings = Vec<Vec<(i64, Point3, Point3)>>;
type FaceLoopCache = HashMap<i64, Option<LoopReadings>>;

#[derive(Clone, Copy)]
struct Box3 {
    low: [f64; 3],
    high: [f64; 3],
}

impl Box3 {
    fn of(frame: &PlaneFrame, bounds: &AsmHistoricalEnvelope) -> Option<Self> {
        let mut low = [f64::INFINITY; 3];
        let mut high = [f64::NEG_INFINITY; 3];
        for x in [bounds.low.x, bounds.high.x] {
            for y in [bounds.low.y, bounds.high.y] {
                for z in [bounds.low.z, bounds.high.z] {
                    for (i, value) in position(frame, Point3::new(x, y, z))
                        .into_iter()
                        .enumerate()
                    {
                        if !value.is_finite() {
                            return None;
                        }
                        low[i] = low[i].min(value);
                        high[i] = high[i].max(value);
                    }
                }
            }
        }
        Some(Self { low, high })
    }

    fn contains(self, other: Self, tolerance: f64) -> bool {
        (0..3).all(|i| {
            other.low[i] >= self.low[i] - tolerance && other.high[i] <= self.high[i] + tolerance
        })
    }

    fn misses_interior(self, other: Self, tolerance: f64) -> bool {
        (0..3).any(|i| {
            other.high[i] <= self.low[i] + tolerance || other.low[i] >= self.high[i] - tolerance
        })
    }
}

fn position(frame: &PlaneFrame, point: Point3) -> [f64; 3] {
    let p = frame.project(point);
    [p.u, p.v, frame.height(point)]
}

fn unique<'a, T: 'a>(mut rows: impl Iterator<Item = &'a T>) -> Option<&'a T> {
    let row = rows.next()?;
    rows.next().is_none().then_some(row)
}

fn rectangle(cap: &Cap, far: f64, tolerance: f64) -> Option<(Box3, Vec<Piece>)> {
    let u = cap.corners.iter().map(|p| p.u);
    let v = cap.corners.iter().map(|p| p.v);
    let low = [
        u.clone().fold(f64::INFINITY, f64::min),
        v.clone().fold(f64::INFINITY, f64::min),
        far.min(0.0),
    ];
    let high = [
        u.fold(f64::NEG_INFINITY, f64::max),
        v.fold(f64::NEG_INFINITY, f64::max),
        far.max(0.0),
    ];
    if low.iter().chain(&high).any(|value| !value.is_finite())
        || !((high[0] - low[0]).is_finite() && (high[1] - low[1]).is_finite())
        || high[0] - low[0] <= tolerance
        || high[1] - low[1] <= tolerance
    {
        return None;
    }
    let corners = [
        Point2::new(low[0], low[1]),
        Point2::new(high[0], low[1]),
        Point2::new(high[0], high[1]),
        Point2::new(low[0], high[1]),
    ];
    let sides: Vec<_> = (0..4)
        .map(|i| Piece::Line(corners[i], corners[(i + 1) % 4]))
        .collect();
    let refs: Vec<_> = sides.iter().collect();
    let cap_refs: Vec<_> = cap.pieces.iter().collect();
    (cap.pieces
        .iter()
        .all(|piece| matches!(piece, Piece::Line(..)) && piece.covered_by(&refs, tolerance))
        && sides
            .iter()
            .all(|piece| piece.covered_by(&cap_refs, tolerance)))
    .then_some((Box3 { low, high }, sides))
}

fn body_faces(topology: &AsmHistoricalTopology, body: i64) -> Option<Vec<i64>> {
    for rows in [
        &topology.body_regions,
        &topology.region_shells,
        &topology.shell_faces,
    ] {
        let mut seen = HashSet::new();
        if rows
            .iter()
            .flat_map(|row| &row.member_refs)
            .any(|member| !seen.insert(*member))
        {
            return None;
        }
    }
    let relation = |rows: &[crate::history_records::AsmHistoricalRelation], owner| {
        let mut matching = rows.iter().filter(|row| row.owner_ref == owner);
        let values = matching.next()?.member_refs.clone();
        matching.next().is_none().then_some(values)
    };
    let mut faces = Vec::new();
    for region in relation(&topology.body_regions, body)? {
        for shell in relation(&topology.region_shells, region)? {
            faces.extend(relation(&topology.shell_faces, shell)?);
        }
    }
    Some(faces)
}

fn affected_body(topology: &AsmHistoricalTopology, faces: &[i64]) -> Option<i64> {
    let mut matching = topology.bodies.iter().filter(|body| {
        body_faces(topology, **body)
            .is_some_and(|owned| faces.iter().all(|face| owned.contains(face)))
    });
    let body = *matching.next()?;
    matching.next().is_none().then_some(body)
}

fn closed(topology: &AsmHistoricalTopology, body: i64) -> bool {
    let Some(faces) = body_faces(topology, body) else {
        return false;
    };
    let mut edges = HashMap::new();
    for face in faces {
        let Some(loops) = oriented_loops(topology, face) else {
            return false;
        };
        if loops.is_empty() {
            return false;
        }
        for (edge, _, _) in loops.into_iter().flatten() {
            *edges.entry(edge).or_insert(0usize) += 1;
        }
    }
    !edges.is_empty() && edges.values().all(|count| *count == 2)
}

fn same_edge_geometry(
    before: &AsmHistoricalTopology,
    after: &AsmHistoricalTopology,
    edge: i64,
    tolerance: f64,
    before_loops: &mut FaceLoopCache,
    after_loops: &mut FaceLoopCache,
) -> bool {
    let incident_loops: HashSet<_> = before
        .coedge_topology
        .iter()
        .filter(|row| row.edge == edge)
        .map(|row| row.owner_loop)
        .collect();
    for relation in &before.face_loops {
        if !relation
            .member_refs
            .iter()
            .any(|id| incident_loops.contains(id))
        {
            continue;
        }
        let Some(binding) = unique(
            before
                .face_surfaces
                .iter()
                .filter(|row| row.entity == relation.owner_ref),
        ) else {
            return false;
        };
        let Some(plane) = unique(
            before
                .surface_planes
                .iter()
                .filter(|row| row.surface == binding.carrier),
        ) else {
            continue;
        };
        if before
            .surface_planes
            .iter()
            .filter(|row| row.surface == binding.carrier)
            .ne(after
                .surface_planes
                .iter()
                .filter(|row| row.surface == binding.carrier))
        {
            return false;
        }
        let Some(normal) = plane.normal.unit() else {
            return false;
        };
        let Some(u) = normal
            .cross(Vector3::new(1.0, 0.0, 0.0))
            .unit()
            .or_else(|| normal.cross(Vector3::new(0.0, 1.0, 0.0)).unit())
        else {
            return false;
        };
        let frame = PlaneFrame {
            origin: plane.origin,
            normal,
            u,
            v: normal.cross(u),
        };
        let read = |topology: &AsmHistoricalTopology, cache: &mut FaceLoopCache| {
            let loops = cache
                .entry(relation.owner_ref)
                .or_insert_with(|| oriented_loops(topology, relation.owner_ref))
                .as_ref()?;
            let mut matching = loops.iter().flatten().filter(|(id, _, _)| *id == edge);
            let &(_, from, to) = matching.next()?;
            if matching.next().is_some() {
                return None;
            }
            let (piece, measured) = super::edge_piece(
                &frame,
                topology,
                relation.owner_ref,
                edge,
                (from, to),
                tolerance,
            )?;
            measured.is_none().then_some(piece)
        };
        let (Some(a), Some(b)) = (read(before, before_loops), read(after, after_loops)) else {
            return false;
        };
        return a.covered_by(&[&b], tolerance) && b.covered_by(&[&a], tolerance);
    }
    false
}

fn unchanged_faces_agree(
    before: &AsmHistoricalTopology,
    after: &AsmHistoricalTopology,
    transition: &AsmHistoricalTransition,
    body: i64,
    tolerance: f64,
) -> bool {
    let Some(faces) = body_faces(before, body) else {
        return false;
    };
    let Some(result_faces) = body_faces(after, body) else {
        return false;
    };
    if faces.iter().any(|face| {
        !result_faces.contains(face) && !transition.topology.faces.deleted.contains(face)
    }) || result_faces
        .iter()
        .any(|face| !faces.contains(face) && !transition.topology.faces.inserted.contains(face))
    {
        return false;
    }
    let stable = |slot| {
        !transition.records.updated.contains(&slot)
            && !transition.records.inserted.contains(&slot)
            && !transition.records.deleted.contains(&slot)
    };
    let mut before_loops = HashMap::new();
    let mut after_loops = HashMap::new();
    for face in faces {
        if transition.topology.faces.deleted.contains(&face)
            || transition.topology.faces.updated.contains(&face)
        {
            continue;
        }
        if !after.faces.contains(&face) {
            return false;
        }
        let Some(binding) = unique(before.face_surfaces.iter().filter(|row| row.entity == face))
        else {
            return false;
        };
        let plane = unique(
            before
                .surface_planes
                .iter()
                .filter(|row| row.surface == binding.carrier),
        );
        let cylinder = unique(
            before
                .surface_cylinders
                .iter()
                .filter(|row| row.surface == binding.carrier),
        );
        if (plane.is_none() && cylinder.is_none())
            || before
                .surface_planes
                .iter()
                .filter(|row| row.surface == binding.carrier)
                .ne(after
                    .surface_planes
                    .iter()
                    .filter(|row| row.surface == binding.carrier))
            || before
                .surface_cylinders
                .iter()
                .filter(|row| row.surface == binding.carrier)
                .ne(after
                    .surface_cylinders
                    .iter()
                    .filter(|row| row.surface == binding.carrier))
        {
            return false;
        }
        if !stable(binding.carrier)
            || after
                .face_surfaces
                .iter()
                .filter(|row| row.entity == face)
                .ne(before.face_surfaces.iter().filter(|row| row.entity == face))
            || after
                .face_envelopes
                .iter()
                .filter(|row| row.entity == face)
                .ne(before
                    .face_envelopes
                    .iter()
                    .filter(|row| row.entity == face))
        {
            return false;
        }
        let Some(loops) = unique(before.face_loops.iter().filter(|row| row.owner_ref == face))
        else {
            return false;
        };
        for loop_id in &loops.member_refs {
            if !stable(*loop_id) {
                return false;
            }
            let Some(ring) = unique(
                before
                    .loop_coedges
                    .iter()
                    .filter(|row| row.owner_ref == *loop_id),
            ) else {
                return false;
            };
            for coedge in &ring.member_refs {
                if !stable(*coedge) {
                    return false;
                }
                let Some(step) = unique(
                    before
                        .coedge_topology
                        .iter()
                        .filter(|row| row.coedge == *coedge),
                ) else {
                    return false;
                };
                if !stable(step.edge)
                    || !same_edge_geometry(
                        before,
                        after,
                        step.edge,
                        tolerance,
                        &mut before_loops,
                        &mut after_loops,
                    )
                {
                    return false;
                }
                let Some(binding) = unique(
                    before
                        .edge_curves
                        .iter()
                        .filter(|row| row.entity == step.edge),
                ) else {
                    return false;
                };
                if !binding.carrier.is_some_and(stable) {
                    return false;
                }
                let Some(edge) = unique(
                    before
                        .edge_vertices
                        .iter()
                        .filter(|row| row.edge == step.edge),
                ) else {
                    return false;
                };
                for vertex in [edge.start_vertex, edge.end_vertex] {
                    let Some(point) = unique(
                        before
                            .vertex_points
                            .iter()
                            .filter(|row| row.entity == vertex),
                    ) else {
                        return false;
                    };
                    if before
                        .vertex_points
                        .iter()
                        .filter(|row| row.entity == vertex)
                        .ne(after
                            .vertex_points
                            .iter()
                            .filter(|row| row.entity == vertex))
                        || before
                            .point_positions
                            .iter()
                            .filter(|row| row.point == point.carrier)
                            .ne(after
                                .point_positions
                                .iter()
                                .filter(|row| row.point == point.carrier))
                    {
                        return false;
                    }
                }
            }
        }
    }
    true
}

fn face_envelope(topology: &AsmHistoricalTopology, face: i64) -> Option<&AsmHistoricalEnvelope> {
    let bounds = unique(
        topology
            .face_envelopes
            .iter()
            .filter(|row| row.entity == face),
    )?;
    let mut boundary: Vec<_> = oriented_loops(topology, face)?
        .into_iter()
        .flatten()
        .map(|(edge, _, _)| edge)
        .collect();
    boundary.sort_unstable();
    if bounds.boundary != boundary || boundary.is_empty() {
        return None;
    }
    let contains = |point: Point3| {
        point.x >= bounds.low.x
            && point.x <= bounds.high.x
            && point.y >= bounds.low.y
            && point.y <= bounds.high.y
            && point.z >= bounds.low.z
            && point.z <= bounds.high.z
    };
    for edge in boundary {
        let edge = unique(
            topology
                .edge_envelopes
                .iter()
                .filter(|row| row.entity == edge),
        )?;
        if !contains(edge.low) || !contains(edge.high) {
            return None;
        }
    }
    Some(bounds)
}

fn filled(
    topology: &AsmHistoricalTopology,
    body: i64,
    frame: &PlaneFrame,
    prism: Box3,
    tolerance: f64,
) -> bool {
    let Some(faces) = body_faces(topology, body) else {
        return false;
    };
    let mut edges = HashMap::new();
    for face in faces {
        let Some(bounds) = face_envelope(topology, face) else {
            return false;
        };
        let Some(bounds) = Box3::of(frame, bounds) else {
            return false;
        };
        if (prism.low[2] == 0.0 && bounds.low[2] < -tolerance)
            || (prism.high[2] == 0.0 && bounds.high[2] > tolerance)
            || !prism.misses_interior(bounds, tolerance)
        {
            return false;
        }
        let Some(loops) = oriented_loops(topology, face) else {
            return false;
        };
        if loops.is_empty() {
            return false;
        }
        for (edge, _, _) in loops.into_iter().flatten() {
            *edges.entry(edge).or_insert(0usize) += 1;
        }
    }
    !edges.is_empty() && edges.values().all(|count| *count == 2)
}

struct PlaneEdges {
    origin: Point3,
    normal: Vector3,
    frame: PlaneFrame,
    pieces: [Vec<Piece>; 2],
    unchanged: [HashSet<i64>; 2],
}

fn outside_segments(
    from: Point3,
    to: Point3,
    frame: &PlaneFrame,
    prism: Box3,
    tolerance: f64,
) -> Vec<(Point3, Point3)> {
    let start = position(frame, from);
    let end = position(frame, to);
    let mut low: f64 = 0.0;
    let mut high: f64 = 1.0;
    for i in 0..3 {
        let direction = end[i] - start[i];
        if direction == 0.0 {
            if start[i] < prism.low[i] - tolerance || start[i] > prism.high[i] + tolerance {
                return vec![(from, to)];
            }
            continue;
        }
        let a = (prism.low[i] - start[i]) / direction;
        let b = (prism.high[i] - start[i]) / direction;
        low = low.max(a.min(b));
        high = high.min(a.max(b));
    }
    if low > high {
        return vec![(from, to)];
    }
    let at = |t| from.translated(to.vector_from(from), t);
    let mut pieces = Vec::new();
    if low > 0.0 {
        pieces.push((from, at(low)));
    }
    if high < 1.0 {
        pieces.push((at(high), to));
    }
    pieces
}

fn straight(
    topology: &AsmHistoricalTopology,
    edge: i64,
    from: Point3,
    to: Point3,
    tolerance: f64,
) -> bool {
    let Some(binding) = unique(topology.edge_curves.iter().filter(|row| row.entity == edge)) else {
        return false;
    };
    let Some(axis) = unique(
        topology
            .curve_axes
            .iter()
            .filter(|row| Some(row.curve) == binding.carrier),
    ) else {
        return false;
    };
    let direction = axis.direction;
    let chord = to.vector_from(from);
    direction.norm() > 0.0
        && chord.norm() > tolerance
        && direction.cross(chord).norm() <= STRAIGHT_EDGE * direction.norm() * chord.norm()
        && [from, to].into_iter().all(|point| {
            point.vector_from(axis.origin).cross(direction).norm() <= tolerance * direction.norm()
        })
}

fn parity(pieces: &[Piece], tolerance: f64, budget: &WorkBudget) -> Option<Vec<Piece>> {
    let endpoints: Vec<_> = pieces
        .iter()
        .flat_map(|piece| {
            let [a, _, b] = piece.points();
            [a, b]
        })
        .collect();
    let mut outline = Vec::new();
    for piece in pieces {
        if !budget.charge_by(endpoints.len()) {
            return None;
        }
        for fragment in super::compact::split_piece(piece, &endpoints, tolerance) {
            if !budget.charge_by(pieces.len().checked_add(outline.len())?) {
                return None;
            }
            let count = pieces
                .iter()
                .filter(|source| fragment.covered_by(&[*source], tolerance))
                .count();
            if count % 2 == 1 && !outline.iter().any(|other| fragment.same(other, tolerance)) {
                outline.push(fragment);
            }
        }
    }
    Some(outline)
}

fn outside_agrees(
    before: &AsmHistoricalTopology,
    after: &AsmHistoricalTopology,
    transition: &AsmHistoricalTransition,
    frame: &PlaneFrame,
    prism: Box3,
    tolerance: f64,
    budget: &WorkBudget,
) -> bool {
    let delta = &transition.topology.faces;
    let sides = [
        (before, delta.deleted.iter().chain(&delta.updated)),
        (after, delta.inserted.iter().chain(&delta.updated)),
    ];
    let mut planes: Vec<PlaneEdges> = Vec::new();
    for (side, (topology, faces)) in sides.into_iter().enumerate() {
        for face in faces {
            let Some(bounds) = face_envelope(topology, *face) else {
                return false;
            };
            let Some(envelope) = Box3::of(frame, bounds) else {
                return false;
            };
            if prism.contains(envelope, tolerance) {
                continue;
            }
            let Some(binding) = unique(
                topology
                    .face_surfaces
                    .iter()
                    .filter(|row| row.entity == *face),
            ) else {
                return false;
            };
            let Some(plane) = unique(
                topology
                    .surface_planes
                    .iter()
                    .filter(|row| row.surface == binding.carrier),
            ) else {
                return false;
            };
            let Some(normal) = plane.normal.unit() else {
                return false;
            };
            let index = match planes.iter().position(|row| {
                (row.normal.dot(normal).abs() - 1.0).abs() <= PARALLEL_CAP
                    && plane.origin.vector_from(row.origin).dot(normal).abs() <= tolerance
            }) {
                Some(index) => index,
                None => {
                    let Some(u) = normal
                        .cross(Vector3::new(1.0, 0.0, 0.0))
                        .unit()
                        .or_else(|| normal.cross(Vector3::new(0.0, 1.0, 0.0)).unit())
                    else {
                        return false;
                    };
                    planes.push(PlaneEdges {
                        origin: plane.origin,
                        normal,
                        frame: PlaneFrame {
                            origin: plane.origin,
                            normal,
                            u,
                            v: normal.cross(u),
                        },
                        pieces: [Vec::new(), Vec::new()],
                        unchanged: [HashSet::new(), HashSet::new()],
                    });
                    planes.len() - 1
                }
            };
            let target = &mut planes[index];
            let Some(loops) = oriented_loops(topology, *face) else {
                return false;
            };
            for (edge, from, to) in loops.into_iter().flatten() {
                let Some(bounds) = unique(
                    topology
                        .edge_envelopes
                        .iter()
                        .filter(|row| row.entity == edge),
                ) else {
                    return false;
                };
                let Some(envelope) = Box3::of(frame, bounds) else {
                    return false;
                };
                if prism.contains(envelope, tolerance) {
                    continue;
                }
                if straight(topology, edge, from, to, tolerance) {
                    for (a, b) in outside_segments(from, to, frame, prism, tolerance) {
                        if a.vector_from(b).norm() > tolerance {
                            target.pieces[side].push(Piece::Line(
                                target.frame.project(a),
                                target.frame.project(b),
                            ));
                        }
                    }
                } else {
                    let Some(curve) =
                        unique(topology.edge_curves.iter().filter(|row| row.entity == edge))
                            .and_then(|row| row.carrier)
                    else {
                        return false;
                    };
                    if transition.topology.edges.updated.contains(&edge)
                        || transition.topology.edges.inserted.contains(&edge)
                        || transition.topology.edges.deleted.contains(&edge)
                        || transition.topology.curves.updated.contains(&curve)
                        || transition.topology.curves.inserted.contains(&curve)
                        || transition.topology.curves.deleted.contains(&curve)
                    {
                        return false;
                    }
                    let Some((piece, measured)) = super::edge_piece(
                        &target.frame,
                        topology,
                        *face,
                        edge,
                        (from, to),
                        tolerance,
                    ) else {
                        return false;
                    };
                    if measured.is_some() {
                        return false;
                    }
                    target.pieces[side].push(piece);
                    if !target.unchanged[side].insert(edge) {
                        target.unchanged[side].remove(&edge);
                    }
                }
            }
        }
    }
    planes.into_iter().all(|plane| {
        if plane.unchanged[0] != plane.unchanged[1] {
            return false;
        }
        let Some(a) = parity(&plane.pieces[0], tolerance, budget) else {
            return false;
        };
        let Some(b) = parity(&plane.pieces[1], tolerance, budget) else {
            return false;
        };
        let ar: Vec<_> = a.iter().collect();
        let br: Vec<_> = b.iter().collect();
        a.iter().all(|piece| piece.covered_by(&br, tolerance))
            && b.iter().all(|piece| piece.covered_by(&ar, tolerance))
    })
}

fn comparison_allowance(
    before: &AsmHistoricalTopology,
    after: &AsmHistoricalTopology,
    transition: &AsmHistoricalTransition,
) -> Option<usize> {
    let mut size = transition
        .records
        .inserted
        .len()
        .max(transition.records.deleted.len())
        .max(transition.records.updated.len());
    for topology in [before, after] {
        let lengths = [
            topology.bodies.len(),
            topology.faces.len(),
            topology.loops.len(),
            topology.coedges.len(),
            topology.edges.len(),
            topology.vertices.len(),
            topology.surface_planes.len(),
            topology.surface_cylinders.len(),
            topology.curve_axes.len(),
            topology.face_envelopes.len(),
            topology.edge_envelopes.len(),
            topology.point_positions.len(),
            topology.edge_samples.len(),
        ];
        size = size.max(lengths.into_iter().max().unwrap_or(0));
        let references = [
            &topology.body_regions,
            &topology.region_shells,
            &topology.shell_faces,
            &topology.face_loops,
            &topology.loop_coedges,
        ]
        .into_iter()
        .flat_map(|rows| rows.iter())
        .try_fold(0usize, |total, row| {
            total.checked_add(row.member_refs.len())
        });
        let references = references?;
        size = size.max(references);
    }
    size.checked_mul(size)
        .and_then(|square| square.checked_mul(32))
        .filter(|work| *work <= MAX_JOIN_ENVELOPE_COMPARISONS)
}

pub(in crate::design::profile_select) fn candidate(
    sketch: &Sketch,
    resolution: ScopedExtrudeProfileResolution<'_>,
    states: (i64, i64),
    sweep: super::compact::Sweep<'_>,
    named: &[SketchEntityId],
) -> Result<Option<PlanarProfileRef>, CodecError> {
    if !matches!(sweep.operation, BooleanOp::Join) {
        return Ok(None);
    }
    let Some(far) = super::compact::blind_length(&sweep) else {
        return Ok(None);
    };
    let Some((after, state)) = unique_state(resolution.histories, states.0) else {
        return Ok(None);
    };
    let Some((before, _)) = unique_state(resolution.histories, states.1) else {
        return Ok(None);
    };
    let Some(transition) = state
        .transition
        .as_ref()
        .filter(|row| row.previous_state_id == Some(states.1))
    else {
        return Ok(None);
    };
    let Some(estimate) = comparison_allowance(before, after, transition) else {
        resolution
            .profile_work
            .note_join_envelope_capped(MAX_JOIN_ENVELOPE_COMPARISONS);
        return Ok(None);
    };
    let budget = WorkBudget::new(MAX_JOIN_ENVELOPE_COMPARISONS);
    if !budget.charge_by(estimate) {
        resolution
            .profile_work
            .note_join_envelope_capped(MAX_JOIN_ENVELOPE_COMPARISONS);
        return Ok(None);
    }
    let Some(frame) = PlaneFrame::of_sketch(sketch) else {
        return Ok(None);
    };
    let tolerance = resolution.linear_tolerance;
    let before_changed: Vec<_> = transition
        .topology
        .faces
        .deleted
        .iter()
        .chain(&transition.topology.faces.updated)
        .copied()
        .collect();
    let after_changed: Vec<_> = transition
        .topology
        .faces
        .inserted
        .iter()
        .chain(&transition.topology.faces.updated)
        .copied()
        .collect();
    if before_changed.is_empty() || after_changed.is_empty() {
        return Ok(None);
    }
    let Some(body) = affected_body(before, &before_changed) else {
        return Ok(None);
    };
    if affected_body(after, &after_changed) != Some(body)
        || !closed(before, body)
        || !unchanged_faces_agree(before, after, transition, body, tolerance)
    {
        return Ok(None);
    }
    let members: Vec<_> = resolution
        .entities
        .iter()
        .filter(|entity| entity.sketch == sketch.id && named.contains(entity.id()))
        .cloned()
        .collect();
    if members.is_empty()
        || members.iter().any(|entity| {
            !matches!(
                entity.geometry.definition(),
                SketchGeometryDefinition::Line { .. }
            )
        })
    {
        return Ok(None);
    }
    let work = ProfileGeometryWork::new();
    let Some(initial) = atomic_sketch_regions(sketch, &members, tolerance, &work, resolution.ctx)?
    else {
        return Ok(None);
    };

    if initial.len() != 1 {
        return Ok(None);
    }
    let Some(initial_sampled) = sample_regions(&initial, tolerance, &work) else {
        return Ok(None);
    };
    let Some(regions) = atomic_sketch_regions(
        sketch,
        resolution.entities,
        tolerance,
        &work,
        resolution.ctx,
    )?
    else {
        return Ok(None);
    };
    let Some(sampled) = sample_regions(&regions, tolerance, &work) else {
        return Ok(None);
    };
    if work.assumed_flags().into_iter().any(|flag| flag) {
        return Ok(None);
    }
    let mut found = None;
    let mut attempted = false;
    for face in &after_changed {
        if !matches!(parallel_plane(after, *face, frame.normal), ParallelPlane::At(point) if frame.height(point).abs() <= tolerance)
        {
            continue;
        }
        let Some(cap) = face_cap(&frame, after, *face, tolerance) else {
            continue;
        };
        let Some((prism, outline)) = rectangle(&cap, far, tolerance) else {
            continue;
        };
        if !matches!(
            outlines_agree(
                &cap,
                &initial_sampled,
                &[0],
                &members,
                tolerance,
                resolution.ctx
            )?,
            Agreement::Exact
        ) {
            continue;
        }
        if members.iter().any(|entity| {
            let SketchGeometryDefinition::Line { start, end } = *entity.geometry.definition()
            else {
                return true;
            };
            let piece = Piece::Line(start.get(), end.get());
            !outline.iter().any(|side| {
                let points = [
                    piece.points()[0],
                    piece.points()[2],
                    side.points()[0],
                    side.points()[2],
                ];
                super::compact::split_piece(side, &points, tolerance)
                    .iter()
                    .any(|part| {
                        let [a, _, b] = part.points();
                        super::distance(a, b) > tolerance && part.covered_by(&[&piece], tolerance)
                    })
            })
        }) {
            continue;
        }
        if attempted && !budget.charge_by(estimate) {
            resolution
                .profile_work
                .note_join_envelope_capped(MAX_JOIN_ENVELOPE_COMPARISONS);
            return Ok(None);
        }
        attempted = true;
        let fills = filled(after, body, &frame, prism, tolerance);
        let outside = outside_agrees(before, after, transition, &frame, prism, tolerance, &budget);

        if budget.exhausted() {
            resolution
                .profile_work
                .note_join_envelope_capped(MAX_JOIN_ENVELOPE_COMPARISONS);
        }
        if !fills || !outside {
            continue;
        }
        let selected: Vec<_> = sampled
            .iter()
            .enumerate()
            .filter(|(_, region)| cap.contains(region.point))
            .map(|(i, _)| i)
            .collect();
        if !matches!(
            outlines_agree(
                &cap,
                &sampled,
                &selected,
                resolution.entities,
                tolerance,
                resolution.ctx
            )?,
            Agreement::Exact
        ) {
            continue;
        }
        let Some(ResolvedProfileSelection::Regions(regions)) =
            trimmed_selection(&sampled, &selected, tolerance)
        else {
            continue;
        };
        let Ok(candidate) = PlanarProfileRef::sketch_regions(sketch.id.clone(), regions) else {
            continue;
        };
        if found
            .as_ref()
            .is_some_and(|previous| *previous != candidate)
        {
            return Ok(None);
        }
        found = Some(candidate);
    }

    Ok(found)
}

#[cfg(test)]
mod tests;
