// SPDX-License-Identifier: Apache-2.0
//! Selected material regions split into faces of the sketch loop arrangement.
//!
//! A boundary patch selects the rings that bound the extruded material. A
//! sketch loop that no ring selects and that lies in that material does not
//! bound it, so the material covers the loop. A consumer that matches regions
//! to the faces of the sketch arrangement needs one region per face. This
//! module states the material as the exterior face, with every loop directly
//! inside it as a hole, followed by one face for each unselected loop inside
//! the material.

use std::collections::HashSet;

use cadmpeg_ir::features::SketchProfileRegion;
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{Sketch, SketchEntityUse, SketchGeometryDefinition};

use super::{containment, distance, polar, LoopCurve, ProfileIndex, EPS_PROFILE_WIRE_POINT_MM};

/// Most profile-table loops that a face split compares pairwise.
const SPLIT_LOOP_LIMIT: usize = 128;
/// Largest sine of the angle between two lines that still counts as parallel.
const EPS_PARALLEL_SINE: f64 = 1.0e-12;

impl ProfileIndex<'_> {
    /// The faces of the selected material, when an unselected loop lies in it
    /// and the faces are exact.
    ///
    /// `selected[ring]` is the profile-table index of each selected ring, and
    /// `nested` pairs each exterior ring with its hole rings, by ring. The
    /// result is `None` when no unselected loop lies in the material, so the
    /// selected rings state it completely. It is also `None` when the faces
    /// are not exact: the table holds a loop without loop geometry or more
    /// loops than a split compares, the sketch has a curve outside its loops,
    /// or loops meet. The selected rings still state the material then.
    pub(super) fn material_faces(
        &self,
        sketch: &Sketch,
        table: &[Vec<SketchEntityUse>],
        selected: &[usize],
        nested: &[(usize, Vec<usize>)],
    ) -> Option<Vec<SketchProfileRegion>> {
        let count = table.len();
        let selected_loops = selected.iter().copied().collect::<HashSet<_>>();
        let curves = if count > SPLIT_LOOP_LIMIT {
            None
        } else {
            table
                .iter()
                .map(|profile| self.loop_curves(profile))
                .collect::<Option<Vec<_>>>()
        };
        let curves = curves?;
        let contains = containment(&curves)?;
        let mut regions = Vec::new();
        let mut involved = HashSet::new();
        let mut split = false;
        for (outer, holes) in nested {
            let outer = selected[*outer];
            let holes = holes.iter().map(|hole| selected[*hole]).collect::<Vec<_>>();
            let inner = selected
                .iter()
                .copied()
                .filter(|ring| *ring != outer && contains[outer][*ring])
                .collect::<Vec<_>>();
            let free = (0..count)
                .filter(|loop_| {
                    !selected_loops.contains(loop_)
                        && contains[outer][*loop_]
                        && !inner.iter().any(|ring| contains[*ring][*loop_])
                })
                .collect::<Vec<_>>();
            split |= !free.is_empty();
            involved.insert(outer);
            involved.extend(holes.iter().chain(&free).copied());
            regions.extend(faces_of(&contains, outer, &holes, &free)?);
        }
        if !split {
            return None;
        }
        let used = table
            .iter()
            .flatten()
            .map(|use_| &use_.entity)
            .collect::<HashSet<_>>();
        let outside = self.by_id.values().any(|entity| {
            entity.sketch == sketch.id
                && !entity.construction
                && !matches!(
                    entity.geometry.definition(),
                    SketchGeometryDefinition::Point { .. }
                )
                && !used.contains(entity.id())
        });
        let meets = involved.iter().any(|loop_| {
            (0..count)
                .filter(|other| other != loop_)
                .any(|other| loops_meet(&curves[*loop_], &curves[other]))
        });
        (!outside && !meets).then_some(regions)
    }
}

/// The faces of one material: the exterior, then each free loop. The members
/// are the selected holes, then the free loops. A member is a hole of the
/// innermost face loop that contains it.
fn faces_of(
    contains: &[Vec<bool>],
    outer: usize,
    holes: &[usize],
    free: &[usize],
) -> Option<Vec<SketchProfileRegion>> {
    let nodes = std::iter::once(outer)
        .chain(free.iter().copied())
        .collect::<Vec<_>>();
    let depth = |loop_: usize| contains.iter().filter(|row| row[loop_]).count();
    let parent = |loop_: usize| {
        nodes
            .iter()
            .copied()
            .filter(|node| *node != loop_ && contains[*node][loop_])
            .max_by_key(|node| depth(*node))
    };
    nodes
        .iter()
        .map(|node| {
            let members = holes
                .iter()
                .chain(free)
                .copied()
                .filter(|member| parent(*member) == Some(*node))
                .map(|member| u32::try_from(member).ok())
                .collect::<Option<Vec<_>>>()?;
            SketchProfileRegion::loops(u32::try_from(*node).ok()?, members).ok()
        })
        .collect()
}

