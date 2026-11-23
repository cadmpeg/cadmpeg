// SPDX-License-Identifier: Apache-2.0
//! Validate line-fragment ordinals against the participating curve graph.

use super::super::{
    analytic_segment_intersections, arrangement_circle_angles, arrangement_node,
    arrangement_split_parameters, point_distance, profile_use_segment,
    sketch_geometry_parameter_range, sketch_geometry_point, CodecError, DecodeContext, Point2,
    ProfileBoundarySegment, ProfileGeometryWork,
};
use cadmpeg_ir::features::profiles::SketchProfileBoundaryUse;
use cadmpeg_ir::sketches::{SketchEntity, SketchEntityUse, SketchGeometryDefinition};
use std::collections::HashSet;

pub(super) fn validate(
    entities: &[SketchEntity],
    members: &[(SketchEntityUse, [u32; 2])],
    boundary: &[SketchProfileBoundaryUse],
    tolerance: f64,
    work: &ProfileGeometryWork,
    ctx: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    if entities.iter().any(|entity| {
        !matches!(
            entity.geometry.definition(),
            SketchGeometryDefinition::Line { .. }
        )
    }) {
        return Ok(false);
    }
    let Some(pieces) = member_pieces(entities, members, boundary.len(), tolerance, work, ctx)?
    else {
        return Ok(false);
    };
    let Some(edges) = boundary_edges(entities, boundary, ctx)? else {
        return Ok(false);
    };
    Ok(pieces.iter().all(|piece| {
        let ProfileBoundarySegment::Line { start: a, end: b } = piece else {
            return false;
        };
        edges.iter().any(|edge| {
            let ProfileBoundarySegment::Line { start: c, end: d } = edge else {
                return false;
            };
            (point_distance(*a, *c) <= tolerance && point_distance(*b, *d) <= tolerance)
                || (point_distance(*a, *d) <= tolerance && point_distance(*b, *c) <= tolerance)
        })
    }))
}

/// Whether the piece that each member names lies on the boundary: the
/// boundary edges on the piece's carrier cover its whole extent.
pub(super) fn lie_on_boundary(
    entities: &[SketchEntity],
    members: &[(SketchEntityUse, [u32; 2])],
    boundary: &[SketchProfileBoundaryUse],
    tolerance: f64,
    work: &ProfileGeometryWork,
    ctx: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    let Some(pieces) = member_pieces(entities, members, boundary.len(), tolerance, work, ctx)?
    else {
        return Ok(false);
    };
    let Some(edges) = boundary_edges(entities, boundary, ctx)? else {
        return Ok(false);
    };
    Ok(pieces
        .iter()
        .all(|piece| piece_is_covered(piece, &edges, tolerance)))
}

/// Whether the edges on the carrier of `piece` cover its extent.
fn piece_is_covered(
    piece: &ProfileBoundarySegment,
    edges: &[ProfileBoundarySegment],
    tolerance: f64,
) -> bool {
    let (length, spans) = match piece {
        ProfileBoundarySegment::Line { start: a, end: b } => {
            let length = point_distance(*a, *b);
            if length <= tolerance {
                return false;
            }
            let (du, dv) = ((b.u - a.u) / length, (b.v - a.v) / length);
            let along = |point: Point2| (point.u - a.u) * du + (point.v - a.v) * dv;
            let across = |point: Point2| ((point.u - a.u) * dv - (point.v - a.v) * du).abs();
            let spans = edges
                .iter()
                .filter_map(|edge| {
                    let ProfileBoundarySegment::Line { start: c, end: d } = edge else {
                        return None;
                    };
                    (across(*c) <= tolerance && across(*d) <= tolerance).then(|| {
                        let (from, to) = (along(*c), along(*d));
                        [from.min(to), from.max(to)]
                    })
                })
                .collect::<Vec<_>>();
            (length, spans)
        }
        ProfileBoundarySegment::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => {
            let sweep = end_angle - start_angle;
            let length = sweep.abs();
            if length * radius.get() <= tolerance {
                return false;
            }
            let direction = sweep.signum();
            let along =
                |angle: f64| ((angle - start_angle) * direction).rem_euclid(std::f64::consts::TAU);
            let mut spans = Vec::new();
            for edge in edges {
                let ProfileBoundarySegment::Arc {
                    center: other,
                    radius: other_radius,
                    start_angle: from,
                    end_angle: to,
                } = edge
                else {
                    continue;
                };
                if point_distance(*center, *other) > tolerance
                    || (radius.get() - other_radius.get()).abs() > tolerance
                {
                    continue;
                }
                let first = if direction > 0.0 {
                    from.min(*to)
                } else {
                    from.max(*to)
                };
                let lo = along(first);
                let extent = (to - from).abs();
                for shift in [0.0, -std::f64::consts::TAU] {
                    spans.push([lo + shift, lo + extent + shift]);
                }
            }
            return spans_cover(length, spans, tolerance / radius.get());
        }
    };
    spans_cover(length, spans, tolerance)
}

