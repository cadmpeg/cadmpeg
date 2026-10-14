// SPDX-License-Identifier: Apache-2.0
//! Nominal thread diameter and abstaining physical-cylinder selection.

use crate::native::features::holes::FeatureSymbolicThread;
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, FeatureOperation};
use cadmpeg_ir::geometry::SolvedSurfaceGeometry;
use cadmpeg_ir::ids::{FaceId, ShellId, VertexId};
use cadmpeg_ir::index::ModelIndex;
use cadmpeg_ir::scalar::PositiveLength;
use cadmpeg_ir::topology::{Face, LoopBoundary};
use std::collections::BTreeMap;

const EPS_AXIS_DISTANCE_MM: f64 = 1.0e-6;
const EPS_NOMINAL_RADIUS_MM: f64 = 1.0e-7;
const EPS_AXIS_DIRECTION: f64 = 1.0e-10;

struct PhysicalCylinder {
    shell: ShellId,
    axis: [f64; 3],
    foot: [f64; 3],
    radius: f64,
    interval: [f64; 2],
    faces: Vec<FaceId>,
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a.into_iter().zip(b).map(|(a, b)| a * b).sum()
}

fn subtract(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|i| a[i] - b[i])
}

fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    let difference = subtract(a, b);
    dot(difference, difference).sqrt()
}

fn positive_decimal(value: &str) -> Option<f64> {
    if value.is_empty()
        || value.starts_with('.')
        || value.ends_with('.')
        || value.bytes().filter(|b| *b == b'.').count() > 1
        || !value.bytes().all(|b| b.is_ascii_digit() || b == b'.')
    {
        return None;
    }
    let value: f64 = value.parse().ok()?;
    (value.is_finite() && value > 0.0).then_some(value)
}

fn nominal_diameter(designation: &str) -> Option<PositiveLength> {
    if let Some(metric) = designation.strip_prefix('M') {
        let (diameter, pitch) = metric.split_once('_')?;
        positive_decimal(
            pitch
                .strip_prefix("x_")
                .or_else(|| pitch.strip_prefix("X_"))?,
        )?;
        return PositiveLength::new(positive_decimal(diameter)?);
    }
    let (fraction, thread) = designation.split_once('-')?;
    let (threads, family) = thread.split_once('_')?;
    if !matches!(family, "UNC" | "UNF")
        || !threads.bytes().all(|b| b.is_ascii_digit())
        || threads.parse::<u32>().ok()? == 0
    {
        return None;
    }
    let (numerator, denominator) = fraction.split_once('/')?;
    if !numerator.bytes().all(|b| b.is_ascii_digit())
        || !denominator.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    PositiveLength::new(positive_decimal(numerator)? / positive_decimal(denominator)? * 25.4)
}

fn vertex_parameter(
    ctx: &DecodeContext<'_>,
    index: &ModelIndex<'_>,
    vertex: &VertexId,
    axis: [f64; 3],
) -> Result<Option<f64>, CodecError> {
    let Some(vertex) = index.vertices(vertex.as_str(), ctx)? else {
        return Ok(None);
    };
    let Some(point) = index.points(vertex.point.as_str(), ctx)? else {
        return Ok(None);
    };
    let point = point.position().get();
    let value = dot([point.x, point.y, point.z], axis);
    Ok(value.is_finite().then_some(value))
}

