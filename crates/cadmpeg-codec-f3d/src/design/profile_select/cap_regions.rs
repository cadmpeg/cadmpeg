// SPDX-License-Identifier: Apache-2.0
//! Prove an Extrude's sketch regions from the end cap its own transition
//! inserts.
//!
//! A planar face that an Extrude transition inserts parallel to its Sketch,
//! at one offset from the Sketch plane, repeats the swept region there. The
//! regions are the bounded faces of the arrangement of every
//! non-construction Sketch curve. A region whose interior lies inside the cap
//! is selected; the selection is admitted only when the selected regions
//! cover the cap's area exactly.

use std::collections::HashMap;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::math::{Point2, Point3};

use super::ResolvedProfileSelection;
use crate::design::geometry::{
    atomic_sketch_regions, point_in_polygon, point_segment_distance, profile_loop_contains_points,
    project_to_sketch, AtomicSketchRegion, ProfileGeometryWork,
};
use crate::history_records::{AsmDeltaState, AsmHistoricalTopology, AsmHistory};

/// Line segments one full circle or arc of a cap boundary is drawn with.
const ARC_SEGMENTS: u32 = 64;
/// Grid lines per axis searched for a point well inside a region.
const SAMPLE_GRID: u32 = 24;
/// Relative difference allowed between the cap area and the selected area.
/// Both sides are polylines of the same circles and arcs.
const AREA_AGREEMENT: f64 = 1e-3;
/// How far from parallel, as a cosine, a cap's plane normal may be to the sketch normal.
const PARALLEL_CAP: f64 = 1e-9;
/// Most loops and loop edges one cap may have.
const CAP_EDGES: usize = 4096;

/// One planar cap: its loops as closed sketch-plane polylines.
struct Cap {
    loops: Vec<Vec<Point2>>,
    area: f64,
}

impl Cap {
    fn contains(&self, point: Point2) -> bool {
        self.loops
            .iter()
            .filter(|ring| point_in_polygon(point, ring))
            .count()
            % 2
            == 1
    }
}

/// One arrangement region with a point well inside it and its area net of
/// the regions nested in it.
struct Sampled<'a> {
    region: &'a AtomicSketchRegion,
    point: Point2,
    area: f64,
    nested: Vec<usize>,
}

/// The regions the cap of the transition from `previous_state_id` to
/// `state_id` proves, when `current` covers other regions or is unknown.
/// `None` when the cap proves nothing, or proves `current`.
pub(super) fn cap_profile_selection(
    sketch: &cadmpeg_ir::sketches::Sketch,
    resolution: super::ScopedExtrudeProfileResolution<'_>,
    state_id: i64,
    previous_state_id: i64,
    current: Option<&ResolvedProfileSelection>,
) -> Result<Option<ResolvedProfileSelection>, CodecError> {
    let entities = resolution.entities;
    let histories = resolution.histories;
    let tolerance = resolution.linear_tolerance;
    let work = resolution.profile_work;
    let ctx = resolution.ctx;
    let Some((topology, state)) = unique_state(histories, state_id) else {
        return Ok(None);
    };
    let Some(transition) = state.transition.as_ref() else {
        return Ok(None);
    };
    if transition.previous_state_id != Some(previous_state_id) {
        return Ok(None);
    }
    let Some(caps) = far_caps(
        sketch,
        topology,
        &transition.topology.faces.inserted,
        tolerance,
    ) else {
        return Ok(None);
    };
    let Some(regions) = atomic_sketch_regions(sketch, entities, tolerance, work, ctx)? else {
        return Ok(None);
    };
    let Some(sampled) = sample_regions(&regions, tolerance) else {
        return Ok(None);
    };
    let mut selected = Vec::new();
    for cap in &caps {
        let mut covered = 0.0;
        for (index, region) in sampled.iter().enumerate() {
            if cap.contains(region.point) {
                covered += region.area;
                if !selected.contains(&index) {
                    selected.push(index);
                }
            }
        }
        if (covered - cap.area).abs() > AREA_AGREEMENT * cap.area.max(1.0) {
            return Ok(None);
        }
    }
    if selected.is_empty() {
        return Ok(None);
    }
    selected.sort_unstable();
    if let Some(current) = current {
        let points: Vec<Point2> = sampled.iter().map(|region| region.point).collect();
        if let Some(covered) =
            covered_regions(sketch, entities, current, &points, tolerance, work, ctx)?
        {
            if covered == selected {
                return Ok(None);
            }
        }
    }
    let mut out = Vec::new();
    for &index in &selected {
        let region = &sampled[index];
        let holes = region
            .nested
            .iter()
            .filter(|nested| !selected.contains(nested))
            .map(|nested| {
                sampled[*nested]
                    .region
                    .boundary
                    .iter()
                    .rev()
                    .map(|use_| {
                        let mut use_ = use_.clone();
                        use_.reversed = !use_.reversed;
                        use_
                    })
                    .collect()
            })
            .collect();
        let Ok(region) = cadmpeg_ir::features::SketchProfileRegion::trimmed(
            region.region.boundary.clone(),
            holes,
        ) else {
            return Ok(None);
        };
        out.push(region);
    }
    Ok(Some(ResolvedProfileSelection::Regions(out)))
}

