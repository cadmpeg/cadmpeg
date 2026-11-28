// SPDX-License-Identifier: Apache-2.0
//! Certify the two end sections and changed faces of a two-sided blind Join.

use super::super::{
    changed_planes, face_cap, oriented_loops, outlines_agree, parallel_plane, sample_regions, sole,
    unique_state, Agreement, Cap, ParallelPlane, PlaneFrame, PlaneSides, Sampled, SIDE_OFFSET,
};
use super::{deleted_sides, inserted_sides_agree, split_piece, Sweep};
use crate::design::geometry::{atomic_sketch_regions, ProfileGeometryWork};
use crate::design::profile_select::{ResolvedProfileSelection, ScopedExtrudeProfileResolution};
use crate::history_records::{AsmHistoricalTopology, AsmHistoricalTransition};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::extents::{
    ExtrudeDirection, ExtrudeExtent, ExtrudeSide, ExtrudeStart, LinearTermination,
};
use cadmpeg_ir::features::operation_settings::BooleanOp;
use cadmpeg_ir::features::profiles::PlanarProfileRef;
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::Sketch;

fn travel(sweep: Sweep<'_>, planes: &[PlaneSides], tolerance: f64) -> Option<(f64, f64)> {
    if !matches!(sweep.operation, BooleanOp::Join | BooleanOp::NewBody) {
        return None;
    }
    let base = match sweep.start {
        ExtrudeStart::ProfilePlane {} => 0.0,
        ExtrudeStart::OffsetProfilePlane { offset } => offset.get(),
        _ => return None,
    };
    let ExtrudeExtent::TwoSided { first, second } = sweep.extent else {
        return None;
    };
    let length = |side: &ExtrudeSide| match side.termination {
        LinearTermination::Blind { length } if side.draft.is_none() => Some(length.get()),
        _ => None,
    };
    let (first, second) = (length(first)?, length(second)?);
    if first <= 0.0 || second <= 0.0 {
        return None;
    }
    let sign = match sweep.direction {
        ExtrudeDirection::ProfileNormal {} => 1.0,
        ExtrudeDirection::ReversedProfileNormal {} => -1.0,
        _ => return None,
    };
    let first = base + sign * first;
    let mut matching = [-sign * second, sign * second]
        .into_iter()
        .map(|second| (first.min(base + second), first.max(base + second)))
        .filter(|(low, high)| {
            high - low > tolerance
                && [low, high].into_iter().all(|height| {
                    planes
                        .iter()
                        .any(|plane| (plane.offset - height).abs() <= tolerance)
                })
        });
    let interval = matching.next()?;
    matching.next().is_none().then_some(interval)
}

fn difference(plane: &PlaneSides, point: Point2) -> bool {
    plane.before.iter().any(|cap| cap.contains(point))
        != plane.after.iter().any(|cap| cap.contains(point))
}

fn difference_cap(plane: &PlaneSides, tolerance: f64) -> Option<Cap> {
    let mut pieces = Vec::new();
    let sources = || plane.before.iter().chain(&plane.after);
    let endpoints: Vec<_> = sources()
        .flat_map(|source| &source.pieces)
        .flat_map(|piece| {
            let [start, _, end] = piece.points();
            [start, end]
        })
        .collect();
    for source in sources() {
        if source.assumed() {
            return None;
        }
        for piece in &source.pieces {
            for fragment in split_piece(piece, &endpoints, tolerance) {
                let (left, right) = fragment.sides_of_middle(SIDE_OFFSET * tolerance)?;
                if difference(plane, left) != difference(plane, right) {
                    pieces.push(fragment);
                }
            }
        }
    }
    Some(Cap::new(Vec::new(), pieces, Vec::new(), 0.0, Vec::new()))
}

