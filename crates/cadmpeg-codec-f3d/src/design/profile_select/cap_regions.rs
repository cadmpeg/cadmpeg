// SPDX-License-Identifier: Apache-2.0
//! Prove an Extrude's sketch regions from the faces its own transition
//! inserts or cuts.
//!
//! A planar face that an Extrude transition inserts parallel to its Sketch,
//! at one offset from the Sketch plane, repeats the swept region there. The
//! regions are the bounded faces of the arrangement of every
//! non-construction Sketch curve. A region whose interior lies inside the cap
//! is selected; the selection is admitted only when the outline of the
//! selected regions and the cap's boundary cover each other exactly: every
//! outline line or arc lies on a cap line or arc of the same carrier over
//! its whole interval, and the reverse.
//!
//! A cut through the part inserts no such cap. When the stored profiles name
//! curves that bound several regions, a region bounded only by those curves
//! is selected when a plane parallel to the Sketch that the transition
//! changed held the region's interior point before the transition and does
//! not after it. Such a selection reports `feature.profile-cut-regions-assumed`.
//!
//! Each cap edge is drawn from a proven carrier: a straight edge whose curve
//! axis runs along its chord, or a circle where the cap meets a cylinder whose
//! axis is parallel to the cap normal. A cap with any other edge proves
//! nothing.

use std::collections::HashSet;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use std::f64::consts::TAU;

use cadmpeg_ir::math::{Point2, Point3, Vector3};

use super::ResolvedProfileSelection;
use crate::design::geometry::{
    atomic_sketch_regions, boundary_use_carrier, point_in_polygon, point_segment_distance,
    profile_loop_contains_points, AtomicSketchRegion, BoundaryCarrier, ProfileGeometryWork,
};
use crate::history_records::{AsmDeltaState, AsmHistoricalTopology, AsmHistory};

/// Line segments one full circle or arc of a cap boundary is drawn with.
const ARC_SEGMENTS: u32 = 64;
/// Grid lines per axis searched for a point well inside a region.
const SAMPLE_GRID: u32 = 24;
/// Relative difference allowed between the cap area and the selected area.
/// Both sides are polylines of the same circles and arcs.
pub(super) const AREA_AGREEMENT: f64 = 1e-3;
/// How far from parallel, as a cosine, a cap's plane normal may be to the frame normal.
pub(super) const PARALLEL_CAP: f64 = 1e-9;
/// Largest sine of the angle between a straight edge's carrier axis and its chord.
const STRAIGHT_CARRIER: f64 = 1e-9;
/// How many linear tolerances either side of a boundary segment its two sides
/// are sampled at.
pub(super) const SIDE_OFFSET: f64 = 4.0;
/// Most loops and loop edges one cap may have.
pub(super) const CAP_EDGES: usize = 4096;

/// An orthonormal frame on one plane.
#[derive(Clone, Copy)]
pub(super) struct PlaneFrame {
    origin: Point3,
    u: Vector3,
    v: Vector3,
    normal: Vector3,
}

impl PlaneFrame {
    fn of_sketch(sketch: &cadmpeg_ir::sketches::Sketch) -> Option<Self> {
        let (origin, normal, u) = sketch.resolved_placement()?;
        let (normal, u) = (normal.get(), u.get());
        Some(Self {
            origin: origin.get(),
            u,
            v: normal.cross(u),
            normal,
        })
    }

    /// The frame through `origin` with unit normal along `normal` and its
    /// first axis across the world axis least aligned with it.
    pub(super) fn of_plane(origin: Point3, normal: Vector3) -> Option<Self> {
        let normal = normal.unit()?;
        let across = [
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
        ]
        .into_iter()
        .min_by(|a, b| a.dot(normal).abs().total_cmp(&b.dot(normal).abs()))?;
        let u = across.cross(normal).unit()?;
        Some(Self {
            origin,
            u,
            v: normal.cross(u),
            normal,
        })
    }

    pub(super) fn normal(&self) -> Vector3 {
        self.normal
    }

    pub(super) fn project(&self, point: Point3) -> Point2 {
        let offset = point.vector_from(self.origin);
        Point2::new(offset.dot(self.u), offset.dot(self.v))
    }