/// Whether the spans, clipped to `[0, length]`, cover it without a gap above
/// `tolerance`.
fn spans_cover(length: f64, spans: Vec<[f64; 2]>, tolerance: f64) -> bool {
    let mut spans: Vec<[f64; 2]> = spans
        .into_iter()
        .map(|[from, to]| [from.max(0.0), to.min(length)])
        .filter(|[from, to]| to - from > tolerance)
        .collect();
    spans.sort_by(|left, right| left[0].total_cmp(&right[0]));
    let mut reach = 0.0_f64;
    for [from, to] in spans {
        if from > reach + tolerance {
            return false;
        }
        reach = reach.max(to);
    }
    reach >= length - tolerance
}

fn boundary_edges(
    entities: &[SketchEntity],
    boundary: &[SketchProfileBoundaryUse],
    ctx: &DecodeContext<'_>,
) -> Result<Option<Vec<ProfileBoundarySegment>>, CodecError> {
    let mut edges = Vec::new();
    for use_ in boundary {
        let Some(entity) = entities.iter().find(|entity| entity.id() == &use_.entity) else {
            return Ok(None);
        };
        let [a, b] = use_.parameter_range.endpoints();
        let edge = match entity.geometry.definition() {
            SketchGeometryDefinition::Line { .. } => {
                let Some(a) = sketch_geometry_point(&entity.geometry, a, ctx)? else {
                    return Ok(None);
                };
                let Some(b) = sketch_geometry_point(&entity.geometry, b, ctx)? else {
                    return Ok(None);
                };
                ProfileBoundarySegment::Line { start: a, end: b }
            }
            SketchGeometryDefinition::Circle { center, radius }
            | SketchGeometryDefinition::Arc { center, radius, .. } => ProfileBoundarySegment::Arc {
                center: center.get(),
                radius: *radius,
                start_angle: a,
                end_angle: b,
            },
            _ => return Ok(None),
        };
        edges.push(edge);
    }
    Ok(Some(edges))
}

/// The angular interval of piece `ordinal` (counted from 1) of the `count`
/// pieces into which the nodes at `angles` divide a circle. The pieces run
/// in ascending angle from the piece that holds angle 0, which ends at the
/// first node; a circle with no node is one piece.
fn circle_piece(angles: &[f64], ordinal: u32, count: u32, tolerance: f64) -> Option<[f64; 2]> {
    use std::f64::consts::TAU;
    let nodes = angles.len().max(1);
    if usize::try_from(count).ok() != Some(nodes) {
        return None;
    }
    let index = usize::try_from(ordinal).ok()?.checked_sub(1)?;
    if index >= nodes {
        return None;
    }
    let Some((&first, &last)) = angles.first().zip(angles.last()) else {
        return Some([0.0, TAU]);
    };
    if angles.len() == 1 {
        return Some([first, first + TAU]);
    }
    if first <= tolerance {
        // A node at angle 0 starts the first piece.
        let end = angles.get(index + 1).copied().unwrap_or(TAU);
        return Some([angles[index], end]);
    }
    if index == 0 {
        return Some([last, first + TAU]);
    }
    Some([angles[index - 1], angles[index]])
}

