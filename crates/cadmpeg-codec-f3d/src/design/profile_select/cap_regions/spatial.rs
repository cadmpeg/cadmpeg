// SPDX-License-Identifier: Apache-2.0
//! Certify spatial Sketch profile sets against the far caps of an Extrude
//! transition.
//!
//! The selected profiles of one spatial Sketch lie on one plane. Each profile
//! is one closed boundary of lines, circles and circular arcs. The Extrude
//! transition inserts planar faces parallel to that plane; those at the
//! offset farthest from it repeat the swept region there. The profile set is
//! certified when every far face has proven carriers and the outline of the
//! union of the profiles and the outline of the union of the far faces cover
//! each other, carrier by carrier over their whole intervals.

use std::f64::consts::TAU;

use cadmpeg_ir::features::geometry::FinitePoint3;
use cadmpeg_ir::math::{Point2, Vector3};
use cadmpeg_ir::scalar::PositiveLength;
use cadmpeg_ir::sketches::{SpatialSketch, SpatialSketchEntity, SpatialSketchGeometryDefinition};
use cadmpeg_ir::units::UnitVector3;

use super::{
    face_outlines_cover_caps, far_caps, polygon_area, unique_state, Cap, Piece, PlaneFrame,
    PARALLEL_CAP,
};
use crate::history_records::AsmHistory;

/// Whether the far caps of the transition from `state_ids.1` to
/// `state_ids.0` prove the union of the `selected` profiles of `sketch`.
pub(in crate::design::profile_select) fn profiles_agree(
    sketch: &SpatialSketch,
    entities: &[SpatialSketchEntity],
    selected: &[u32],
    histories: &[AsmHistory],
    state_ids: (i64, i64),
    tolerance: f64,
) -> bool {
    let Some(first) = selected
        .first()
        .and_then(|index| sketch.profiles.get(usize::try_from(*index).ok()?))
    else {
        return false;
    };
    let Some(frame) = PlaneFrame::of_plane(first.origin().get(), Vector3::from(first.normal()))
    else {
        return false;
    };
    let mut caps = Vec::new();
    for index in selected {
        let Some(profile) = usize::try_from(*index)
            .ok()
            .and_then(|index| sketch.profiles.get(index))
        else {
            return false;
        };
        if (Vector3::from(profile.normal()).dot(frame.normal()).abs() - 1.0).abs() > PARALLEL_CAP
            || frame.height(profile.origin().get()).abs() > tolerance
        {
            return false;
        }
        let Some(cap) = profile_cap(&frame, sketch, profile, entities, tolerance) else {
            return false;
        };
        caps.push(cap);
    }
    let Some((after, state)) = unique_state(histories, state_ids.0) else {
        return false;
    };
    let Some(transition) = state.transition.as_ref() else {
        return false;
    };
    if transition.previous_state_id != Some(state_ids.1) {
        return false;
    }
    let Some((far, faces)) = far_caps(
        &frame,
        after,
        &transition.topology.faces.inserted,
        None,
        tolerance,
    ) else {
        return false;
    };
    far.abs() > tolerance
        && !faces.iter().any(Cap::assumed)
        && face_outlines_cover_caps(&caps, &faces, tolerance)
}

/// The closed boundary of one spatial profile as a cap in `frame`. `None`
/// when a boundary curve is not a line, circle or circular arc on the frame
/// plane, or when the boundary does not close.
fn profile_cap(
    frame: &PlaneFrame,
    sketch: &SpatialSketch,
    profile: &cadmpeg_ir::sketches::SpatialSketchProfile,
    entities: &[SpatialSketchEntity],
    tolerance: f64,
) -> Option<Cap> {
    let mut pieces = Vec::new();
    for use_ in profile.boundary() {
        let mut matches = entities
            .iter()
            .filter(|entity| entity.sketch == sketch.id && entity.id() == &use_.entity);
        let entity = matches.next()?;
        if matches.next().is_some() {
            return None;
        }
        pieces.push(boundary_piece(
            frame,
            entity.geometry.definition(),
            use_.reversed,
            tolerance,
        )?);
    }
    let mut ring = Vec::new();
    let mut corners = Vec::new();
    for (index, piece) in pieces.iter().enumerate() {
        let [start, _, end] = piece.points();
        let [next, _, _] = pieces[(index + 1) % pieces.len()].points();
        if super::distance(end, next) > tolerance {
            return None;
        }
        corners.push(start);
        piece.push(&mut ring);
    }
    if ring.len() < 3 {
        return None;
    }
    let area = polygon_area(&ring).abs();
    (area > tolerance * tolerance).then(|| {
        let measured = vec![None; pieces.len()];
        Cap::new(vec![ring], pieces, corners, area, measured)
    })
}