    pub(super) fn height(&self, point: Point3) -> f64 {
        point.vector_from(self.origin).dot(self.normal)
    }
}

/// One proven piece of a cap loop, in frame coordinates.
enum Piece {
    Line(Point2, Point2),
    /// From `start` radians through `sweep` radians; a negative sweep turns
    /// clockwise.
    Arc {
        center: Point2,
        radius: f64,
        start: f64,
        sweep: f64,
    },
}

impl Piece {
    fn distance(&self, point: Point2) -> f64 {
        match *self {
            Piece::Line(start, end) => point_segment_distance(point, (start, end)),
            Piece::Arc {
                center,
                radius,
                start,
                sweep,
            } => {
                let angle = (point.v - center.v).atan2(point.u - center.u);
                let along = if sweep >= 0.0 {
                    (angle - start).rem_euclid(TAU)
                } else {
                    (start - angle).rem_euclid(TAU)
                };
                if along <= sweep.abs() {
                    ((point.u - center.u).hypot(point.v - center.v) - radius).abs()
                } else {
                    let at = |angle: f64| {
                        Point2::new(
                            center.u + radius * angle.cos(),
                            center.v + radius * angle.sin(),
                        )
                    };
                    distance(point, at(start)).min(distance(point, at(start + sweep)))
                }
            }
        }
    }

    /// The piece's start, middle and end points.
    fn points(&self) -> [Point2; 3] {
        match *self {
            Piece::Line(start, end) => [
                start,
                Point2::new(f64::midpoint(start.u, end.u), f64::midpoint(start.v, end.v)),
                end,
            ],
            Piece::Arc {
                center,
                radius,
                start,
                sweep,
            } => [start, start + sweep / 2.0, start + sweep].map(|angle| {
                Point2::new(
                    center.u + radius * angle.cos(),
                    center.v + radius * angle.sin(),
                )
            }),
        }
    }

    /// Whether `other` traces the same points as this piece, in either
    /// direction.
    fn same(&self, other: &Piece, tolerance: f64) -> bool {
        let near = |a: Point2, b: Point2| distance(a, b) <= tolerance;
        let ([start, middle, end], [other_start, other_middle, other_end]) =
            (self.points(), other.points());
        matches!(
            (self, other),
            (Piece::Line(..), Piece::Line(..)) | (Piece::Arc { .. }, Piece::Arc { .. })
        ) && near(middle, other_middle)
            && (near(start, other_start) && near(end, other_end)
                || near(start, other_end) && near(end, other_start))
    }

    /// Whether pieces of `by` on this piece's own carrier cover its whole
    /// interval: collinear lines for a line, arcs of the same circle for an
    /// arc.
    fn covered_by(&self, by: &[&Piece], tolerance: f64) -> bool {
        match *self {
            Piece::Line(start, end) => {
                let length = distance(start, end);
                if length <= tolerance {
                    return false;
                }
                let (du, dv) = ((end.u - start.u) / length, (end.v - start.v) / length);
                let along = |point: Point2| (point.u - start.u) * du + (point.v - start.v) * dv;
                let across =
                    |point: Point2| ((point.u - start.u) * dv - (point.v - start.v) * du).abs();
                let intervals = by
                    .iter()
                    .filter_map(|other| match **other {
                        Piece::Line(a, b) if across(a) <= tolerance && across(b) <= tolerance => {
                            let (a, b) = (along(a), along(b));
                            Some((a.min(b), a.max(b)))
                        }
                        _ => None,
                    })
                    .collect();
                intervals_cover(intervals, 0.0, length, tolerance)
            }
            Piece::Arc {
                center,
                radius,
                start,
                sweep,
            } => {
                let counterclockwise = |start: f64, sweep: f64| {
                    if sweep >= 0.0 {
                        (start, sweep)
                    } else {
                        (start + sweep, -sweep)
                    }
                };
                let (first, length) = counterclockwise(start, sweep);
                let mut intervals = Vec::new();
                for other in by {
                    let Piece::Arc {
                        center: other_center,
                        radius: other_radius,
                        start: other_start,
                        sweep: other_sweep,
                    } = **other
                    else {
                        continue;
                    };
                    if distance(center, other_center) > tolerance
                        || (radius - other_radius).abs() > tolerance
                    {
                        continue;
                    }
                    let (other_first, other_length) = counterclockwise(other_start, other_sweep);
                    let offset = (other_first - first).rem_euclid(TAU);
                    intervals.push((offset, offset + other_length));
                    intervals.push((offset - TAU, offset - TAU + other_length));
                }
                intervals_cover(intervals, 0.0, length, tolerance / radius)
            }
        }
    }

