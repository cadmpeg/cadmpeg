// SPDX-License-Identifier: Apache-2.0
//! Validate line-fragment ordinals against the participating curve graph.

use super::super::{
    analytic_segment_intersections, arrangement_node, arrangement_split_parameters, point_distance,
    sketch_geometry_point, CodecError, DecodeContext, ProfileBoundarySegment, ProfileGeometryWork,
};
use cadmpeg_ir::features::SketchProfileBoundaryUse;
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
    if !entities
        .len()
        .checked_mul(entities.len())
        .and_then(|count| count.checked_add(members.len().checked_mul(boundary.len())?))
        .is_some_and(|count| work.arrangement.charge_by(count))
    {
        work.arrangement_skipped.set(true);
        return Ok(false);
    }
    let mut nodes = Vec::new();
    let mut segments = Vec::new();
    for entity in entities {
        let SketchGeometryDefinition::Line { start, end } = entity.geometry.definition() else {
            return Ok(false);
        };
        arrangement_node(&mut nodes, start.get(), tolerance);
        arrangement_node(&mut nodes, end.get(), tolerance);
        segments.push(ProfileBoundarySegment::Line {
            start: start.get(),
            end: end.get(),
        });
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
    let mut edges = Vec::new();
    for use_ in boundary {
        let Some(entity) = entities.iter().find(|entity| entity.id() == &use_.entity) else {
            return Ok(false);
        };
        let [a, b] = use_.parameter_range.endpoints();
        let Some(a) = sketch_geometry_point(&entity.geometry, a, ctx)? else {
            return Ok(false);
        };
        let Some(b) = sketch_geometry_point(&entity.geometry, b, ctx)? else {
            return Ok(false);
        };
        edges.push([a, b]);
    }
    let mut used = HashSet::new();
    for (member, [ordinal, count]) in members {
        if !used.insert((&member.entity, ordinal)) {
            return Ok(false);
        }
        let Some(entity) = entities.iter().find(|entity| entity.id() == &member.entity) else {
            return Ok(false);
        };
        let Some(parameters) =
            arrangement_split_parameters(&entity.geometry, [0.0, 1.0], &nodes, tolerance)?
        else {
            return Ok(false);
        };
        if u32::try_from(parameters.len() - 1).ok() != Some(*count) {
            return Ok(false);
        }
        let Some(index) = usize::try_from(*ordinal)
            .ok()
            .and_then(|i| i.checked_sub(1))
        else {
            return Ok(false);
        };
        let Some(range) = parameters.get(index..).and_then(|tail| tail.get(..2)) else {
            return Ok(false);
        };
        let Some(a) = sketch_geometry_point(&entity.geometry, range[0], ctx)? else {
            return Ok(false);
        };
        let Some(b) = sketch_geometry_point(&entity.geometry, range[1], ctx)? else {
            return Ok(false);
        };
        if !edges.iter().any(|[c, d]| {
            (point_distance(a, *c) <= tolerance && point_distance(b, *d) <= tolerance)
                || (point_distance(a, *d) <= tolerance && point_distance(b, *c) <= tolerance)
        }) {
            return Ok(false);
        }
    }
    Ok(true)
}