fn face_interval(
    ctx: &DecodeContext<'_>,
    index: &ModelIndex<'_>,
    face: &Face,
    axis: [f64; 3],
) -> Result<Option<[f64; 2]>, CodecError> {
    let mut interval: Option<[f64; 2]> = None;
    let mut include = |vertex: &VertexId| -> Result<bool, CodecError> {
        let Some(t) = vertex_parameter(ctx, index, vertex, axis)? else {
            return Ok(false);
        };
        interval = Some(interval.map_or([t, t], |[lo, hi]| [lo.min(t), hi.max(t)]));
        Ok(true)
    };
    for identity in &face.loops {
        let Some(boundary) = index.loops(identity.as_str(), ctx)? else {
            return Ok(None);
        };
        if boundary.face != face.id {
            return Ok(None);
        }
        match &boundary.boundary {
            LoopBoundary::Vertex { vertex, .. } => {
                if !include(vertex)? {
                    return Ok(None);
                }
            }
            LoopBoundary::Ring(ring) => {
                for anchored in ring.vertex_uses() {
                    if !include(&anchored.vertex)? {
                        return Ok(None);
                    }
                }
                for identity in ring.coedges() {
                    let Some(coedge) = index.coedges(identity.as_str(), ctx)? else {
                        return Ok(None);
                    };
                    if coedge.owner_loop != boundary.id {
                        return Ok(None);
                    }
                    let Some(edge) = index.edges(coedge.edge.as_str(), ctx)? else {
                        return Ok(None);
                    };
                    if !include(&edge.start)? || !include(&edge.end)? {
                        return Ok(None);
                    }
                }
            }
        }
    }
    Ok(interval)
}

fn cylinder_groups(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
) -> Result<Vec<PhysicalCylinder>, CodecError> {
    let index = ModelIndex::new_model_only(ir, ctx)?;
    let (owners, _storage) = ctx.collect_scoped_btree_groups(
        ir.model
            .shells
            .iter()
            .flat_map(|shell| shell.faces().iter().map(move |face| (face.as_str(), shell))),
        "NX symbolic thread face ownership",
    )?;
    let mut groups: Vec<PhysicalCylinder> = Vec::new();
    for face in &ir.model.faces {
        ctx.charge_work(1, "NX symbolic thread cylinder scan")?;
        let Some(surface) = index.surfaces(face.surface.as_str(), ctx)? else {
            continue;
        };
        let Some(SolvedSurfaceGeometry::Cylinder(cylinder)) = surface.geometry.solved() else {
            continue;
        };
        let Some([shell]) = owners.get(face.id.as_str()).map(Vec::as_slice) else {
            continue;
        };
        if shell.id != face.shell {
            continue;
        }
        let Some(region) = index.regions(shell.region.as_str(), ctx)? else {
            continue;
        };
        ctx.charge_work(
            u64_from_index(region.shells.len()),
            "NX symbolic thread region ownership",
        )?;
        if region.shells.iter().filter(|id| **id == shell.id).count() != 1 {
            continue;
        }
        let Some(body) = index.bodies(region.body.as_str(), ctx)? else {
            continue;
        };
        if body.transform.is_some() {
            continue;
        }
        ctx.charge_work(
            u64_from_index(body.regions.len()),
            "NX symbolic thread body ownership",
        )?;
        if body.regions.iter().filter(|id| **id == region.id).count() != 1 {
            continue;
        }
        let direction = cylinder.frame().axis().as_raw();
        let mut axis = [direction.x, direction.y, direction.z];
        if axis
            .iter()
            .find(|v| v.abs() > EPS_AXIS_DIRECTION)
            .is_some_and(|v| *v < 0.0)
        {
            axis = axis.map(|v| -v);
        }
        let origin = cylinder.origin().get();
        let origin = [origin.x, origin.y, origin.z];
        let foot = subtract(origin, axis.map(|v| dot(origin, axis) * v));
        let Some(interval) = face_interval(ctx, &index, face, axis)? else {
            continue;
        };
        if !foot.iter().all(|v| v.is_finite()) {
            continue;
        }
        let radius = cylinder.radius().get();
        ctx.charge_work(
            u64_from_index(groups.len()),
            "NX symbolic thread seam groups",
        )?;
        let mut matches = groups.iter_mut().filter(|group| {
            group.shell == shell.id
                && distance(group.axis, axis) <= EPS_AXIS_DIRECTION
                && distance(group.foot, foot) <= EPS_AXIS_DISTANCE_MM
                && (group.radius - radius).abs() <= EPS_NOMINAL_RADIUS_MM
                && group
                    .interval
                    .iter()
                    .zip(interval)
                    .all(|(a, b)| (a - b).abs() <= EPS_AXIS_DISTANCE_MM)
        });
        let matched = matches.next();
        if matches.next().is_some() {
            return Ok(Vec::new());
        }
        if let Some(group) = matched {
            ctx.reserve_vec(&mut group.faces, 1, "NX symbolic thread seam faces")?;
            group.faces.push(
                face.id
                    .try_clone_for_decode(ctx, "NX symbolic thread face identity")?,
            );
        } else {
            let mut faces = ctx.collection_vec(1, "NX symbolic thread physical cylinder face")?;
            faces.push(
                face.id
                    .try_clone_for_decode(ctx, "NX symbolic thread face identity")?,
            );
            ctx.reserve_vec(&mut groups, 1, "NX symbolic thread physical cylinders")?;
            groups.push(PhysicalCylinder {
                shell: shell
                    .id
                    .try_clone_for_decode(ctx, "NX symbolic thread shell identity")?,
                axis,
                foot,
                radius,
                interval,
                faces,
            });
        }
    }
    Ok(groups)
}