    /// Appends the piece's polyline, without its end, to `ring`.
    fn push(&self, ring: &mut Vec<Point2>) {
        match *self {
            Piece::Line(start, _) => ring.push(start),
            Piece::Arc {
                center,
                radius,
                start,
                sweep,
            } => {
                for k in 0..ARC_SEGMENTS {
                    let angle = start + sweep * f64::from(k) / f64::from(ARC_SEGMENTS);
                    ring.push(Point2::new(
                        center.u + radius * angle.cos(),
                        center.v + radius * angle.sin(),
                    ));
                }
            }
        }
    }
}

pub(super) fn distance(a: Point2, b: Point2) -> f64 {
    (a.u - b.u).hypot(a.v - b.v)
}

/// One planar cap: its loops as closed frame polylines, the proven
/// pieces they are drawn from, and the loop vertices.
pub(super) struct Cap {
    pub(super) loops: Vec<Vec<Point2>>,
    pieces: Vec<Piece>,
    pub(super) corners: Vec<Point2>,
    pub(super) area: f64,
}

impl Cap {
    pub(super) fn contains(&self, point: Point2) -> bool {
        self.loops
            .iter()
            .filter(|ring| point_in_polygon(point, ring))
            .count()
            % 2
            == 1
    }

    pub(super) fn boundary_distance(&self, point: Point2) -> f64 {
        self.pieces
            .iter()
            .map(|piece| piece.distance(point))
            .fold(f64::INFINITY, f64::min)
    }
}

