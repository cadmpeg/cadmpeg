// SPDX-License-Identifier: Apache-2.0
//! Certify compact loop selections against analytic extrusion transitions.

use super::super::ResolvedProfileSelection;
use super::{
    changed_planes, changed_planes_with_gaps, covered_regions, distance, face_cap,
    face_cap_indexed, intervals_cover, oriented_loops, outlines_agree, parallel_plane,
    parallel_plane_indexed, sample_regions, unique_state, Agreement, Cap, HistoricalTopologyIndex,
    ParallelPlane, Piece, PlaneFrame, PlaneSides, PARALLEL_CAP, SIDE_OFFSET, STRAIGHT_CARRIER,
};
use crate::design::geometry::{atomic_sketch_regions, ProfileGeometryWork};
use crate::history_records::AsmHistoricalTopology;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::extents::{
    ExtrudeDirection, ExtrudeExtent, ExtrudeSide, ExtrudeStart, LinearTermination,
};
use cadmpeg_ir::features::operation_settings::BooleanOp;
use cadmpeg_ir::features::profiles::{PlanarProfileRef, ProfileRef};
use cadmpeg_ir::math::{Point3, Vector3};
use std::collections::{HashMap, HashSet};

mod two_sided;

const PERPENDICULAR_SIDE: f64 = 1.0e-9;
const TRANSVERSE_CYLINDER: f64 = 1.0e-9;

#[derive(Clone, Copy)]
pub(in crate::design::profile_select) struct Sweep<'a> {
    pub(in crate::design::profile_select) operation: &'a BooleanOp,
    pub(in crate::design::profile_select) start: &'a ExtrudeStart,
    pub(in crate::design::profile_select) direction: &'a ExtrudeDirection,
    pub(in crate::design::profile_select) extent: &'a ExtrudeExtent,
}

pub(in crate::design::profile_select) fn selection_agrees(
    sketch: &cadmpeg_ir::sketches::Sketch,
    resolution: super::super::ScopedExtrudeProfileResolution<'_>,
    candidate: &ProfileRef,
    state_ids: (i64, i64),
    sweep: Sweep<'_>,
) -> Result<bool, CodecError> {
    let current = match candidate {
        ProfileRef::Planar(PlanarProfileRef::SketchProfiles { profiles, .. }) => {
            ResolvedProfileSelection::Loops(profiles.to_vec())
        }
        ProfileRef::Planar(PlanarProfileRef::SketchRegions { regions, .. }) => {
            ResolvedProfileSelection::Regions(regions.to_vec())
        }
        _ => return Ok(false),
    };
    let Some((after, state)) = unique_state(resolution.histories, state_ids.0) else {
        return Ok(false);
    };
    let Some(transition) = state.transition.as_ref() else {
        return Ok(false);
    };
    if transition.previous_state_id != Some(state_ids.1) {
        return Ok(false);
    }
    let Some((before, _)) = unique_state(resolution.histories, state_ids.1) else {
        return Ok(false);
    };
    let Some(far) = signed_length(&sweep) else {
        return Ok(false);
    };
    let Some(frame) = PlaneFrame::of_sketch(sketch) else {
        return Ok(false);
    };
    let tolerance = resolution.linear_tolerance;
    for (topology, faces) in [
        (
            before,
            transition
                .topology
                .faces
                .deleted
                .iter()
                .chain(&transition.topology.faces.updated),
        ),
        (
            after,
            transition
                .topology
                .faces
                .inserted
                .iter()
                .chain(&transition.topology.faces.updated),
        ),
    ] {
        for face in faces {
            match parallel_plane(topology, *face, frame.normal) {
                ParallelPlane::At(_) if face_cap(&frame, topology, *face, tolerance).is_none() => {
                    return Ok(false)
                }
                ParallelPlane::Unreadable => return Ok(false),
                _ => {}
            }
        }
    }
    let work = ProfileGeometryWork::new();
    let Some(regions) = atomic_sketch_regions(
        sketch,
        resolution.entities,
        tolerance,
        &work,
        resolution.ctx,
    )?
    else {
        return Ok(false);
    };
    let Some(sampled) = sample_regions(&regions, tolerance, resolution.profile_work) else {
        return Ok(false);
    };
    let points: Vec<_> = sampled.iter().map(|region| region.point).collect();
    let selected = match &current {
        ResolvedProfileSelection::Regions(regions)
            if regions.iter().any(|region| {
                matches!(
                    region,
                    cadmpeg_ir::features::profiles::SketchProfileRegion::Trimmed { .. }
                )
            }) =>
        {
            let Some(selected) = trimmed_indices(regions, &sampled, tolerance) else {
                return Ok(false);
            };
            selected
        }
        _ => {
            let Some(selected) = covered_regions(
                sketch,
                resolution.entities,
                &current,
                &points,
                tolerance,
                &work,
                resolution.ctx,
            )?
            else {
                return Ok(false);
            };
            selected
        }
    };
    if selected.is_empty() {
        return Ok(false);
    }
    let Some(planes) = changed_planes(
        sketch,
        (before, after),
        &transition.topology.faces,
        tolerance,
    ) else {
        return Ok(false);
    };
    let mut witnessed = HashSet::new();
    let mut outline = Vec::new();
    for plane in &planes {
        let changed = |point| {
            plane.before.iter().any(|cap| cap.contains(point))
                != plane.after.iter().any(|cap| cap.contains(point))
        };
        let covered: Vec<_> = sampled
            .iter()
            .enumerate()
            .filter(|(_, region)| changed(region.point))
            .map(|(index, _)| index)
            .collect();
        if covered.iter().any(|index| !selected.contains(index)) {
            return Ok(false);
        }
        let Some(pieces) = changed_outline(plane, &sampled, &covered, resolution, tolerance)?
        else {
            return Ok(false);
        };
        if pieces.is_empty() && covered.is_empty() {
            continue;
        }
        for index in covered {
            let point = sampled[index].point;
            let at_floor = (plane.offset - far).abs() <= tolerance;
            if plane.before.iter().any(|cap| cap.contains(point))
                || (at_floor && plane.before.is_empty())
            {
                witnessed.insert(index);
            } else if !at_floor {
                return Ok(false);
            }
        }
        outline.extend(pieces);
    }
    if witnessed.len() != selected.len() {
        return Ok(false);
    }
    let deleted = deleted_sides(before, &transition.topology.faces.deleted, &frame);
    Ok(inserted_sides_agree(
        after,
        &transition.topology.faces.inserted,
        &deleted,
        &frame,
        &outline,
        (far.min(0.0), far.max(0.0)),
        tolerance,
    ))
}