fn unique_state(
    histories: &[AsmHistory],
    state_id: i64,
) -> Option<(&AsmHistoricalTopology, &AsmDeltaState)> {
    let mut states = histories
        .iter()
        .flat_map(|history| history.states().iter().map(move |state| (history, state)))
        .filter(|(_, state)| state.state_id == state_id);
    let (history, state) = states.next()?;
    if states.next().is_some() {
        return None;
    }
    Some((history.topology(state)?, state))
}

/// The inserted planar faces parallel to the sketch at the offset farthest
/// from its plane, each with every loop resolved.
fn far_caps(
    sketch: &cadmpeg_ir::sketches::Sketch,
    topology: &AsmHistoricalTopology,
    inserted: &[i64],
    tolerance: f64,
) -> Option<Vec<Cap>> {
    let (origin, normal, _) = sketch.resolved_placement()?;
    let normal = normal.get();
    let offset = |point: Point3| point.vector_from(origin.get()).dot(normal);
    let surfaces: HashMap<i64, i64> = topology
        .face_surfaces
        .iter()
        .map(|binding| (binding.entity, binding.carrier))
        .collect();
    let mut caps: Vec<(f64, Cap)> = Vec::new();
    for face in inserted {
        let Some(plane) = surfaces.get(face).and_then(|surface| {
            topology
                .surface_planes
                .iter()
                .find(|plane| plane.surface == *surface)
        }) else {
            continue;
        };
        if (plane.normal.dot(normal).abs() - 1.0).abs() > PARALLEL_CAP {
            continue;
        }
        let height = offset(plane.origin);
        let cap = face_cap(sketch, topology, *face, tolerance)?;
        caps.push((height, cap));
    }
    let far = caps
        .iter()
        .map(|(height, _)| *height)
        .max_by(|a, b| a.abs().total_cmp(&b.abs()).then(a.total_cmp(b)))?;
    let caps: Vec<Cap> = caps
        .into_iter()
        .filter(|(height, _)| (height - far).abs() <= tolerance)
        .map(|(_, cap)| cap)
        .collect();
    (!caps.is_empty()).then_some(caps)
}

/// Every loop of `face` as a sketch-plane polyline: lines through their
/// ends, circles and arcs through their ends and carrier midpoint.
fn face_cap(
    sketch: &cadmpeg_ir::sketches::Sketch,
    topology: &AsmHistoricalTopology,
    face: i64,
    tolerance: f64,
) -> Option<Cap> {
    let position = |vertex: i64| -> Option<Point3> {
        let point = topology
            .vertex_points
            .iter()
            .find(|binding| binding.entity == vertex)?
            .carrier;
        Some(
            topology
                .point_positions
                .iter()
                .find(|row| row.point == point)?
                .position,
        )
    };
    let mut loops = Vec::new();
    let mut edges_seen = 0usize;
    for loop_ in relation(&topology.face_loops, face)? {
        let coedges = relation(&topology.loop_coedges, *loop_)?;
        edges_seen = edges_seen.checked_add(coedges.len())?;
        if coedges.is_empty() || edges_seen > CAP_EDGES {
            return None;
        }
        let mut ring: Vec<Point2> = Vec::new();
        let mut previous_end: Option<i64> = None;
        for coedge in coedges {
            let edge = topology
                .coedge_topology
                .iter()
                .find(|row| row.coedge == *coedge)?
                .edge;
            let ends = topology.edge_vertices.iter().find(|row| row.edge == edge)?;
            let (start, end) = match previous_end {
                Some(vertex) if vertex == ends.end_vertex && vertex != ends.start_vertex => {
                    (ends.end_vertex, ends.start_vertex)
                }
                Some(vertex) if vertex != ends.start_vertex => {
                    // The first edge's direction was a guess; turn it round.
                    if ring.len() > 1 || coedges.len() < 2 {
                        return None;
                    }
                    ring.reverse();
                    if vertex == ends.end_vertex {
                        (ends.end_vertex, ends.start_vertex)
                    } else {
                        (ends.start_vertex, ends.end_vertex)
                    }
                }
                _ => (ends.start_vertex, ends.end_vertex),
            };
            let from = project_to_sketch(sketch, position(start)?)?;
            let to = project_to_sketch(sketch, position(end)?)?;
            let mid = topology
                .edge_samples
                .iter()
                .find(|row| row.edge == edge)
                .and_then(|row| project_to_sketch(sketch, row.interior));
            push_edge(&mut ring, from, mid, to, tolerance)?;
            previous_end = Some(end);
        }
        if ring.len() < 3 {
            return None;
        }
        loops.push(ring);
    }
    let mut areas: Vec<f64> = loops.iter().map(|ring| polygon_area(ring).abs()).collect();
    let outer = areas
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.total_cmp(b.1))
        .map(|(index, _)| index)?;
    let outer_area = areas.swap_remove(outer);
    let area = outer_area - areas.iter().sum::<f64>();
    (area > tolerance * tolerance).then_some(Cap { loops, area })
}