pub(super) fn definitions<'a>(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    threads: &'a [FeatureSymbolicThread],
) -> Result<BTreeMap<&'a str, FeatureDefinition>, CodecError> {
    let mut result = BTreeMap::new();
    ctx.charge_work(
        u64_from_index(threads.len()),
        "NX symbolic thread construction scan",
    )?;
    if !threads.iter().any(|thread| thread.construction.is_some()) {
        return Ok(result);
    }
    let groups = cylinder_groups(ctx, ir)?;
    for thread in threads {
        let Some(construction) = &thread.construction else {
            continue;
        };
        let Some(designation) = thread.designation() else {
            continue;
        };
        ctx.charge_work(
            u64_from_index(designation.len() + threads.len()),
            "NX symbolic thread designation and unique owner",
        )?;
        if threads
            .iter()
            .filter(|other| other.operation_label == thread.operation_label)
            .count()
            != 1
        {
            continue;
        }
        let Some(diameter) = nominal_diameter(designation) else {
            continue;
        };
        let point = construction.point_mm();
        ctx.charge_work(
            u64_from_index(groups.len()),
            "NX symbolic thread cylinder candidates",
        )?;
        let mut candidates = groups.iter().filter(|group| {
            let t = dot(point, group.axis);
            let foot = subtract(point, group.axis.map(|v| t * v));
            distance(foot, group.foot) <= EPS_AXIS_DISTANCE_MM
                && t >= group.interval[0] - EPS_AXIS_DISTANCE_MM
                && t <= group.interval[1] + EPS_AXIS_DISTANCE_MM
                && group.radius <= diameter.get() / 2.0 + EPS_NOMINAL_RADIUS_MM
        });
        let first = candidates.next();
        let face = if let (Some(group), None) = (first, candidates.next()) {
            let mut faces =
                ctx.collection_vec(group.faces.len(), "NX symbolic thread selected faces")?;
            for face in &group.faces {
                faces.push(face.try_clone_for_decode(ctx, "NX symbolic thread selected face")?);
            }
            FaceSelection::Resolved {
                faces,
                native: ctx
                    .copy_retained_text(&thread.id, "NX symbolic thread selection witness")?,
            }
        } else {
            FaceSelection::Unresolved
        };
        ctx.insert_btree_map(
            &mut result,
            thread.operation_label.as_str(),
            FeatureDefinition::Operation(FeatureOperation::CosmeticThread {
                face,
                diameter: Some(diameter),
                extent: None,
            }),
            "NX symbolic thread definitions",
        )?;
    }
    Ok(result)
}

#[cfg(test)]
mod tests;