fn changed_regions(plane: &PlaneSides, sampled: &[Sampled<'_>]) -> Vec<usize> {
    sampled
        .iter()
        .enumerate()
        .filter(|(_, region)| difference(plane, region.point))
        .map(|(index, _)| index)
        .collect()
}

fn unchanged_face(
    before: &AsmHistoricalTopology,
    after: &AsmHistoricalTopology,
    transition: &AsmHistoricalTransition,
    face: i64,
) -> bool {
    let same = || -> Option<()> {
        for family in [
            &transition.topology.surfaces,
            &transition.topology.curves,
            &transition.topology.points,
        ] {
            if !family.updated.is_empty() || !family.deleted.is_empty() {
                return None;
            }
        }
        let old_loops = oriented_loops(before, face)?;
        let new_loops = oriented_loops(after, face)?;
        if old_loops != new_loops {
            return None;
        }
        let old = sole(before.face_surfaces.iter().filter(|row| row.entity == face)).ok()??;
        let new = sole(after.face_surfaces.iter().filter(|row| row.entity == face)).ok()??;
        if old != new {
            return None;
        }
        let surface = old.carrier;
        let planes = before
            .surface_planes
            .iter()
            .filter(|row| row.surface == surface)
            .count();
        let cylinders = before
            .surface_cylinders
            .iter()
            .filter(|row| row.surface == surface)
            .count();
        if !matches!((planes, cylinders), (1, 0) | (0, 1)) {
            return None;
        }
        if transition.records.updated.contains(&surface)
            || before
                .surface_planes
                .iter()
                .filter(|row| row.surface == surface)
                .collect::<Vec<_>>()
                != after
                    .surface_planes
                    .iter()
                    .filter(|row| row.surface == surface)
                    .collect::<Vec<_>>()
            || before
                .surface_cylinders
                .iter()
                .filter(|row| row.surface == surface)
                .collect::<Vec<_>>()
                != after
                    .surface_cylinders
                    .iter()
                    .filter(|row| row.surface == surface)
                    .collect::<Vec<_>>()
        {
            return None;
        }
        let old = sole(before.face_loops.iter().filter(|row| row.owner_ref == face)).ok()??;
        let new = sole(after.face_loops.iter().filter(|row| row.owner_ref == face)).ok()??;
        if old != new || old.member_refs.is_empty() {
            return None;
        }
        for loop_ in &old.member_refs {
            if transition.records.updated.contains(loop_) {
                return None;
            }
            let old = sole(
                before
                    .loop_coedges
                    .iter()
                    .filter(|row| row.owner_ref == *loop_),
            )
            .ok()??;
            let new = sole(
                after
                    .loop_coedges
                    .iter()
                    .filter(|row| row.owner_ref == *loop_),
            )
            .ok()??;
            if old != new || old.member_refs.is_empty() {
                return None;
            }
            for coedge in &old.member_refs {
                let old = sole(
                    before
                        .coedge_topology
                        .iter()
                        .filter(|row| row.coedge == *coedge),
                )
                .ok()??;
                let new = sole(
                    after
                        .coedge_topology
                        .iter()
                        .filter(|row| row.coedge == *coedge),
                )
                .ok()??;
                if old.edge != new.edge || transition.records.updated.contains(&old.edge) {
                    return None;
                }
                let edge = old.edge;
                let old =
                    sole(before.edge_curves.iter().filter(|row| row.entity == edge)).ok()??;
                let new =
                    sole(after.edge_curves.iter().filter(|row| row.entity == edge)).ok()??;
                if old != new {
                    return None;
                }
                let curve = old.carrier?;
                if transition.records.updated.contains(&curve) {
                    return None;
                }
                let old =
                    sole(before.curve_axes.iter().filter(|row| row.curve == curve)).ok()??;
                let new = sole(after.curve_axes.iter().filter(|row| row.curve == curve)).ok()??;
                if old != new {
                    return None;
                }
            }
        }
        Some(())
    };
    same().is_some()
}

pub(super) fn added_candidate(
    sketch: &Sketch,
    resolution: ScopedExtrudeProfileResolution<'_>,
    state_ids: (i64, i64),
    sweep: Sweep<'_>,
) -> Result<Option<PlanarProfileRef>, CodecError> {
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
    let Some(frame) = PlaneFrame::of_sketch(sketch) else {
        return Ok(None);
    };
    let mut faces = transition.topology.faces.clone();
    faces
        .updated
        .retain(|face| !unchanged_face(before, after, transition, *face));
    let old: Vec<_> = faces
        .deleted
        .iter()
        .chain(&faces.updated)
        .copied()
        .collect();
    let new: Vec<_> = faces
        .inserted
        .iter()
        .chain(&faces.updated)
        .copied()
        .collect();
    let tolerance = resolution.linear_tolerance;
    for (topology, changed) in [(before, &old), (after, &new)] {
        for face in changed {
            match parallel_plane(topology, *face, frame.normal) {
                ParallelPlane::At(_) => {
                    if face_cap(&frame, topology, *face, tolerance).is_none() {
                        return Ok(None);
                    }
                }
                ParallelPlane::Unreadable => return Ok(None),
                ParallelPlane::Elsewhere => {
                    let Some(binding) = topology
                        .face_surfaces
                        .iter()
                        .find(|row| row.entity == *face)
                    else {
                        return Ok(None);
                    };
                    let planes = topology
                        .surface_planes
                        .iter()
                        .filter(|row| row.surface == binding.carrier)
                        .count();
                    let cylinders = topology
                        .surface_cylinders
                        .iter()
                        .filter(|row| row.surface == binding.carrier)
                        .count();
                    if !matches!((planes, cylinders), (1, 0) | (0, 1)) {
                        return Ok(None);
                    }
                }
            }
        }
    }
    let Some(planes) = changed_planes(sketch, (before, after), &faces, tolerance) else {
        return Ok(None);
    };
    let Some(travel) = travel(sweep, &planes, tolerance) else {
        return Ok(None);
    };
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
    if work.assumed_flags().into_iter().any(|assumed| assumed) {
        return Ok(None);
    }
    let Some(sampled) = sample_regions(&regions, tolerance, resolution.profile_work) else {
        return Ok(None);
    };
    let Some(end) = planes
        .iter()
        .find(|plane| (plane.offset - travel.1).abs() <= tolerance)
    else {
        return Ok(None);
    };
    let selected = changed_regions(end, &sampled);
    if selected.is_empty() {
        return Ok(None);
    }
    let mut ends = [false; 2];
    let mut outline = Vec::new();
    for plane in &planes {
        let covered = changed_regions(plane, &sampled);
        let Some(cap) = difference_cap(plane, tolerance) else {
            return Ok(None);
        };
        if (!cap.pieces.is_empty() || !covered.is_empty())
            && !matches!(
                outlines_agree(
                    &cap,
                    &sampled,
                    &covered,
                    resolution.entities,
                    tolerance,
                    resolution.ctx,
                )?,
                Agreement::Exact
            )
        {
            return Ok(None);
        }
        let endpoint = [travel.0, travel.1]
            .iter()
            .position(|height| (plane.offset - height).abs() <= tolerance);
        if let Some(endpoint) = endpoint {
            if covered != selected
                || selected.iter().any(|index| {
                    plane
                        .before
                        .iter()
                        .any(|cap| cap.contains(sampled[*index].point))
                })
            {
                return Ok(None);
            }
            ends[endpoint] = true;
            outline.extend(cap.pieces);
        } else if !covered.is_empty()
            && (plane.offset < travel.0 - tolerance
                || plane.offset > travel.1 + tolerance
                || covered.iter().any(|index| {
                    !selected.contains(index)
                        || plane
                            .after
                            .iter()
                            .any(|cap| cap.contains(sampled[*index].point))
                }))
        {
            return Ok(None);
        }
    }
    if !ends.into_iter().all(|end| end) {
        return Ok(None);
    }
    let deleted = deleted_sides(before, &old, &frame);
    if !inserted_sides_agree(after, &new, &deleted, &frame, &outline, travel, tolerance) {
        return Ok(None);
    }
    let old_lateral: Vec<_> = old
        .iter()
        .copied()
        .filter(|face| {
            matches!(
                parallel_plane(before, *face, frame.normal),
                ParallelPlane::Elsewhere
            )
        })
        .collect();
    let continued = deleted_sides(after, &new, &frame);
    if !old_lateral.is_empty()
        && !inserted_sides_agree(
            before,
            &old_lateral,
            &continued,
            &frame,
            &outline,
            travel,
            tolerance,
        )
    {
        return Ok(None);
    }
    let Some(ResolvedProfileSelection::Regions(regions)) =
        super::super::trimmed_selection(&sampled, &selected, tolerance)
    else {
        return Ok(None);
    };
    Ok(PlanarProfileRef::sketch_regions(sketch.id.clone(), regions).ok())
}

#[cfg(test)]
mod tests;