/// Appends the polyline of one edge, without its end, to `ring`.
fn push_edge(
    ring: &mut Vec<Point2>,
    from: Point2,
    mid: Option<Point2>,
    to: Point2,
    tolerance: f64,
) -> Option<()> {
    let near = |a: Point2, b: Point2| (a.u - b.u).hypot(a.v - b.v) <= tolerance;
    let Some(mid) = mid else {
        // No carrier sample: only a straight edge is drawn.
        if near(from, to) {
            return None;
        }
        ring.push(from);
        return Some(());
    };
    if near(from, to) {
        // A closed circle through its seam and the far end of its diameter.
        let center = Point2::new(f64::midpoint(from.u, mid.u), f64::midpoint(from.v, mid.v));
        let radius = (from.u - center.u).hypot(from.v - center.v);
        if radius <= tolerance {
            return None;
        }
        let start = (from.v - center.v).atan2(from.u - center.u);
        for k in 0..ARC_SEGMENTS {
            let angle = start + std::f64::consts::TAU * f64::from(k) / f64::from(ARC_SEGMENTS);
            ring.push(Point2::new(
                center.u + radius * angle.cos(),
                center.v + radius * angle.sin(),
            ));
        }
        return Some(());
    }
    if point_segment_distance(mid, (from, to)) <= tolerance {
        ring.push(from);
        return Some(());
    }
    let center = circle_through(from, mid, to)?;
    let radius = (from.u - center.u).hypot(from.v - center.v);
    let angle = |p: Point2| (p.v - center.v).atan2(p.u - center.u);
    let (a0, am, a1) = (angle(from), angle(mid), angle(to));
    let tau = std::f64::consts::TAU;
    let ccw = |a: f64, b: f64| (b - a).rem_euclid(tau);
    let sweep = if ccw(a0, am) <= ccw(a0, a1) {
        ccw(a0, a1)
    } else {
        -ccw(a1, a0)
    };
    for k in 0..ARC_SEGMENTS {
        let angle = a0 + sweep * f64::from(k) / f64::from(ARC_SEGMENTS);
        ring.push(Point2::new(
            center.u + radius * angle.cos(),
            center.v + radius * angle.sin(),
        ));
    }
    Some(())
}

/// The members of the one row `owner` owns in `rows`.
fn relation(rows: &[crate::history_records::AsmHistoricalRelation], owner: i64) -> Option<&[i64]> {
    let mut rows = rows.iter().filter(|row| row.owner_ref == owner);
    let row = rows.next()?;
    rows.next().is_none().then_some(row.member_refs.as_slice())
}

fn circle_through(first: Point2, second: Point2, third: Point2) -> Option<Point2> {
    let det = 2.0
        * (first.u * (second.v - third.v)
            + second.u * (third.v - first.v)
            + third.u * (first.v - second.v));
    if det.abs() <= f64::EPSILON {
        return None;
    }
    let sq = |point: Point2| point.u * point.u + point.v * point.v;
    let center_u = (sq(first) * (second.v - third.v)
        + sq(second) * (third.v - first.v)
        + sq(third) * (first.v - second.v))
        / det;
    let center_v = (sq(first) * (third.u - second.u)
        + sq(second) * (first.u - third.u)
        + sq(third) * (second.u - first.u))
        / det;
    (center_u.is_finite() && center_v.is_finite()).then(|| Point2::new(center_u, center_v))
}

fn polygon_area(ring: &[Point2]) -> f64 {
    cadmpeg_ir::math::planar::polygon_area_twice(ring)
        .map_or(f64::NAN, cadmpeg_ir::scalar::FiniteReal::get)
        * 0.5
}