/// Whether two closed loops share a point.
fn loops_meet(left: &[LoopCurve], right: &[LoopCurve]) -> bool {
    left.iter()
        .any(|a| right.iter().any(|b| curves_meet(*a, *b)))
}

/// Whether two oriented curves share a point within the kernel linear
/// tolerance. Coincident circles and collinear overlapping lines meet.
fn curves_meet(left: LoopCurve, right: LoopCurve) -> bool {
    let mut candidates = vec![left.ends().0, left.ends().1, right.ends().0, right.ends().1];
    match (left, right) {
        (LoopCurve::Line { start: a, end: b }, LoopCurve::Line { start: c, end: d }) => {
            candidates.extend(line_crossing((a, b), (c, d)));
        }
        (LoopCurve::Line { start, end }, round) | (round, LoopCurve::Line { start, end }) => {
            candidates.extend(line_circle((start, end), round));
        }
        _ => candidates.extend(circle_circle(left, right)),
    }
    candidates
        .into_iter()
        .any(|point| left.touches(point) && right.touches(point))
}

/// The crossing point of two non-parallel lines.
fn line_crossing(left: (Point2, Point2), right: (Point2, Point2)) -> Option<Point2> {
    let (r_u, r_v) = (left.1.u - left.0.u, left.1.v - left.0.v);
    let (s_u, s_v) = (right.1.u - right.0.u, right.1.v - right.0.v);
    let denominator = r_u.mul_add(s_v, -(r_v * s_u));
    if denominator.abs() <= EPS_PARALLEL_SINE * r_u.hypot(r_v) * s_u.hypot(s_v) {
        return None;
    }
    let (q_u, q_v) = (right.0.u - left.0.u, right.0.v - left.0.v);
    let along = q_u.mul_add(s_v, -(q_v * s_u)) / denominator;
    Some(Point2::new(
        along.mul_add(r_u, left.0.u),
        along.mul_add(r_v, left.0.v),
    ))
}

/// The foot of the circle center on a line and the two points of the line at
/// the circle radius from the center. A tangent line has all three at the foot.
fn line_circle(line: (Point2, Point2), round: LoopCurve) -> Vec<Point2> {
    let (LoopCurve::Arc { center, radius, .. } | LoopCurve::Circle { center, radius }) = round
    else {
        return Vec::new();
    };
    let length = distance(line.0, line.1);
    if length <= 0.0 {
        return Vec::new();
    }
    let (d_u, d_v) = (
        (line.1.u - line.0.u) / length,
        (line.1.v - line.0.v) / length,
    );
    let along = (center.u - line.0.u).mul_add(d_u, (center.v - line.0.v) * d_v);
    let foot = Point2::new(along.mul_add(d_u, line.0.u), along.mul_add(d_v, line.0.v));
    let half = radius
        .mul_add(radius, -distance(foot, center).powi(2))
        .max(0.0)
        .sqrt();
    vec![
        foot,
        Point2::new(half.mul_add(d_u, foot.u), half.mul_add(d_v, foot.v)),
        Point2::new((-half).mul_add(d_u, foot.u), (-half).mul_add(d_v, foot.v)),
    ]
}

/// The meeting points of two circles. Concentric circles of equal radius
/// meet along their whole length, and the first circle's start stands for it.
fn circle_circle(left: LoopCurve, right: LoopCurve) -> Vec<Point2> {
    let (
        LoopCurve::Arc {
            center: a,
            radius: ra,
            ..
        }
        | LoopCurve::Circle {
            center: a,
            radius: ra,
        },
        LoopCurve::Arc {
            center: b,
            radius: rb,
            ..
        }
        | LoopCurve::Circle {
            center: b,
            radius: rb,
        },
    ) = (left, right)
    else {
        return Vec::new();
    };
    let between = distance(a, b);
    if between <= EPS_PROFILE_WIRE_POINT_MM {
        return if (ra - rb).abs() <= EPS_PROFILE_WIRE_POINT_MM {
            vec![polar(a, ra, 0.0), left.middle(), right.middle()]
        } else {
            Vec::new()
        };
    }
    if between > ra + rb + EPS_PROFILE_WIRE_POINT_MM
        || between < (ra - rb).abs() - EPS_PROFILE_WIRE_POINT_MM
    {
        return Vec::new();
    }
    let along = (ra * ra - rb * rb + between * between) / (2.0 * between);
    let half = ra.mul_add(ra, -(along * along)).max(0.0).sqrt();
    let (e_u, e_v) = ((b.u - a.u) / between, (b.v - a.v) / between);
    let base = Point2::new(along.mul_add(e_u, a.u), along.mul_add(e_v, a.v));
    vec![
        base,
        Point2::new((-half).mul_add(e_v, base.u), half.mul_add(e_u, base.v)),
        Point2::new(half.mul_add(e_v, base.u), (-half).mul_add(e_u, base.v)),
    ]
}

#[cfg(test)]
mod tests;
