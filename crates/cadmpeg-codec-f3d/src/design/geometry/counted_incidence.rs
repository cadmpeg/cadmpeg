// SPDX-License-Identifier: Apache-2.0
//! Check counted member pieces against the partition of a member ring.

use super::{
    analytic_segment_intersections, arrangement_split_parameters, profile_use_segment,
    sketch_geometry_parameter_range, sketch_geometry_speed_bound, ArrangementRing, CodecError,
    Point2, ProfileBoundarySegment, ProfileGeometryWork,
};
use cadmpeg_ir::sketches::{SketchEntity, SketchEntityUse, SketchGeometryDefinition};
use std::f64::consts::TAU;

/// Whether each member `(use, [k, n])` names a distinct fragment of `ring`
/// with its curve and direction, and that fragment is piece `k` of the `n`
/// pieces into which the other curves of the ring cut the curve. Pieces
/// follow the curve's parameter direction. The first piece of a circle
/// holds angle zero, or ends there.
pub(super) fn ring_holds(
    ring: &ArrangementRing,
    members: &[(SketchEntityUse, [u32; 2])],
    entities: &[SketchEntity],
    tolerance: f64,
    work: &ProfileGeometryWork,
) -> Result<bool, CodecError> {
    if ring.fragments.len() != members.len()
        || ring
            .fragments
            .iter()
            .any(|fragment| !fragment.coincident.is_empty())
    {
        return Ok(false);
    }
    let mut curves = Vec::<&SketchEntity>::new();
    for fragment in &ring.fragments {
        if curves
            .iter()
            .any(|curve| curve.id() == &fragment.curve.entity)
        {
            continue;
        }
        let Some(entity) = entities
            .iter()
            .find(|entity| entity.id() == &fragment.curve.entity)
        else {
            return Ok(false);
        };
        curves.push(entity);
    }
    if !curves
        .len()
        .checked_mul(curves.len())
        .and_then(|count| count.checked_add(members.len().checked_mul(members.len())?))
        .is_some_and(|count| work.arrangement.charge_by(count))
    {
        work.arrangement_skipped.set(true);
        return Ok(false);
    }
    let mut segments = Vec::new();
    for curve in &curves {
        let Some(segment) = full_segment(curve) else {
            return Ok(false);
        };
        segments.push(segment);
    }
    let mut claimed = vec![false; ring.fragments.len()];
    for (member, [ordinal, count]) in members {
        let Some(curve_index) = curves.iter().position(|curve| curve.id() == &member.entity) else {
            return Ok(false);
        };
        let mut cuts = Vec::new();
        for (other_index, other) in segments.iter().enumerate() {
            if other_index == curve_index {
                continue;
            }
            if let Some(points) = analytic_segment_intersections(&segments[curve_index], other) {
                cuts.extend(points.into_iter().flatten());
            }
        }
        let Some(CurvePieces {
            pieces,
            parameter_tolerance,
            periodic,
        }) = curve_pieces(curves[curve_index], &cuts, tolerance)?
        else {
            return Ok(false);
        };
        let Some(piece) = usize::try_from(*ordinal)
            .ok()
            .and_then(|ordinal| ordinal.checked_sub(1))
            .and_then(|index| pieces.get(index))
        else {
            return Ok(false);
        };
        if usize::try_from(*count).ok() != Some(pieces.len()) {
            return Ok(false);
        }
        let same = |left: f64, right: f64| {
            if periodic {
                let difference = (left - right).rem_euclid(TAU);
                difference.min(TAU - difference) <= parameter_tolerance
            } else {
                (left - right).abs() <= parameter_tolerance
            }
        };
        let Some(fragment) = ring
            .fragments
            .iter()
            .enumerate()
            .position(|(index, fragment)| {
                !claimed[index]
                    && fragment.curve == *member
                    && fragment
                        .range
                        .is_some_and(|[low, high]| same(low, piece[0]) && same(high, piece[1]))
            })
        else {
            return Ok(false);
        };
        claimed[fragment] = true;
    }
    Ok(true)
}

/// The segment of a whole line, circular arc, or circle.
fn full_segment(entity: &SketchEntity) -> Option<ProfileBoundarySegment> {
    match *entity.geometry.definition() {
        SketchGeometryDefinition::Circle { center, radius } => Some(ProfileBoundarySegment::Arc {
            center: center.get(),
            radius,
            start_angle: 0.0,
            end_angle: TAU,
        }),
        SketchGeometryDefinition::Line { .. } | SketchGeometryDefinition::Arc { .. } => {
            profile_use_segment(entity, false)
        }
        _ => None,
    }
}

/// The ordered parameter intervals of a curve's pieces.
struct CurvePieces {
    pieces: Vec<[f64; 2]>,
    parameter_tolerance: f64,
    /// The parameter is an angle modulo a full turn.
    periodic: bool,
}

/// The pieces into which the points `cuts` divide `entity`.
fn curve_pieces(
    entity: &SketchEntity,
    cuts: &[Point2],
    tolerance: f64,
) -> Result<Option<CurvePieces>, CodecError> {
    if let SketchGeometryDefinition::Circle { center, radius } = *entity.geometry.definition() {
        let center = center.get();
        let parameter_tolerance = tolerance / radius.get().max(tolerance);
        let mut angles = cuts
            .iter()
            .map(|point| {
                (point.v - center.v)
                    .atan2(point.u - center.u)
                    .rem_euclid(TAU)
            })
            .collect::<Vec<_>>();
        angles.sort_by(f64::total_cmp);
        angles.dedup_by(|right, left| *right - *left <= parameter_tolerance);
        if let (Some(first), Some(last)) = (angles.first().copied(), angles.last().copied()) {
            if angles.len() > 1 && first + TAU - last <= parameter_tolerance {
                angles.pop();
            }
        }
        let pieces = match angles.as_slice() {
            [] => vec![[0.0, TAU]],
            [first, ..] => {
                let mut pieces = Vec::new();
                if let Some(last) = angles.last() {
                    pieces.push([*last, first + TAU]);
                }
                pieces.extend(angles.windows(2).map(|pair| [pair[0], pair[1]]));
                pieces
            }
        };
        return Ok(Some(CurvePieces {
            pieces,
            parameter_tolerance,
            periodic: true,
        }));
    }
    let Some(range) = sketch_geometry_parameter_range(&entity.geometry) else {
        return Ok(None);
    };
    let Some(speed) = sketch_geometry_speed_bound(&entity.geometry, range) else {
        return Ok(None);
    };
    let Some(parameters) = arrangement_split_parameters(&entity.geometry, range, cuts, tolerance)?
    else {
        return Ok(None);
    };
    let pieces = parameters
        .windows(2)
        .map(|pair| [pair[0], pair[1]])
        .collect();
    Ok(Some(CurvePieces {
        pieces,
        parameter_tolerance: tolerance / speed.max(tolerance),
        periodic: false,
    }))
}