/// Each region with a point well inside it, outside the regions nested in it,
/// and its area net of them. `None` when a region has no such point.
fn sample_regions(regions: &[AtomicSketchRegion], tolerance: f64) -> Option<Vec<Sampled<'_>>> {
    let areas: Vec<f64> = regions
        .iter()
        .map(|region| polygon_area(&region.polyline).abs())
        .collect();
    let clearance = |point: Point2, ring: &[Point2]| -> f64 {
        (0..ring.len())
            .map(|i| point_segment_distance(point, (ring[i], ring[(i + 1) % ring.len()])))
            .fold(f64::INFINITY, f64::min)
    };
    let inside = |inner: &[Point2], outer: &[Point2]| {
        inner
            .iter()
            .all(|p| point_in_polygon(*p, outer) || clearance(*p, outer) <= tolerance)
            && inner
                .iter()
                .any(|p| point_in_polygon(*p, outer) && clearance(*p, outer) > tolerance)
    };
    let mut contained: Vec<Vec<usize>> = vec![Vec::new(); regions.len()];
    for (outer, region) in regions.iter().enumerate() {
        for (inner, other) in regions.iter().enumerate() {
            if inner != outer
                && areas[inner] < areas[outer]
                && inside(&other.polyline, &region.polyline)
            {
                contained[outer].push(inner);
            }
        }
    }
    let mut out = Vec::new();
    for (index, region) in regions.iter().enumerate() {
        // Only the outermost nested regions take area away from this one.
        let nested: Vec<usize> = contained[index]
            .iter()
            .copied()
            .filter(|inner| {
                !contained[index]
                    .iter()
                    .any(|middle| middle != inner && contained[*middle].contains(inner))
            })
            .collect();
        let area = areas[index] - nested.iter().map(|inner| areas[*inner]).sum::<f64>();
        let (mut low, mut high) = (
            Point2::new(f64::INFINITY, f64::INFINITY),
            Point2::new(f64::NEG_INFINITY, f64::NEG_INFINITY),
        );
        for p in &region.polyline {
            low = Point2::new(low.u.min(p.u), low.v.min(p.v));
            high = Point2::new(high.u.max(p.u), high.v.max(p.v));
        }
        let mut best: Option<(Point2, f64)> = None;
        for i in 1..SAMPLE_GRID {
            for j in 1..SAMPLE_GRID {
                let point = Point2::new(
                    low.u + (high.u - low.u) * f64::from(i) / f64::from(SAMPLE_GRID),
                    low.v + (high.v - low.v) * f64::from(j) / f64::from(SAMPLE_GRID),
                );
                if !point_in_polygon(point, &region.polyline)
                    || nested
                        .iter()
                        .any(|inner| point_in_polygon(point, &regions[*inner].polyline))
                {
                    continue;
                }
                let room = nested
                    .iter()
                    .map(|inner| clearance(point, &regions[*inner].polyline))
                    .fold(clearance(point, &region.polyline), f64::min);
                if best.is_none_or(|(_, most)| room > most) {
                    best = Some((point, room));
                }
            }
        }
        let (point, room) = best?;
        if room <= 10.0 * tolerance || area <= tolerance * tolerance {
            return None;
        }
        out.push(Sampled {
            region,
            point,
            area,
            nested,
        });
    }
    Some(out)
}

/// The indices of the regions whose points `current` covers, in order;
/// `None` when that cannot be decided.
fn covered_regions(
    sketch: &cadmpeg_ir::sketches::Sketch,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    current: &ResolvedProfileSelection,
    points: &[Point2],
    tolerance: f64,
    work: &ProfileGeometryWork,
    ctx: &DecodeContext<'_>,
) -> Result<Option<Vec<usize>>, CodecError> {
    let contains = |index: u32| -> Result<Option<Vec<bool>>, CodecError> {
        let Some(profile) = usize::try_from(index)
            .ok()
            .and_then(|index| sketch.profiles.get(index))
        else {
            return Ok(None);
        };
        profile_loop_contains_points(profile, entities, points, tolerance, work, ctx)
    };
    let mut inside = vec![false; points.len()];
    match current {
        ResolvedProfileSelection::Loops(loops) => {
            for index in loops {
                let Some(hits) = contains(*index)? else {
                    return Ok(None);
                };
                for (slot, hit) in inside.iter_mut().zip(hits) {
                    *slot ^= hit;
                }
            }
        }
        ResolvedProfileSelection::Regions(regions) => {
            for region in regions {
                let cadmpeg_ir::features::SketchProfileRegion::Loops { loops } = region else {
                    return Ok(None);
                };
                let Some(outer) = contains(loops.outer())? else {
                    return Ok(None);
                };
                let mut hits = outer;
                for hole in loops.holes() {
                    let Some(hole) = contains(*hole)? else {
                        return Ok(None);
                    };
                    for (slot, hit) in hits.iter_mut().zip(hole) {
                        *slot &= !hit;
                    }
                }
                for (slot, hit) in inside.iter_mut().zip(hits) {
                    *slot |= hit;
                }
            }
        }
    }
    Ok(Some(
        inside
            .iter()
            .enumerate()
            .filter(|(_, hit)| **hit)
            .map(|(index, _)| index)
            .collect(),
    ))
}