/// One arrangement region with a point well inside it and the regions
/// nested directly in it.
struct Sampled<'a> {
    region: &'a AtomicSketchRegion,
    point: Point2,
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
    let Some(frame) = PlaneFrame::of_sketch(sketch) else {
        return Ok(None);
    };
    let Some(caps) = far_caps(
        &frame,
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
        let mut inside = Vec::new();
        for (index, region) in sampled.iter().enumerate() {
            if cap.contains(region.point) {
                inside.push(index);
                if !selected.contains(&index) {
                    selected.push(index);
                }
            }
        }
        if !outlines_agree(cap, &sampled, &inside, entities, tolerance) {
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
    Ok(trimmed_selection(&sampled, &selected))
}

/// The material a transition leaves on one plane parallel to the sketch:
/// the faces on it before the transition that it deleted or changed, and
/// the faces on it after the transition that it inserted or changed.
struct PlaneSides {
    offset: f64,
    before: Vec<Cap>,
    after: Vec<Cap>,
}

/// The planes parallel to the sketch that the transition from `before` to
/// `after` changed. A plane on which any changed face has no proven cap is
/// left out. `None` when a changed face has no surface binding or several,
/// or its surface several plane rows.
fn changed_planes(
    sketch: &cadmpeg_ir::sketches::Sketch,
    (before, after): (&AsmHistoricalTopology, &AsmHistoricalTopology),
    faces: &crate::history_records::AsmHistoricalEntityDelta,
    tolerance: f64,
) -> Option<Vec<PlaneSides>> {
    let frame = PlaneFrame::of_sketch(sketch)?;
    let mut planes: Vec<PlaneSides> = Vec::new();
    let mut incomplete: Vec<f64> = Vec::new();
    let sides = [
        (before, faces.deleted.iter().chain(&faces.updated), true),
        (after, faces.inserted.iter().chain(&faces.updated), false),
    ];
    for (topology, changed, is_before) in sides {
        for face in changed {
            let point = match parallel_plane(topology, *face, frame.normal) {
                ParallelPlane::At(point) => point,
                ParallelPlane::Elsewhere => continue,
                ParallelPlane::Unreadable => return None,
            };
            let offset = frame.height(point);
            let Some(cap) = face_cap(&frame, topology, *face, tolerance) else {
                incomplete.push(offset);
                continue;
            };
            let index = match planes
                .iter()
                .position(|plane| (plane.offset - offset).abs() <= tolerance)
            {
                Some(index) => index,
                None => {
                    planes.push(PlaneSides {
                        offset,
                        before: Vec::new(),
                        after: Vec::new(),
                    });
                    planes.len() - 1
                }
            };
            if is_before {
                planes[index].before.push(cap);
            } else {
                planes[index].after.push(cap);
            }
        }
    }
    planes.retain(|plane| {
        !incomplete
            .iter()
            .any(|offset| (plane.offset - offset).abs() <= tolerance)
    });
    Some(planes)
}

/// The regions bounded only by the curves of the stored profiles `current`
/// names that the transition from `previous_state_id` to `state_id` cut out
/// of the planes parallel to the sketch it changed.
///
/// A region is cut when a changed plane held its interior before the
/// transition and does not after it, and kept when a changed plane holds it
/// on both sides. `None` when a candidate region has no such evidence or
/// conflicting evidence, when the cut regions do not use every profile
/// curve, or when they are the regions `current` already covers.
pub(super) fn removed_profile_selection(
    sketch: &cadmpeg_ir::sketches::Sketch,
    resolution: super::ScopedExtrudeProfileResolution<'_>,
    state_id: i64,
    previous_state_id: i64,
    current: Option<&ResolvedProfileSelection>,
) -> Result<Option<ResolvedProfileSelection>, CodecError> {
    let Some(current @ ResolvedProfileSelection::Loops(indices)) = current else {
        return Ok(None);
    };
    let mut curves = HashSet::new();
    for index in indices {
        let Some(profile) = usize::try_from(*index)
            .ok()
            .and_then(|index| sketch.profiles.as_slice().get(index))
        else {
            return Ok(None);
        };
        curves.extend(profile.iter().map(|use_| &use_.entity));
    }
    let entities = resolution.entities;
    let tolerance = resolution.linear_tolerance;
    let work = resolution.profile_work;
    let ctx = resolution.ctx;
    let Some((after, state)) = unique_state(resolution.histories, state_id) else {
        return Ok(None);
    };
    let Some(transition) = state.transition.as_ref() else {
        return Ok(None);
    };
    if transition.previous_state_id != Some(previous_state_id) {
        return Ok(None);
    }
    let Some((before, _)) = unique_state(resolution.histories, previous_state_id) else {
        return Ok(None);
    };
    let Some(planes) = changed_planes(
        sketch,
        (before, after),
        &transition.topology.faces,
        tolerance,
    ) else {
        return Ok(None);
    };
    if planes.is_empty() {
        return Ok(None);
    }
    let Some(regions) = atomic_sketch_regions(sketch, entities, tolerance, work, ctx)? else {
        return Ok(None);
    };
    let Some(sampled) = sample_regions(&regions, tolerance) else {
        return Ok(None);
    };
    let mut selected = Vec::new();
    let mut used = HashSet::new();
    for (index, region) in sampled.iter().enumerate() {
        if !region
            .region
            .boundary
            .iter()
            .all(|use_| curves.contains(&use_.entity))
        {
            continue;
        }
        let mut removed = false;
        let mut kept = false;
        for plane in &planes {
            let held = plane.before.iter().any(|cap| cap.contains(region.point));
            let holds = plane.after.iter().any(|cap| cap.contains(region.point));
            match (held, holds) {
                (true, true) => kept = true,
                (true, false) => removed = true,
                (false, true) => return Ok(None),
                (false, false) => {}
            }
        }
        if removed == kept {
            return Ok(None);
        }
        if removed {
            selected.push(index);
            used.extend(region.region.boundary.iter().map(|use_| &use_.entity));
        }
    }
    if selected.is_empty() || used != curves {
        return Ok(None);
    }
    let points: Vec<Point2> = sampled.iter().map(|region| region.point).collect();
    if covered_regions(sketch, entities, current, &points, tolerance, work, ctx)?
        .is_some_and(|covered| covered == selected)
    {
        return Ok(None);
    }
    let selection = trimmed_selection(&sampled, &selected);
    if selection.is_some() {
        work.note_cut_regions_assumed();
    }
    Ok(selection)
}

/// The `selected` regions as trimmed regions, each with the nested regions
/// left out of the selection as holes.
fn trimmed_selection(
    sampled: &[Sampled<'_>],
    selected: &[usize],
) -> Option<ResolvedProfileSelection> {
    let mut out = Vec::new();
    for &index in selected {
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
            return None;
        };
        out.push(region);
    }
    Some(ResolvedProfileSelection::Regions(out))
}

/// Whether the outline of the union of the `chosen` regions and the cap's
/// boundary cover each other exactly.
///
/// Each boundary use of a chosen region counts once, and once more as a
/// hole of that region for each region nested directly in it. A use counted
/// an even number of times separates two chosen regions; the rest form the
/// outline. Every outline line or arc must lie on cap pieces of the same
/// carrier over its whole interval, and every cap piece on outline pieces.
/// A boundary on any other curve proves nothing.
fn outlines_agree(
    cap: &Cap,
    sampled: &[Sampled<'_>],
    chosen: &[usize],
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    tolerance: f64,
) -> bool {
    let rings = chosen
        .iter()
        .flat_map(|index| std::iter::once(*index).chain(sampled[*index].nested.iter().copied()));
    let mut counted: Vec<(&cadmpeg_ir::sketches::SketchEntityId, Piece, usize)> = Vec::new();
    for ring in rings {
        for use_ in &sampled[ring].region.boundary {
            let Some(carrier) = boundary_use_carrier(use_, entities) else {
                return false;
            };
            let piece = match carrier {
                BoundaryCarrier::Line { start, end } => Piece::Line(start, end),
                BoundaryCarrier::Arc {
                    center,
                    radius,
                    start,
                    sweep,
                } => Piece::Arc {
                    center,
                    radius,
                    start,
                    sweep,
                },
            };
            match counted
                .iter_mut()
                .find(|(entity, other, _)| *entity == &use_.entity && piece.same(other, tolerance))
            {
                Some((_, _, count)) => *count += 1,
                None => counted.push((&use_.entity, piece, 1)),
            }
        }
    }
    let outline: Vec<&Piece> = counted
        .iter()
        .filter(|(_, _, count)| count % 2 == 1)
        .map(|(_, piece, _)| piece)
        .collect();
    let cap_pieces: Vec<&Piece> = cap.pieces.iter().collect();
    !outline.is_empty()
        && outline
            .iter()
            .all(|piece| piece.covered_by(&cap_pieces, tolerance))
        && cap
            .pieces
            .iter()
            .all(|piece| piece.covered_by(&outline, tolerance))
}

/// Whether ordered intervals reach from `from` to `to` without a gap wider
/// than `slack`.
fn intervals_cover(mut intervals: Vec<(f64, f64)>, from: f64, to: f64, slack: f64) -> bool {
    intervals.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut reach = from;
    for (low, high) in intervals {
        if reach >= to - slack {
            break;
        }
        if low > reach + slack {
            return false;
        }
        reach = reach.max(high);
    }
    reach >= to - slack
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

/// The inserted planar faces parallel to the frame at the offset farthest
/// from its plane, each with every loop resolved. `None` when an inserted
/// face has no surface binding or several, or its surface several plane rows.
fn far_caps(
    frame: &PlaneFrame,
    topology: &AsmHistoricalTopology,
    inserted: &[i64],
    tolerance: f64,
) -> Option<Vec<Cap>> {
    let mut caps: Vec<(f64, Cap)> = Vec::new();
    for face in inserted {
        let origin = match parallel_plane(topology, *face, frame.normal) {
            ParallelPlane::At(origin) => origin,
            ParallelPlane::Elsewhere => continue,
            ParallelPlane::Unreadable => return None,
        };
        let height = frame.height(origin);
        let cap = face_cap(frame, topology, *face, tolerance)?;
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

/// What the surface binding of a face states about a plane parallel to a
/// normal.
enum ParallelPlane {
    /// A point of the face's plane, which is parallel to the normal's plane.
    At(Point3),
    /// The face's surface has no plane row, or its plane is not parallel.
    Elsewhere,
    /// The face has no surface binding or several, or its surface several
    /// plane rows.
    Unreadable,
}

fn parallel_plane(
    topology: &AsmHistoricalTopology,
    face: i64,
    normal: cadmpeg_ir::math::Vector3,
) -> ParallelPlane {
    let Ok(Some(binding)) = sole(
        topology
            .face_surfaces
            .iter()
            .filter(|binding| binding.entity == face),
    ) else {
        return ParallelPlane::Unreadable;
    };
    match sole(
        topology
            .surface_planes
            .iter()
            .filter(|plane| plane.surface == binding.carrier),
    ) {
        Err(Several) => ParallelPlane::Unreadable,
        Ok(Some(plane)) if (plane.normal.dot(normal).abs() - 1.0).abs() <= PARALLEL_CAP => {
            ParallelPlane::At(plane.origin)
        }
        Ok(_) => ParallelPlane::Elsewhere,
    }
}

/// Every loop of `face` as a frame polyline drawn from its proven pieces.
/// `None` when a loop does not close or an edge has no proven carrier.
pub(super) fn face_cap(
    frame: &PlaneFrame,
    topology: &AsmHistoricalTopology,
    face: i64,
    tolerance: f64,
) -> Option<Cap> {
    let mut loops = Vec::new();
    let mut pieces = Vec::new();
    let mut corners = Vec::new();
    for steps in oriented_loops(topology, face)? {
        let mut ring: Vec<Point2> = Vec::new();
        for (edge, from, to) in steps {
            let piece = edge_piece(frame, topology, face, edge, (from, to), tolerance)?;
            corners.push(frame.project(from));
            piece.push(&mut ring);
            pieces.push(piece);
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
    (area > tolerance * tolerance).then_some(Cap {
        loops,
        pieces,
        corners,
        area,
    })
}

/// Each loop of `face` as its edges in loop order, each with the positions
/// it runs from and to. `None` when a loop does not close or a row is
/// missing.
pub(super) fn oriented_loops(
    topology: &AsmHistoricalTopology,
    face: i64,
) -> Option<Vec<Vec<(i64, Point3, Point3)>>> {
    let position = |vertex: i64| -> Option<Point3> {
        let point = sole(
            topology
                .vertex_points
                .iter()
                .filter(|binding| binding.entity == vertex),
        )
        .ok()??
        .carrier;
        Some(
            sole(
                topology
                    .point_positions
                    .iter()
                    .filter(|row| row.point == point),
            )
            .ok()??
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
        let mut ends = Vec::new();
        for coedge in coedges {
            let edge = sole(
                topology
                    .coedge_topology
                    .iter()
                    .filter(|row| row.coedge == *coedge),
            )
            .ok()??
            .edge;
            ends.push(sole(topology.edge_vertices.iter().filter(|row| row.edge == edge)).ok()??);
        }
        // Orient every edge, the first one by the edge after it, before
        // drawing any of them.
        let mut steps: Vec<(i64, i64, i64)> = Vec::new();
        for (index, edge) in ends.iter().enumerate() {
            let (from, to) = (edge.start_vertex, edge.end_vertex);
            let step = match steps.last() {
                Some(&(_, _, previous)) if previous == from => (from, to),
                Some(&(_, _, previous)) if previous == to => (to, from),
                Some(_) => return None,
                None if ends.len() == 1 => (from, to),
                None => {
                    let next = ends.get(index + 1)?;
                    let next = [next.start_vertex, next.end_vertex];
                    if next.contains(&to) {
                        (from, to)
                    } else if next.contains(&from) {
                        (to, from)
                    } else {
                        return None;
                    }
                }
            };
            steps.push((edge.edge, step.0, step.1));
        }
        if steps.first()?.1 != steps.last()?.2 {
            return None;
        }
        loops.push(
            steps
                .into_iter()
                .map(|(edge, from, to)| Some((edge, position(from)?, position(to)?)))
                .collect::<Option<Vec<_>>>()?,
        );
    }
    Some(loops)
}

/// The proven piece of the cap `face`'s `edge` between its two ends: a line
/// when the edge's curve axis runs along its chord through both ends, or a
/// circle where the cap meets a cylinder about an axis parallel to the cap
/// normal. `None` for any other carrier.
fn edge_piece(
    frame: &PlaneFrame,
    topology: &AsmHistoricalTopology,
    face: i64,
    edge: i64,
    (from, to): (Point3, Point3),
    tolerance: f64,
) -> Option<Piece> {
    let normal = frame.normal;
    let start = frame.project(from);
    let end = frame.project(to);
    let interior = sole(topology.edge_samples.iter().filter(|row| row.edge == edge))
        .ok()?
        .map(|row| row.interior);
    let closed = distance(start, end) <= tolerance;
    let curve = sole(
        topology
            .edge_curves
            .iter()
            .filter(|binding| binding.entity == edge),
    )
    .ok()?
    .and_then(|binding| binding.carrier);
    let axis = match curve {
        Some(curve) => sole(topology.curve_axes.iter().filter(|row| row.curve == curve)).ok()?,
        None => None,
    };
    if !closed {
        if let Some(axis) = axis {
            let chord = to.vector_from(from);
            let direction = axis.direction;
            let off_line = |point: Point3| {
                point.vector_from(axis.origin).cross(direction).norm() / direction.norm()
            };
            if direction.cross(chord).norm() <= STRAIGHT_CARRIER * direction.norm() * chord.norm()
                && off_line(from) <= tolerance
                && off_line(to) <= tolerance
            {
                return match interior.map(|point| frame.project(point)) {
                    Some(point) if point_segment_distance(point, (start, end)) > tolerance => None,
                    _ => Some(Piece::Line(start, end)),
                };
            }
        }
    }
    let mut surfaces = Vec::new();
    for row in topology
        .coedge_topology
        .iter()
        .filter(|row| row.edge == edge)
    {
        let owner = sole(
            topology
                .face_loops
                .iter()
                .filter(|relation| relation.member_refs.contains(&row.owner_loop)),
        )
        .ok()??
        .owner_ref;
        if owner == face {
            continue;
        }
        let binding = sole(
            topology
                .face_surfaces
                .iter()
                .filter(|binding| binding.entity == owner),
        )
        .ok()??;
        surfaces.push(binding.carrier);
    }
    let mut cylinders = topology.surface_cylinders.iter().filter(|cylinder| {
        surfaces.contains(&cylinder.surface)
            && (cylinder.axis.dot(normal).abs() / cylinder.axis.norm() - 1.0).abs() <= PARALLEL_CAP
    });
    let cylinder = cylinders.next()?;
    let center = frame.project(cylinder.origin);
    let radius = cylinder.radius;
    if cylinders.any(|other| {
        distance(frame.project(other.origin), center) > tolerance
            || (other.radius - radius).abs() > tolerance
    }) {
        return None;
    }
    let on_circle = |point: Point2| (distance(point, center) - radius).abs() <= tolerance;
    if radius <= tolerance || !on_circle(start) || !on_circle(end) {
        return None;
    }
    let angle = |point: Point2| (point.v - center.v).atan2(point.u - center.u);
    let first = angle(start);
    if closed {
        return Some(Piece::Arc {
            center,
            radius,
            start: first,
            sweep: TAU,
        });
    }
    let middle = frame.project(interior?);
    if !on_circle(middle) {
        return None;
    }
    let counterclockwise = (angle(end) - first).rem_euclid(TAU);
    let sweep = if (angle(middle) - first).rem_euclid(TAU) < counterclockwise {
        counterclockwise
    } else {
        counterclockwise - TAU
    };
    Some(Piece::Arc {
        center,
        radius,
        start: first,
        sweep,
    })
}

/// Several rows where at most one may occur.
struct Several;

/// The item of `items` when it holds at most one.
fn sole<T>(mut items: impl Iterator<Item = T>) -> Result<Option<T>, Several> {
    let first = items.next();
    if first.is_some() && items.next().is_some() {
        Err(Several)
    } else {
        Ok(first)
    }
}

pub(super) fn relation(
    rows: &[crate::history_records::AsmHistoricalRelation],
    owner: i64,
) -> Option<&[i64]> {
    let mut rows = rows.iter().filter(|row| row.owner_ref == owner);
    let row = rows.next()?;
    rows.next().is_none().then_some(row.member_refs.as_slice())
}

fn polygon_area(ring: &[Point2]) -> f64 {
    cadmpeg_ir::math::planar::polygon_area_twice(ring)
        .map_or(f64::NAN, cadmpeg_ir::scalar::FiniteReal::get)
        * 0.5
}

/// Each region with a point well inside it, outside the regions nested in it.
/// `None` when a region has no such point.
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

#[cfg(test)]
mod tests;