/// One boundary curve as a frame piece traversed in its boundary direction.
fn boundary_piece(
    frame: &PlaneFrame,
    geometry: &SpatialSketchGeometryDefinition<FinitePoint3, UnitVector3, PositiveLength>,
    reversed: bool,
    tolerance: f64,
) -> Option<Piece> {
    let on_plane = |height: f64| height.abs() <= tolerance;
    match geometry {
        SpatialSketchGeometryDefinition::Line { start, end } => {
            let (start, end) = (start.get(), end.get());
            if !on_plane(frame.height(start)) || !on_plane(frame.height(end)) {
                return None;
            }
            let (start, end) = (frame.project(start), frame.project(end));
            Some(if reversed {
                Piece::Line(end, start)
            } else {
                Piece::Line(start, end)
            })
        }
        SpatialSketchGeometryDefinition::Circle {
            center,
            normal,
            reference_direction,
            radius,
        } => arc_piece(
            frame,
            ArcCurve {
                center: center.get(),
                normal: Vector3::from(*normal),
                reference: Vector3::from(*reference_direction),
                radius: radius.get(),
                start: 0.0,
                end: TAU,
            },
            reversed,
            tolerance,
        ),
        SpatialSketchGeometryDefinition::Arc {
            center,
            normal,
            reference_direction,
            radius,
            start_angle,
            end_angle,
        } => arc_piece(
            frame,
            ArcCurve {
                center: center.get(),
                normal: Vector3::from(*normal),
                reference: Vector3::from(*reference_direction),
                radius: radius.get(),
                start: start_angle.get(),
                end: end_angle.get(),
            },
            reversed,
            tolerance,
        ),
        _ => None,
    }
}

/// A model-space circular arc from `start` to `end` radians.
#[derive(Clone, Copy)]
struct ArcCurve {
    center: cadmpeg_ir::math::Point3,
    normal: Vector3,
    reference: Vector3,
    radius: f64,
    start: f64,
    end: f64,
}

fn arc_piece(frame: &PlaneFrame, arc: ArcCurve, reversed: bool, tolerance: f64) -> Option<Piece> {
    let normal = arc.normal.unit()?;
    let alignment = normal.dot(frame.normal());
    if (alignment.abs() - 1.0).abs() > PARALLEL_CAP
        || frame.height(arc.center).abs() > tolerance
        || arc.radius.partial_cmp(&tolerance) != Some(std::cmp::Ordering::Greater)
        || arc.end.partial_cmp(&arc.start) != Some(std::cmp::Ordering::Greater)
    {
        return None;
    }
    let center = frame.project(arc.center);
    let reference = frame.project(arc.center.translated(arc.reference, 1.0));
    let base = (reference.v - center.v).atan2(reference.u - center.u);
    // An arc whose normal is opposite the frame normal turns clockwise in it.
    let turn = alignment.signum();
    let start = base + turn * arc.start;
    let sweep = turn * (arc.end - arc.start);
    Some(if reversed {
        Piece::Arc {
            center: Point2::new(center.u, center.v),
            radius: arc.radius,
            start: start + sweep,
            sweep: -sweep,
        }
    } else {
        Piece::Arc {
            center: Point2::new(center.u, center.v),
            radius: arc.radius,
            start,
            sweep,
        }
    })
}

#[cfg(test)]
mod tests;
