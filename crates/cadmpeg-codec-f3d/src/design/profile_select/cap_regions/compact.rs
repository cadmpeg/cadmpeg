// SPDX-License-Identifier: Apache-2.0
//! Certify compact loop selections against an analytic Cut transition.

use super::super::ResolvedProfileSelection;
use super::{
    changed_planes, covered_regions, distance, face_cap, oriented_loops, outlines_agree,
    parallel_plane, sample_regions, unique_state, Agreement, Cap, ParallelPlane, Piece, PlaneFrame,
    PARALLEL_CAP, SIDE_OFFSET, STRAIGHT_CARRIER,
};
use crate::design::geometry::{atomic_sketch_regions, ProfileGeometryWork};
use crate::history_records::AsmHistoricalTopology;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{
    BooleanOp, ExtrudeDirection, ExtrudeExtent, ExtrudeStart, LinearTermination, PlanarProfileRef,
    ProfileRef,
};
use cadmpeg_ir::math::Point3;
use std::collections::HashSet;

const PERPENDICULAR_SIDE: f64 = 1.0e-9;

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
    let Some(sampled) = sample_regions(&regions, tolerance) else {
        return Ok(false);
    };
    let points: Vec<_> = sampled.iter().map(|region| region.point).collect();
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
        let mut cap = Cap {
            loops: Vec::new(),
            pieces: Vec::new(),
            corners: Vec::new(),
            area: 0.0,
            measured: Vec::new(),
        };
        for source in plane.before.iter().chain(&plane.after) {
            for piece in &source.pieces {
                let Some((left, right)) = piece.sides_of_middle(SIDE_OFFSET * tolerance) else {
                    return Ok(false);
                };
                if changed(left) != changed(right) {
                    cap.pieces.push(piece.clone());
                }
            }
        }
        if cap.pieces.is_empty() && covered.is_empty() {
            continue;
        }
        if !matches!(
            outlines_agree(
                &cap,
                &sampled,
                &covered,
                resolution.entities,
                tolerance,
                resolution.ctx
            )?,
            Agreement::Exact
        ) {
            return Ok(false);
        }
        for index in covered {
            let point = sampled[index].point;
            if plane.before.iter().any(|cap| cap.contains(point)) {
                witnessed.insert(index);
            } else if (plane.offset - far).abs() > tolerance {
                return Ok(false);
            }
        }
        outline.extend(cap.pieces);
    }
    if witnessed.len() != selected.len() {
        return Ok(false);
    }
    let mut side_count = 0;
    for face in &transition.topology.faces.inserted {
        let Some(binding) = after.face_surfaces.iter().find(|row| row.entity == *face) else {
            return Ok(false);
        };
        if let ParallelPlane::At(point) = parallel_plane(after, *face, frame.normal) {
            let height = frame.height(point);
            if height < far.min(0.0) - tolerance || height > far.max(0.0) + tolerance {
                return Ok(false);
            }
            continue;
        }
        if !lateral_carrier_agrees(after, binding.carrier, &frame, &outline, tolerance) {
            return Ok(false);
        }
        let Some(loops) = oriented_loops(after, *face) else {
            return Ok(false);
        };
        for (edge, start, end) in loops.into_iter().flatten() {
            if !edge_height_is_bounded(after, &frame, edge, start, end, tolerance) {
                return Ok(false);
            }
            for point in [start, end] {
                if !outline
                    .iter()
                    .any(|piece| piece.distance(frame.project(point)) <= tolerance)
                {
                    return Ok(false);
                }
                let height = frame.height(point);
                if height < far.min(0.0) - tolerance || height > far.max(0.0) + tolerance {
                    return Ok(false);
                }
            }
        }
        side_count += 1;
    }
    Ok(side_count > 0)
}

fn signed_length(sweep: &Sweep<'_>) -> Option<f64> {
    if !matches!(sweep.operation, BooleanOp::Cut)
        || !matches!(sweep.start, ExtrudeStart::ProfilePlane {})
    {
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