/// The pieces of the outline between the part of `plane` that the transition
/// changed and the part it left, when they cover the boundaries of the
/// `covered` regions exactly. `None` when they do not.
fn changed_outline(
    plane: &PlaneSides,
    sampled: &[super::Sampled<'_>],
    covered: &[usize],
    resolution: super::super::ScopedExtrudeProfileResolution<'_>,
    tolerance: f64,
) -> Result<Option<Vec<Piece>>, CodecError> {
    let changed = |point| {
        plane.before.iter().any(|cap| cap.contains(point))
            != plane.after.iter().any(|cap| cap.contains(point))
    };
    let mut cap = Cap::new(Vec::new(), Vec::new(), Vec::new(), 0.0, Vec::new());
    let endpoints: Vec<_> = plane
        .before
        .iter()
        .chain(&plane.after)
        .flat_map(|source| &source.pieces)
        .flat_map(|piece| {
            let [start, _, end] = piece.points();
            [start, end]
        })
        .collect();
    for source in plane.before.iter().chain(&plane.after) {
        for piece in &source.pieces {
            for fragment in split_piece(piece, &endpoints, tolerance) {
                let Some((left, right)) = fragment.sides_of_middle(SIDE_OFFSET * tolerance) else {
                    return Ok(None);
                };
                if changed(left) != changed(right) {
                    cap.pieces.push(fragment);
                }
            }
        }
    }
    if cap.pieces.is_empty() && covered.is_empty() {
        return Ok(Some(Vec::new()));
    }
    if !matches!(
        outlines_agree(
            &cap,
            sampled,
            covered,
            resolution.entities,
            tolerance,
            resolution.ctx
        )?,
        Agreement::Exact
    ) {
        return Ok(None);
    }
    Ok(Some(cap.pieces))
}

/// A lateral face that a transition deletes: its carrier and the heights of
/// its vertices above the sketch plane.
struct DeletedSide {
    carrier: SideCarrier,
    low: f64,
    high: f64,
}

enum SideCarrier {
    Plane {
        origin: Point3,
        normal: Vector3,
    },
    Cylinder {
        origin: Point3,
        axis: Vector3,
        radius: f64,
    },
}

impl SideCarrier {
    fn of(topology: &AsmHistoricalTopology, face: i64) -> Option<Self> {
        let surface = topology
            .face_surfaces
            .iter()
            .find(|row| row.entity == face)?
            .carrier;
        if let Some(plane) = topology
            .surface_planes
            .iter()
            .find(|row| row.surface == surface)
        {
            return Some(Self::Plane {
                origin: plane.origin,
                normal: unit(plane.normal)?,
            });
        }
        let cylinder = topology
            .surface_cylinders
            .iter()
            .find(|row| row.surface == surface)?;
        Some(Self::Cylinder {
            origin: cylinder.origin,
            axis: unit(cylinder.axis)?,
            radius: cylinder.radius,
        })
    }

    fn coincides(&self, other: &Self, tolerance: f64) -> bool {
        let parallel = |a: Vector3, b: Vector3| (a.dot(b).abs() - 1.0).abs() <= PARALLEL_CAP;
        match (self, other) {
            (
                Self::Plane { origin, normal },
                Self::Plane {
                    origin: other_origin,
                    normal: other_normal,
                },
            ) => {
                parallel(*normal, *other_normal)
                    && other_origin.vector_from(*origin).dot(*normal).abs() <= tolerance
            }
            (
                Self::Cylinder {
                    origin,
                    axis,
                    radius,
                },
                Self::Cylinder {
                    origin: other_origin,
                    axis: other_axis,
                    radius: other_radius,
                },
            ) => {
                parallel(*axis, *other_axis)
                    && (radius - other_radius).abs() <= tolerance
                    && other_origin.vector_from(*origin).cross(*axis).norm() <= tolerance
            }
            _ => false,
        }
    }
}

fn unit(vector: Vector3) -> Option<Vector3> {
    let norm = vector.norm();
    (norm > 0.0).then(|| vector.scale(1.0 / norm))
}

/// The lateral faces among `faces` in `before`: faces on a plane or
/// cylinder carrier that is not parallel to the sketch.
fn deleted_sides(
    before: &AsmHistoricalTopology,
    faces: &[i64],
    frame: &PlaneFrame,
) -> Vec<DeletedSide> {
    let mut sides = Vec::new();
    for face in faces {
        if !matches!(
            parallel_plane(before, *face, frame.normal),
            ParallelPlane::Elsewhere
        ) {
            continue;
        }
        let (Some(carrier), Some(loops)) = (
            SideCarrier::of(before, *face),
            oriented_loops(before, *face),
        ) else {
            continue;
        };
        let heights = loops
            .iter()
            .flatten()
            .flat_map(|(_, start, end)| [frame.height(*start), frame.height(*end)]);
        let (low, high) = heights.fold((f64::INFINITY, f64::NEG_INFINITY), |(low, high), h| {
            (low.min(h), high.max(h))
        });
        if low <= high {
            sides.push(DeletedSide { carrier, low, high });
        }
    }
    sides
}

/// Whether every inserted face lies on the sweep: a parallel face within the
/// travel, and a lateral face on the outline. The height range of a lateral
/// face is covered by the travel and by the height ranges of deleted lateral
/// faces on the same carrier, which the inserted face continues.
fn inserted_sides_agree(
    after: &AsmHistoricalTopology,
    faces: &[i64],
    deleted: &[DeletedSide],
    frame: &PlaneFrame,
    outline: &[Piece],
    travel: (f64, f64),
    tolerance: f64,
) -> bool {
    let mut side_count = 0;
    for face in faces {
        let Some(binding) = after.face_surfaces.iter().find(|row| row.entity == *face) else {
            return false;
        };
        if let ParallelPlane::At(point) = parallel_plane(after, *face, frame.normal) {
            let height = frame.height(point);
            if height < travel.0 - tolerance || height > travel.1 + tolerance {
                return false;
            }
            continue;
        }
        if !lateral_carrier_agrees(after, binding.carrier, frame, outline, tolerance) {
            return false;
        }
        let Some(loops) = oriented_loops(after, *face) else {
            return false;
        };
        let mut low = f64::INFINITY;
        let mut high = f64::NEG_INFINITY;
        for (edge, start, end) in loops.into_iter().flatten() {
            if !edge_height_is_bounded(after, frame, edge, start, end, tolerance)
                && !bounded_lateral_ellipse(
                    after,
                    binding.carrier,
                    edge,
                    frame,
                    outline,
                    travel,
                    tolerance,
                )
                && !bounded_lateral_cylinder_cap(
                    after,
                    binding.carrier,
                    edge,
                    frame,
                    start,
                    travel,
                    tolerance,
                )
            {
                return false;
            }
            for point in [start, end] {
                if !outline
                    .iter()
                    .any(|piece| piece.distance(frame.project(point)) <= tolerance)
                {
                    return false;
                }
                let height = frame.height(point);
                low = low.min(height);
                high = high.max(height);
            }
        }
        let mut spans = vec![travel];
        if let Some(carrier) = SideCarrier::of(after, *face) {
            spans.extend(
                deleted
                    .iter()
                    .filter(|side| side.carrier.coincides(&carrier, tolerance))
                    .map(|side| (side.low, side.high)),
            );
        }
        if low > high || !intervals_cover(spans, low, high, tolerance) {
            return false;
        }
        side_count += 1;
    }
    side_count > 0
}

pub(super) fn split_piece(
    piece: &Piece,
    points: &[cadmpeg_ir::math::Point2],
    tolerance: f64,
) -> Vec<Piece> {
    let length = match *piece {
        Piece::Line(start, end) => distance(start, end),
        Piece::Arc { radius, sweep, .. } => radius * sweep.abs(),
    };
    if length <= tolerance {
        return vec![piece.clone()];
    }
    let mut parameters = vec![0.0, 1.0];
    for point in points {
        if piece.distance(*point) > tolerance {
            continue;
        }
        let parameter = match *piece {
            Piece::Line(start, end) => {
                ((point.u - start.u) * (end.u - start.u) + (point.v - start.v) * (end.v - start.v))
                    / (length * length)
            }
            Piece::Arc {
                center,
                start,
                sweep,
                ..
            } => {
                let angle = (point.v - center.v).atan2(point.u - center.u);
                ((angle - start) * sweep.signum()).rem_euclid(std::f64::consts::TAU) / sweep.abs()
            }
        };
        if parameter > tolerance / length && parameter < 1.0 - tolerance / length {
            parameters.push(parameter);
        }
    }
    parameters.sort_by(f64::total_cmp);
    parameters.dedup_by(|a, b| (*a - *b).abs() * length <= tolerance);
    parameters
        .windows(2)
        .map(|pair| match *piece {
            Piece::Line(start, end) => {
                let at = |t: f64| {
                    cadmpeg_ir::math::Point2::new(
                        start.u + (end.u - start.u) * t,
                        start.v + (end.v - start.v) * t,
                    )
                };
                Piece::Line(at(pair[0]), at(pair[1]))
            }
            Piece::Arc {
                center,
                radius,
                start,
                sweep,
            } => Piece::Arc {
                center,
                radius,
                start: start + sweep * pair[0],
                sweep: sweep * (pair[1] - pair[0]),
            },
        })
        .collect()
}

fn trimmed_indices(
    regions: &[cadmpeg_ir::features::profiles::SketchProfileRegion],
    sampled: &[super::Sampled<'_>],
    tolerance: f64,
) -> Option<Vec<usize>> {
    let mut selected = Vec::new();
    for region in regions {
        let cadmpeg_ir::features::profiles::SketchProfileRegion::Trimmed { outer_boundary, .. } =
            region
        else {
            return None;
        };
        let mut matching = sampled.iter().enumerate().filter(|(_, candidate)| {
            candidate.region.boundary.as_slice() == outer_boundary.as_slice()
        });
        let (index, _) = matching.next()?;
        if matching.next().is_some() || selected.contains(&index) {
            return None;
        }
        selected.push(index);
    }
    let Some(ResolvedProfileSelection::Regions(expected)) =
        super::trimmed_selection(sampled, &selected, tolerance)
    else {
        return None;
    };
    (expected.as_slice() == regions).then_some(selected)
}

pub(in crate::design::profile_select) fn removed_candidate(
    sketch: &cadmpeg_ir::sketches::Sketch,
    resolution: super::super::ScopedExtrudeProfileResolution<'_>,
    state_ids: (i64, i64),
    sweep: Sweep<'_>,
) -> Result<Option<PlanarProfileRef>, CodecError> {
    let Some((after, state)) = unique_state(resolution.histories, state_ids.0) else {
        return Ok(None);
    };
    let Some(transition) = state.transition.as_ref() else {
        return Ok(None);
    };
    if transition.previous_state_id != Some(state_ids.1) {
        return Ok(None);
    }
    let Some((before, _)) = unique_state(resolution.histories, state_ids.1) else {
        return Ok(None);
    };
    let tolerance = resolution.linear_tolerance;
    let Some(planes) = changed_planes(
        sketch,
        (before, after),
        &transition.topology.faces,
        tolerance,
    ) else {
        return Ok(None);
    };
    let work = resolution.profile_work;
    let Some(regions) =
        atomic_sketch_regions(sketch, resolution.entities, tolerance, work, resolution.ctx)?
    else {
        return Ok(None);
    };
    let Some(sampled) = sample_regions(&regions, tolerance, work) else {
        return Ok(None);
    };
    let selected: Vec<_> = sampled
        .iter()
        .enumerate()
        .filter(|(_, region)| {
            planes.iter().any(|plane| {
                plane.before.iter().any(|cap| cap.contains(region.point))
                    && !plane.after.iter().any(|cap| cap.contains(region.point))
            })
        })
        .map(|(index, _)| index)
        .collect();
    let selected = if selected.is_empty() {
        floor_regions(&sampled, &planes, signed_length(&sweep), tolerance)
    } else {
        selected
    };
    let Some(ResolvedProfileSelection::Regions(regions)) =
        super::trimmed_selection(&sampled, &selected, tolerance)
    else {
        return Ok(None);
    };
    let Ok(candidate) = PlanarProfileRef::sketch_regions(sketch.id.clone(), regions) else {
        return Ok(None);
    };
    if selection_agrees(
        sketch,
        resolution,
        &ProfileRef::Planar(candidate.clone()),
        state_ids,
        sweep,
    )? {
        Ok(Some(candidate))
    } else {
        Ok(None)
    }
}

/// The regions of a blind Cut that its floor covers, when no changed plane
/// held material before the transition. A Cut that starts in air above a
/// curved face deletes and changes no face parallel to the sketch at the
/// sketch plane. The face it inserts on the plane at the signed length is
/// the section of the tool in the material, so the regions that face covers
/// are the regions the Cut removed. `selection_agrees` checks the result
/// against every changed face.
fn floor_regions(
    sampled: &[super::Sampled<'_>],
    planes: &[PlaneSides],
    far: Option<f64>,
    tolerance: f64,
) -> Vec<usize> {
    let Some(far) = far else {
        return Vec::new();
    };
    let mut floors = planes
        .iter()
        .filter(|plane| plane.before.is_empty() && (plane.offset - far).abs() <= tolerance);
    let (Some(floor), None) = (floors.next(), floors.next()) else {
        return Vec::new();
    };
    sampled
        .iter()
        .enumerate()
        .filter(|(_, region)| floor.after.iter().any(|cap| cap.contains(region.point)))
        .map(|(index, _)| index)
        .collect()
}

pub(in crate::design::profile_select) fn added_candidate(
    sketch: &cadmpeg_ir::sketches::Sketch,
    resolution: super::super::ScopedExtrudeProfileResolution<'_>,
    state_ids: (i64, i64),
    sweep: Sweep<'_>,
) -> Result<Option<PlanarProfileRef>, CodecError> {
    if matches!(sweep.extent, ExtrudeExtent::TwoSided { .. }) {
        return two_sided::added_candidate(sketch, resolution, state_ids, sweep);
    }
    if !matches!(sweep.operation, BooleanOp::Join | BooleanOp::NewBody) {
        return Ok(None);
    }
    let Some(far) = blind_length(&sweep) else {
        return Ok(None);
    };
    let Some((after, state)) = unique_state(resolution.histories, state_ids.0) else {
        return Ok(None);
    };
    let Some(transition) = state
        .transition
        .as_ref()
        .filter(|transition| transition.previous_state_id == Some(state_ids.1))
    else {
        return Ok(None);
    };
    let Some(frame) = PlaneFrame::of_sketch(sketch) else {
        return Ok(None);
    };
    let Some((height, caps)) = super::far_caps(
        &frame,
        after,
        &transition.topology.faces.inserted,
        resolution.linear_tolerance,
    ) else {
        return Ok(None);
    };
    if (height - far).abs() > resolution.linear_tolerance || caps.iter().any(Cap::assumed) {
        return Ok(None);
    }
    let work = ProfileGeometryWork::new();
    let scoped = super::super::ScopedExtrudeProfileResolution {
        profile_work: &work,
        ..resolution
    };
    let Some(ResolvedProfileSelection::Regions(regions)) =
        super::cap_profile_selection(sketch, scoped, state_ids.0, state_ids.1, None)?
    else {
        return Ok(None);
    };
    if work.assumed_flags().into_iter().any(|assumed| assumed) {
        return Ok(None);
    }
    let outline = caps
        .iter()
        .flat_map(|cap| cap.pieces.iter().cloned())
        .collect::<Vec<_>>();
    let Some((before, _)) = unique_state(resolution.histories, state_ids.1) else {
        return Ok(None);
    };
    let deleted = deleted_sides(before, &transition.topology.faces.deleted, &frame);
    if !inserted_sides_agree(
        after,
        &transition.topology.faces.inserted,
        &deleted,
        &frame,
        &outline,
        (far.min(0.0), far.max(0.0)),
        resolution.linear_tolerance,
    ) {
        return Ok(None);
    }
    Ok(PlanarProfileRef::sketch_regions(sketch.id.clone(), regions).ok())
}

fn signed_length(sweep: &Sweep<'_>) -> Option<f64> {
    if !matches!(sweep.operation, BooleanOp::Cut) {
        return None;
    }
    blind_length(sweep)
}

pub(super) fn blind_length(sweep: &Sweep<'_>) -> Option<f64> {
    if !matches!(sweep.start, ExtrudeStart::ProfilePlane {}) {
        return None;
    }
    let ExtrudeExtent::OneSided { side } = sweep.extent else {
        return None;
    };
    if side.draft.is_some() {
        return None;
    }
    let LinearTermination::Blind { length } = side.termination else {
        return None;
    };
    match sweep.direction {
        ExtrudeDirection::ProfileNormal {} => Some(length.get()),
        ExtrudeDirection::ReversedProfileNormal {} => Some(-length.get()),
        _ => None,
    }
}

fn lateral_carrier_agrees(
    topology: &AsmHistoricalTopology,
    surface: i64,
    frame: &PlaneFrame,
    outline: &[Piece],
    tolerance: f64,
) -> bool {
    if let Some(cylinder) = topology
        .surface_cylinders
        .iter()
        .find(|row| row.surface == surface)
    {
        let center = frame.project(cylinder.origin);
        return (cylinder.axis.dot(frame.normal).abs() - 1.0).abs() <= PARALLEL_CAP
            && outline.iter().any(|piece| matches!(piece, Piece::Arc { center: other, radius, .. }
                if distance(center, *other) <= tolerance && (radius - cylinder.radius).abs() <= tolerance));
    }
    if let Some(plane) = topology
        .surface_planes
        .iter()
        .find(|row| row.surface == surface)
    {
        return plane.normal.dot(frame.normal).abs() <= PERPENDICULAR_SIDE
            && outline.iter().any(|piece| match piece {
                Piece::Line(start, end) => [start, end].iter().all(|point| {
                    let position = frame
                        .origin
                        .translated(frame.u.scale(point.u) + frame.v.scale(point.v), 1.0);
                    position.vector_from(plane.origin).dot(plane.normal).abs() <= tolerance
                }),
                Piece::Arc { .. } => false,
            });
    }
    false
}

/// The carriers of the faces that use `edge`.
fn edge_surfaces(topology: &AsmHistoricalTopology, edge: i64) -> HashSet<i64> {
    let mut surfaces = HashSet::new();
    for row in topology
        .coedge_topology
        .iter()
        .filter(|row| row.edge == edge)
    {
        for face in topology
            .face_loops
            .iter()
            .filter(|face| face.member_refs.contains(&row.owner_loop))
        {
            surfaces.extend(
                topology
                    .face_surfaces
                    .iter()
                    .filter(|binding| binding.entity == face.owner_ref)
                    .map(|binding| binding.carrier),
            );
        }
    }
    surfaces
}

/// Whether the edge of a lateral cylinder face that a transverse cylinder
/// cuts off stays within `travel`. The lateral cylinder `surface` has its
/// axis along the sketch normal. A second cylinder that uses `edge` has an
/// axis perpendicular to the normal. Each point of the edge has a height
/// `h` and a position `w` along the direction perpendicular to both axes
/// with `(h - axis_height)^2 + w^2 = radius^2`. The edge lies on the branch
/// of the vertex `start`, and `w` ranges over the lateral circle's diameter
/// about the second axis. The edge stays on that branch when the circle
/// does not reach the second cylinder's silhouette.
fn bounded_lateral_cylinder_cap(
    topology: &AsmHistoricalTopology,
    surface: i64,
    edge: i64,
    frame: &PlaneFrame,
    start: Point3,
    travel: (f64, f64),
    tolerance: f64,
) -> bool {
    let Some(lateral) = topology
        .surface_cylinders
        .iter()
        .find(|cylinder| cylinder.surface == surface)
    else {
        return false;
    };
    let surfaces = edge_surfaces(topology, edge);
    topology
        .surface_cylinders
        .iter()
        .filter(|cylinder| cylinder.surface != surface && surfaces.contains(&cylinder.surface))
        .any(|cap| {
            let Some(axis) = unit(cap.axis) else {
                return false;
            };
            if axis.dot(frame.normal).abs() > PERPENDICULAR_SIDE || cap.radius <= tolerance {
                return false;
            }
            let Some(across) = unit(frame.normal.cross(axis)) else {
                return false;
            };
            let axis_height = frame.height(cap.origin);
            let center = lateral.origin.vector_from(cap.origin).dot(across);
            let (near, far) = (center - lateral.radius, center + lateral.radius);
            let narrowest = if near <= 0.0 && far >= 0.0 {
                0.0
            } else {
                near.abs().min(far.abs())
            };
            let widest = near.abs().max(far.abs());
            if widest >= cap.radius - tolerance {
                return false;
            }
            let (low, high) = (
                (cap.radius.powi(2) - widest.powi(2)).sqrt(),
                (cap.radius.powi(2) - narrowest.powi(2)).sqrt(),
            );
            let side = frame.height(start) - axis_height;
            let (from, to) = if side >= 0.0 {
                (axis_height + low, axis_height + high)
            } else {
                (axis_height - high, axis_height - low)
            };
            from >= travel.0 - tolerance && to <= travel.1 + tolerance
        })
}

fn bounded_lateral_ellipse(
    topology: &AsmHistoricalTopology,
    surface: i64,
    edge: i64,
    frame: &PlaneFrame,
    outline: &[Piece],
    travel: (f64, f64),
    tolerance: f64,
) -> bool {
    let Some(plane) = topology
        .surface_planes
        .iter()
        .find(|plane| plane.surface == surface)
    else {
        return false;
    };
    let tangent = frame.normal.cross(plane.normal);
    if tangent.norm() <= PERPENDICULAR_SIDE {
        return false;
    }
    let tangent = tangent.scale(1.0 / tangent.norm());
    let surfaces = edge_surfaces(topology, edge);
    for cylinder in topology
        .surface_cylinders
        .iter()
        .filter(|cylinder| surfaces.contains(&cylinder.surface))
    {
        let norm = cylinder.axis.norm();
        if norm == 0.0 || cylinder.radius <= tolerance {
            continue;
        }
        let axis = cylinder.axis.scale(1.0 / norm);
        let denominator = plane.normal.dot(axis);
        if denominator.abs() <= TRANSVERSE_CYLINDER * plane.normal.norm() {
            continue;
        }
        let center = cylinder.origin.translated(
            axis,
            -cylinder.origin.vector_from(plane.origin).dot(plane.normal) / denominator,
        );
        let span = |direction: cadmpeg_ir::math::Vector3| {
            let radial = direction - plane.normal.scale(direction.dot(axis) / denominator);
            cylinder.radius * (radial - axis.scale(radial.dot(axis))).norm()
        };
        let height = frame.height(center);
        let height_span = span(frame.normal);
        if height - height_span < travel.0 - tolerance
            || height + height_span > travel.1 + tolerance
        {
            continue;
        }
        let projected_span = span(tangent);
        let extent = Piece::Line(
            frame.project(center.translated(tangent, -projected_span)),
            frame.project(center.translated(tangent, projected_span)),
        );
        if extent.covered_by(&outline.iter().collect::<Vec<_>>(), tolerance) {
            return true;
        }
    }
    false
}

fn edge_height_is_bounded(
    topology: &AsmHistoricalTopology,
    frame: &PlaneFrame,
    edge: i64,
    start: Point3,
    end: Point3,
    tolerance: f64,
) -> bool {
    let axis = topology
        .edge_curves
        .iter()
        .find(|row| row.entity == edge)
        .and_then(|row| row.carrier)
        .and_then(|curve| topology.curve_axes.iter().find(|row| row.curve == curve));
    if let Some(axis) = axis {
        let chord = end.vector_from(start);
        let off_line = |point: Point3| {
            point.vector_from(axis.origin).cross(axis.direction).norm() / axis.direction.norm()
        };
        if chord.norm() > tolerance
            && axis.direction.cross(chord).norm()
                <= STRAIGHT_CARRIER * axis.direction.norm() * chord.norm()
            && off_line(start) <= tolerance
            && off_line(end) <= tolerance
        {
            return true;
        }
    }
    if (frame.height(start) - frame.height(end)).abs() > tolerance {
        return false;
    }
    topology.coedge_topology.iter().filter(|row| row.edge == edge).any(|row| {
        topology.face_loops.iter().filter(|face| face.member_refs.contains(&row.owner_loop)).any(|face| {
            matches!(parallel_plane(topology, face.owner_ref, frame.normal), ParallelPlane::At(point)
                if (frame.height(point) - frame.height(start)).abs() <= tolerance)
        })
    })
}

/// The heights above the sketch plane that the tool of a blind Extrude spans,
/// from its start plane along its direction, or on both sides of the profile
/// plane for a symmetric extent. `None` for a start, direction or
/// side that is not a blind distance without draft.
fn extent_interval(sweep: &Sweep<'_>) -> Option<(f64, f64)> {
    let start = match sweep.start {
        ExtrudeStart::ProfilePlane {} => 0.0,
        ExtrudeStart::OffsetProfilePlane { offset } => offset.get(),
        _ => return None,
    };
    let sign = match sweep.direction {
        ExtrudeDirection::ProfileNormal {} => 1.0,
        ExtrudeDirection::ReversedProfileNormal {} => -1.0,
        _ => return None,
    };
    let blind = |side: &ExtrudeSide| match side.termination {
        LinearTermination::Blind { length } if side.draft.is_none() => Some(length.get()),
        _ => None,
    };
    let (first, second) = match sweep.extent {
        ExtrudeExtent::OneSided { side } => (start, start + sign * blind(side)?),
        ExtrudeExtent::TwoSided { first, second } => {
            (start - sign * blind(second)?, start + sign * blind(first)?)
        }
        ExtrudeExtent::Symmetric { side } => {
            // The mirrored side spans both sides of the profile plane, not of
            // an offset start.
            let half = blind(side)? / 2.0;
            (-half, half)
        }
    };
    Some((first.min(second), first.max(second)))
}

/// Whether the planes parallel to the sketch within the extent agree with the
/// result of an Intersect: no plane holds material over a removed region after
/// the transition, and no plane that held material before it gains material
/// over a kept region. A plane that held no material before the transition is
/// the end face of the tool. The body also extends beyond the regions of the
/// sketch, so the outline of the changed material is not compared with the
/// region boundaries.
fn intersect_agrees(
    planes: &[PlaneSides],
    sampled: &[super::Sampled<'_>],
    removed: &HashSet<usize>,
    kept: &[usize],
) -> bool {
    let removed_material = removed.iter().any(|index| {
        planes
            .iter()
            .flat_map(|plane| &plane.after)
            .any(|cap| cap.contains(sampled[*index].point))
    });
    let kept_gain = kept.iter().any(|index| {
        let point = sampled[*index].point;
        planes
            .iter()
            .filter(|plane| !plane.before.is_empty())
            .any(|plane| {
                !plane.before.iter().any(|cap| cap.contains(point))
                    && plane.after.iter().any(|cap| cap.contains(point))
            })
    });
    !removed_material && !kept_gain
}

/// Every cap that `topology` holds on the plane parallel to the sketch at
/// `offset`, whether or not a transition changed its face. `None` when a face
/// on the plane has no proven cap or a face has an unreadable plane.
fn plane_material(
    frame: &PlaneFrame,
    topology: &AsmHistoricalTopology,
    offset: f64,
    tolerance: f64,
) -> Option<Vec<Cap>> {
    let index = HistoricalTopologyIndex::new(topology);
    let mut caps = Vec::new();
    for face in &topology.faces {
        let point = match parallel_plane_indexed(&index, *face, frame.normal) {
            ParallelPlane::At(point) => point,
            ParallelPlane::Elsewhere => continue,
            ParallelPlane::Unreadable => return None,
        };
        if (frame.height(point) - offset).abs() <= tolerance {
            caps.push(face_cap_indexed(frame, &index, *face, tolerance)?);
        }
    }
    Some(caps)
}

/// The curve and the ordered parameter interval of one boundary piece, which
/// the two faces on either side of the piece share.
fn fragment(
    use_: &cadmpeg_ir::features::profiles::SketchProfileBoundaryUse,
) -> (&cadmpeg_ir::sketches::SketchEntityId, [u64; 2]) {
    let [first, second] = use_.parameter_range.endpoints();
    (
        &use_.entity,
        [first.min(second).to_bits(), first.max(second).to_bits()],
    )
}

/// The unique largest set of atomic faces, none of them in `removed`, whose
/// union has exactly the curves `named` as its boundary. A face that touches a
/// named curve is a candidate. A candidate with a boundary piece on an unnamed
/// curve needs the face across that piece in the set, so a face that leaves
/// such a piece on the outside is dropped, and the set shrinks until every
/// remaining unnamed piece lies inside it. Empty when a named curve is not a
/// boundary curve of the remaining faces.
fn profile_faces(
    sampled: &[super::Sampled<'_>],
    removed: &HashSet<usize>,
    named: &HashSet<cadmpeg_ir::sketches::SketchEntityId>,
) -> Vec<usize> {
    let mut across: HashMap<(&cadmpeg_ir::sketches::SketchEntityId, [u64; 2]), Vec<usize>> =
        HashMap::new();
    for (index, region) in sampled.iter().enumerate() {
        for use_ in &region.region.boundary {
            across.entry(fragment(use_)).or_default().push(index);
        }
    }
    let mut inside: Vec<bool> = sampled
        .iter()
        .enumerate()
        .map(|(index, region)| {
            !removed.contains(&index)
                && region
                    .region
                    .boundary
                    .iter()
                    .any(|use_| named.contains(&use_.entity))
        })
        .collect();
    loop {
        let mut shrunk = false;
        for (index, region) in sampled.iter().enumerate() {
            if !inside[index] {
                continue;
            }
            let open = region.region.boundary.iter().any(|use_| {
                !named.contains(&use_.entity)
                    && !across[&fragment(use_)]
                        .iter()
                        .any(|other| *other != index && inside[*other])
            });
            if open {
                inside[index] = false;
                shrunk = true;
            }
        }
        if !shrunk {
            break;
        }
    }
    let faces: Vec<usize> = (0..sampled.len()).filter(|index| inside[*index]).collect();
    let mut bounding: HashSet<&cadmpeg_ir::sketches::SketchEntityId> = HashSet::new();
    for &index in &faces {
        for use_ in &sampled[index].region.boundary {
            if !across[&fragment(use_)]
                .iter()
                .any(|other| *other != index && inside[*other])
            {
                bounding.insert(&use_.entity);
            }
        }
    }
    if named.iter().all(|id| bounding.contains(id)) {
        faces
    } else {
        Vec::new()
    }
}

/// The regions of an Intersect that its transition kept, when the transition
/// removed at least one other region. An Intersect keeps the body inside the
/// profile. Among the planes parallel to the sketch within the extent of the
/// Intersect, a region is removed when the material of a plane held its
/// interior before the transition and not after it. The material of a plane
/// is every face on it, changed or not, and a plane that no changed face lies
/// on counts when every face on it has a proven cap. The kept regions are the
/// faces that `profile_faces` returns for the removed regions and the named
/// curves: their union has exactly the named curves as its boundary. A region
/// whose material a plane holds on both sides must be among them. `None` when
/// a plane within the extent has a changed face with no proven cap, when a
/// region is both removed and held, when no removed region or no kept face
/// remains, when a kept face encloses an island, or when the removed regions
/// disagree with the planes.
pub(in crate::design::profile_select) fn intersect_candidate(
    sketch: &cadmpeg_ir::sketches::Sketch,
    resolution: super::super::ScopedExtrudeProfileResolution<'_>,
    state_ids: (i64, i64),
    sweep: Sweep<'_>,
    named: &HashSet<cadmpeg_ir::sketches::SketchEntityId>,
) -> Result<Option<PlanarProfileRef>, CodecError> {
    if !matches!(sweep.operation, BooleanOp::Intersect) || named.is_empty() {
        return Ok(None);
    }
    let Some((low, high)) = extent_interval(&sweep) else {
        return Ok(None);
    };
    let Some((after, state)) = unique_state(resolution.histories, state_ids.0) else {
        return Ok(None);
    };
    let Some(transition) = state
        .transition
        .as_ref()
        .filter(|transition| transition.previous_state_id == Some(state_ids.1))
    else {
        return Ok(None);
    };
    let Some((before, _)) = unique_state(resolution.histories, state_ids.1) else {
        return Ok(None);
    };
    let tolerance = resolution.linear_tolerance;
    let Some((mut planes, gaps)) = changed_planes_with_gaps(
        sketch,
        (before, after),
        &transition.topology.faces,
        tolerance,
    ) else {
        return Ok(None);
    };
    let within = |offset: f64| offset >= low - tolerance && offset <= high + tolerance;
    if gaps.iter().any(|offset| within(*offset)) {
        return Ok(None);
    }
    planes.retain(|plane| within(plane.offset));
    let Some(frame) = PlaneFrame::of_sketch(sketch) else {
        return Ok(None);
    };
    for plane in &mut planes {
        let (Some(held_before), Some(held_after)) = (
            plane_material(&frame, before, plane.offset, tolerance),
            plane_material(&frame, after, plane.offset, tolerance),
        ) else {
            return Ok(None);
        };
        plane.before = held_before;
        plane.after = held_after;
    }
    // A plane that no face of the transition changed still shows material
    // that the transition retained.
    let mut offsets: Vec<f64> = Vec::new();
    for topology in [before, after] {
        let index = HistoricalTopologyIndex::new(topology);
        for face in &topology.faces {
            let ParallelPlane::At(point) = parallel_plane_indexed(&index, *face, frame.normal)
            else {
                continue;
            };
            let offset = frame.height(point);
            if within(offset)
                && !planes
                    .iter()
                    .map(|plane| plane.offset)
                    .chain(offsets.iter().copied())
                    .any(|known| (known - offset).abs() <= tolerance)
            {
                offsets.push(offset);
            }
        }
    }
    for offset in offsets {
        let (Some(held_before), Some(held_after)) = (
            plane_material(&frame, before, offset, tolerance),
            plane_material(&frame, after, offset, tolerance),
        ) else {
            return Ok(None);
        };
        planes.push(PlaneSides {
            offset,
            before: held_before,
            after: held_after,
        });
    }
    let work = ProfileGeometryWork::new();
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
    let Some(sampled) = sample_regions(&regions, tolerance, resolution.profile_work) else {
        return Ok(None);
    };
    let mut removed = HashSet::new();
    let mut retained = Vec::new();
    for (index, region) in sampled.iter().enumerate() {
        let (mut lost, mut held) = (false, false);
        for plane in &planes {
            let before = plane.before.iter().any(|cap| cap.contains(region.point));
            let after = plane.after.iter().any(|cap| cap.contains(region.point));
            match (before, after) {
                (true, false) => lost = true,
                (true, true) => held = true,
                _ => {}
            }
        }
        match (lost, held) {
            (true, true) => return Ok(None),
            (true, false) => {
                removed.insert(index);
            }
            (false, true) => retained.push(index),
            (false, false) => {}
        }
    }
    if removed.is_empty() {
        return Ok(None);
    }
    let kept = profile_faces(&sampled, &removed, named);
    let kept_set: HashSet<usize> = kept.iter().copied().collect();
    if kept.is_empty()
        || retained.iter().any(|index| !kept_set.contains(index))
        || kept
            .iter()
            .any(|index| sampled[*index].region.encloses_island)
    {
        return Ok(None);
    }
    let Some(ResolvedProfileSelection::Regions(regions)) =
        super::trimmed_selection(&sampled, &kept, tolerance)
    else {
        return Ok(None);
    };
    let Ok(candidate) = PlanarProfileRef::sketch_regions(sketch.id.clone(), regions) else {
        return Ok(None);
    };
    if !intersect_agrees(&planes, &sampled, &removed, &kept) {
        return Ok(None);
    }
    if kept
        .iter()
        .any(|index| sampled[*index].region.island_absence_assumed)
    {
        work.note_island_absence_assumed();
    }
    Ok(Some(candidate))
}

#[cfg(test)]
pub(in crate::design::profile_select) mod tests;