/// The piece each member names: piece `ordinal` (counted from 1 along the
/// curve's stored direction) of the `count` pieces into which the other
/// curves of `entities` divide the member's line or arc. A circle that no
/// other curve touches is one piece. `None` when an entity is another kind of
/// curve, a member repeats a piece, or an ordinal or count disagrees with the
/// division.
fn member_pieces(
    entities: &[SketchEntity],
    members: &[(SketchEntityUse, [u32; 2])],
    boundary_len: usize,
    tolerance: f64,
    work: &ProfileGeometryWork,
    ctx: &DecodeContext<'_>,
) -> Result<Option<Vec<ProfileBoundarySegment>>, CodecError> {
    if !entities
        .len()
        .checked_mul(entities.len())
        .and_then(|count| count.checked_add(members.len().checked_mul(boundary_len)?))
        .is_some_and(|count| work.arrangement.charge_by(count))
    {
        work.arrangement_skipped.set(true);
        return Ok(None);
    }
    let mut nodes = Vec::new();
    let mut segments = Vec::new();
    for entity in entities {
        let segment = match entity.geometry.definition() {
            SketchGeometryDefinition::Circle { center, radius } => ProfileBoundarySegment::Arc {
                center: center.get(),
                radius: *radius,
                start_angle: 0.0,
                end_angle: std::f64::consts::TAU,
            },
            _ => {
                let Some(segment) = profile_use_segment(entity, false) else {
                    return Ok(None);
                };
                segment
            }
        };
        if let ProfileBoundarySegment::Line { start, end } = &segment {
            arrangement_node(&mut nodes, *start, tolerance);
            arrangement_node(&mut nodes, *end, tolerance);
        } else if let Some([range_start, range_end]) =
            sketch_geometry_parameter_range(&entity.geometry)
        {
            for angle in [range_start, range_end] {
                if let Some(point) = sketch_geometry_point(&entity.geometry, angle, ctx)? {
                    arrangement_node(&mut nodes, point, tolerance);
                }
            }
        }
        segments.push(segment);
    }
    for (index, left) in segments.iter().enumerate() {
        for right in segments.iter().skip(index + 1) {
            if let Some(points) = analytic_segment_intersections(left, right) {
                for point in points.into_iter().flatten() {
                    arrangement_node(&mut nodes, point, tolerance);
                }
            }
        }
    }
    let mut pieces = Vec::new();
    let mut used = HashSet::new();
    for (member, [ordinal, count]) in members {
        if !used.insert((&member.entity, ordinal)) {
            return Ok(None);
        }
        let Some(entity) = entities.iter().find(|entity| entity.id() == &member.entity) else {
            return Ok(None);
        };
        if let SketchGeometryDefinition::Circle { center, radius } = entity.geometry.definition() {
            let angles = arrangement_circle_angles(&nodes, center.get(), *radius, tolerance);
            let Some([start_angle, end_angle]) =
                circle_piece(&angles, *ordinal, *count, tolerance / radius.get())
            else {
                return Ok(None);
            };
            pieces.push(ProfileBoundarySegment::Arc {
                center: center.get(),
                radius: *radius,
                start_angle,
                end_angle,
            });
            continue;
        }
        let Some(range) = sketch_geometry_parameter_range(&entity.geometry) else {
            return Ok(None);
        };
        let Some(parameters) =
            arrangement_split_parameters(&entity.geometry, range, &nodes, tolerance)?
        else {
            return Ok(None);
        };
        if u32::try_from(parameters.len() - 1).ok() != Some(*count) {
            return Ok(None);
        }
        let Some(index) = usize::try_from(*ordinal)
            .ok()
            .and_then(|i| i.checked_sub(1))
        else {
            return Ok(None);
        };
        let Some(span) = parameters.get(index..).and_then(|tail| tail.get(..2)) else {
            return Ok(None);
        };
        if let SketchGeometryDefinition::Arc { center, radius, .. } = entity.geometry.definition() {
            pieces.push(ProfileBoundarySegment::Arc {
                center: center.get(),
                radius: *radius,
                start_angle: span[0],
                end_angle: span[1],
            });
        } else {
            let Some(a) = sketch_geometry_point(&entity.geometry, span[0], ctx)? else {
                return Ok(None);
            };
            let Some(b) = sketch_geometry_point(&entity.geometry, span[1], ctx)? else {
                return Ok(None);
            };
            pieces.push(ProfileBoundarySegment::Line { start: a, end: b });
        }
    }
    Ok(Some(pieces))
}

#[cfg(test)]
mod tests;
