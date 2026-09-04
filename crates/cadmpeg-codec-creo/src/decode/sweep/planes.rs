// SPDX-License-Identifier: Apache-2.0
//! Feature plane equations and generated cylinder and cap extents.

use super::super::analytic::{
    canonical_plane, dot, placed_planes, reconciled_model_plane, PlaneEquation,
};
use super::super::holes::blind_extrude_side;
use super::super::sketch::normalized;
use crate::container::ContainerScan;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{ExtrudeExtent, ExtrudeSide, Length, LinearTermination};
use cadmpeg_ir::geometry::SurfaceGeometry;
use cadmpeg_ir::ids::SurfaceId;
use std::collections::{BTreeMap, BTreeSet};

const EPS_CYLINDER_CARRIER: f64 = 1.0e-9;

fn feature_local_plane(scan: &ContainerScan, surface_id: u32) -> Result<Option<PlaneEquation>, ()> {
    if crate::surface::unique_surface_row(&scan.surfaces.rows, surface_id).is_none() {
        return Err(());
    }
    let outlines = scan
        .planes
        .outlines
        .iter()
        .filter(|plane| plane.surface_id == surface_id)
        .collect::<Vec<_>>();
    match outlines.as_slice() {
        [plane] => Ok(Some(PlaneEquation {
            origin: plane.origin,
            normal: plane.normal,
        })),
        [] => {
            let frames = scan
                .planes
                .local_systems
                .iter()
                .filter(|frame| frame.surface_id == surface_id)
                .collect::<Vec<_>>();
            match frames.as_slice() {
                [] => Ok(None),
                [frame] => Ok(frame
                    .origin
                    .zip(frame.normal)
                    .map(|(origin, normal)| PlaneEquation { origin, normal })),
                _ => Err(()),
            }
        }
        _ => Err(()),
    }
}

const EPS_GEOMETRY_AGREEMENT: f64 = 1.0e-9;
const EPS_AXIS_ALIGNMENT: f64 = 1.0e-10;
const EPS_SIGNED_LENGTH: f64 = 1.0e-9;

pub(in super::super) fn feature_plane_equations(
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
) -> Option<Vec<([f64; 3], [f64; 3])>> {
    let ids = scan
        .surfaces
        .rows
        .iter()
        .filter(|row| {
            row.feature_id == feature_id && row.kind == crate::surface::SurfaceKind::Plane
        })
        .map(|row| row.id)
        .collect::<BTreeSet<_>>();
    let mut local_planes = BTreeMap::new();
    for id in &ids {
        match feature_local_plane(scan, *id) {
            Ok(Some(plane)) => {
                local_planes.insert(*id, plane);
            }
            Ok(None) => {}
            Err(()) => return None,
        }
    }
    ids.into_iter()
        .map(|id| {
            let plane = reconciled_model_plane(&local_planes, ir, id)?;
            Some((plane.origin, plane.normal))
        })
        .collect()
}

pub(in super::super) type FeatureOutlinePlane = (u32, [f64; 3], [f64; 3]);

/// Resolve one uniquely identified plane row to one unambiguous placed plane
/// equation. The equation may be carried by one outline and one positional
/// frame; both carriers are valid only when they agree on origin and normal.
pub(in super::super) fn feature_outline_plane(
    scan: &ContainerScan,
    feature_id: u32,
    surface_id: u32,
) -> Option<FeatureOutlinePlane> {
    let row = crate::surface::unique_surface_row(&scan.surfaces.rows, surface_id)?;
    (row.feature_id == feature_id && row.kind == crate::surface::SurfaceKind::Plane)
        .then_some(())?;
    let outlines = scan
        .planes
        .outlines
        .iter()
        .filter(|plane| plane.surface_id == surface_id);
    let outlines = outlines.cloned().collect::<Vec<_>>();
    let positional_frames = scan
        .planes
        .positional_frames
        .iter()
        .filter(|plane| plane.surface_id == surface_id)
        .cloned()
        .collect::<Vec<_>>();
    let agrees = |left: &crate::surface::OutlinePlane, right: &crate::surface::OutlinePlane| {
        left.origin
            .into_iter()
            .zip(right.origin)
            .all(|(left, right)| {
                (left - right).abs()
                    <= EPS_GEOMETRY_AGREEMENT * left.abs().max(right.abs()).max(1.0)
            })
            && left
                .normal
                .into_iter()
                .zip(right.normal)
                .all(|(left, right)| {
                    (left - right).abs()
                        <= EPS_GEOMETRY_AGREEMENT * left.abs().max(right.abs()).max(1.0)
                })
    };
    let plane = match (outlines.as_slice(), positional_frames.as_slice()) {
        ([], []) => return None,
        ([plane], []) | ([], [plane]) => plane,
        ([outline], [positional]) if agrees(outline, positional) => outline,
        _ => return None,
    };
    Some((surface_id, plane.origin, plane.normal))
}

/// Collect every same-feature plane row only when all rows have complete,
/// unambiguous placed equations. Partial collections cannot establish ordered
/// caps.
pub(in super::super) fn feature_outline_planes(
    scan: &ContainerScan,
    feature_id: u32,
) -> Option<Vec<FeatureOutlinePlane>> {
    scan.surfaces
        .rows
        .iter()
        .filter(|row| {
            row.feature_id == feature_id && row.kind == crate::surface::SurfaceKind::Plane
        })
        .map(|row| row.id)
        .map(|id| feature_outline_plane(scan, feature_id, id))
        .collect()
}

pub(in super::super) fn generated_arc_cylinder_extent(
    scan: &ContainerScan,
    ir: &CadIr,
    definition: &crate::feature::FeatureDefinition,
    transform: &crate::placement::FeatureSectionTransform,
) -> Option<(ExtrudeExtent, [f64; 3])> {
    let feature_id = definition.owner_feature_id?;
    definition.segments.as_ref()?.is_complete().then_some(())?;
    let mut surface_ids = BTreeSet::new();
    for (_, entry) in scan
        .features
        .entity_tables
        .iter()
        .filter(|table| table.feature_id == Some(feature_id))
        .flat_map(|table| table.entries.iter().map(move |entry| (table, entry)))
        .filter(|(table, entry)| {
            entry.class_id == 200 && table.surface_ids.contains(&entry.entity_id)
        })
    {
        let Some(source_id) = entry.source_entity_id else {
            continue;
        };
        let Some(segment) = definition.segments.as_ref()?.segment(source_id) else {
            continue;
        };
        if segment.kind != crate::feature::FeatureSegmentKind::Arc {
            continue;
        }
        let Some(row) = crate::surface::unique_surface_row(&scan.surfaces.rows, entry.entity_id)
            .filter(|row| {
                row.feature_id == feature_id && row.kind == crate::surface::SurfaceKind::Cylinder
            })
        else {
            continue;
        };
        surface_ids.insert(row.id).then_some(())?;
    }
    let frame_records = unique_available_positional_cylinder_frame_records(
        &surface_ids,
        &scan.surfaces.parameters,
    )?;
    (!frame_records.is_empty()).then_some(())?;
    frame_records
        .iter()
        .all(|(surface_id, frame)| cylinder_frame_agrees_with_model(ir, *surface_id, frame))
        .then_some(())?;
    let frames = frame_records
        .into_iter()
        .map(|(_, frame)| frame)
        .collect::<Vec<_>>();
    agreed_generated_cylinder_extent(transform, &frames)
}

fn cylinder_frame_agrees_with_model(
    ir: &CadIr,
    surface_id: u32,
    frame: &crate::surface::PositionalCylinderFrame,
) -> bool {
    let model_id = SurfaceId(format!("creo:visibgeom:surface#{surface_id}"));
    let model_surfaces = ir
        .model
        .surfaces
        .iter()
        .filter(|surface| surface.id == model_id)
        .collect::<Vec<_>>();
    let surface = match model_surfaces.as_slice() {
        [] => return true,
        [surface] => surface,
        _ => return false,
    };
    let SurfaceGeometry::Cylinder {
        origin,
        axis,
        ref_direction,
        radius,
    } = &surface.geometry
    else {
        return matches!(surface.geometry, SurfaceGeometry::Unknown { .. });
    };
    let (Some(frame_axis), Some(model_axis), Some(frame_ref), Some(model_ref)) = (
        normalized(frame.axis),
        normalized([axis.x, axis.y, axis.z]),
        normalized(frame.ref_direction),
        normalized([ref_direction.x, ref_direction.y, ref_direction.z]),
    ) else {
        return false;
    };
    if !radius.is_finite() || *radius <= 0.0 {
        return false;
    }
    let close = |left: f64, right: f64| {
        (left - right).abs() <= EPS_CYLINDER_CARRIER * left.abs().max(right.abs()).max(1.0)
    };
    if !frame_axis
        .into_iter()
        .zip(model_axis)
        .all(|(left, right)| close(left, right))
        || !frame_ref
            .into_iter()
            .zip(model_ref)
            .all(|(left, right)| close(left, right))
        || !close(frame.radius, *radius)
    {
        return false;
    }
    let model_origin = [origin.x, origin.y, origin.z];
    let relative = std::array::from_fn(|index| model_origin[index] - frame.origin[index]);
    let axial = dot(relative, frame_axis);
    let radial = std::array::from_fn(|index| relative[index] - axial * frame_axis[index]);
    let scale = frame
        .origin
        .into_iter()
        .chain(model_origin)
        .map(f64::abs)
        .fold(1.0, f64::max);
    dot(radial, radial).sqrt() <= EPS_CYLINDER_CARRIER * scale
}

pub(in super::super) fn ordered_parallel_cap_extent(
    start: PlaneEquation,
    end: PlaneEquation,
) -> Option<(ExtrudeExtent, [f64; 3])> {
    let start = canonical_plane(start)?;
    let end = canonical_plane(end)?;
    start
        .normal
        .into_iter()
        .zip(end.normal)
        .all(|(left, right)| (left - right).abs() <= EPS_AXIS_ALIGNMENT)
        .then_some(())?;
    let signed_length = dot(
        std::array::from_fn(|axis| end.origin[axis] - start.origin[axis]),
        start.normal,
    );
    let scale = start
        .origin
        .into_iter()
        .chain(end.origin)
        .map(f64::abs)
        .fold(1.0, f64::max);
    (signed_length.abs() > EPS_SIGNED_LENGTH * scale).then_some(())?;
    Some((
        ExtrudeExtent::OneSided {
            side: blind_extrude_side(signed_length.abs()),
        },
        start
            .normal
            .map(|component| component * signed_length.signum()),
    ))
}

pub(in super::super) fn generated_cap_plane_extent(
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
) -> Option<(ExtrudeExtent, [f64; 3])> {
    let tables = scan
        .features
        .entity_tables
        .iter()
        .filter(|table| table.feature_id == Some(feature_id) && table.table_class_id == 29)
        .collect::<Vec<_>>();
    let [table] = tables.as_slice() else {
        return None;
    };
    table
        .entries
        .iter()
        .map(|entry| entry.entity_id)
        .eq(table.entry_ids.iter().copied())
        .then_some(())?;
    let mut start_id = None;
    let mut end_id = None;
    let mut side_count = 0_usize;
    for entry in &table.entries {
        match (entry.class_id, entry.source_entity_id) {
            (204, None) if start_id.replace(entry.entity_id).is_none() => {}
            (203, None) if end_id.replace(entry.entity_id).is_none() => {}
            (200, Some(_)) => side_count += 1,
            _ => return None,
        }
    }
    (side_count > 0
        && table.surface_ids.contains(&start_id?)
        && table.surface_ids.contains(&end_id?))
    .then_some(())?;
    let local_planes = placed_planes(scan);
    let plane = |surface_id: u32| {
        let row = crate::surface::unique_surface_row(&scan.surfaces.rows, surface_id)?;
        (row.feature_id == feature_id && row.kind == crate::surface::SurfaceKind::Plane)
            .then_some(())?;
        reconciled_model_plane(&local_planes, ir, surface_id)
    };
    ordered_parallel_cap_extent(plane(start_id?)?, plane(end_id?)?)
}

pub(in super::super) fn unique_available_positional_cylinder_frame_records(
    surface_ids: &BTreeSet<u32>,
    parameters: &[crate::surface::SurfaceParameterRecord],
) -> Option<Vec<(u32, crate::surface::PositionalCylinderFrame)>> {
    let mut frames = Vec::new();
    for surface_id in surface_ids {
        let mut matching = parameters
            .iter()
            .filter(|record| record.surface_id == *surface_id);
        let first = matching.next();
        if matching.next().is_some() {
            return None;
        }
        if let Some(frame) = first.and_then(|record| record.positional_cylinder_frame) {
            frames.push((*surface_id, frame));
        }
    }
    Some(frames)
}

pub(in super::super) fn agreed_generated_cylinder_extent(
    transform: &crate::placement::FeatureSectionTransform,
    frames: &[crate::surface::PositionalCylinderFrame],
) -> Option<(ExtrudeExtent, [f64; 3])> {
    let normal = normalized(transform.normal)?;
    let first = *frames.first()?;
    let length = first.length.filter(|length| *length > 0.0)?;
    let direction = normalized(first.axis)?;
    let close = |left: f64, right: f64| {
        (left - right).abs() <= EPS_GEOMETRY_AGREEMENT * left.abs().max(right.abs()).max(1.0)
    };
    frames
        .iter()
        .all(|frame| {
            frame
                .length
                .is_some_and(|candidate| close(candidate, length))
                && normalized(frame.axis).is_some_and(|axis| {
                    axis.iter()
                        .zip(direction)
                        .all(|(left, right)| close(*left, right))
                })
                && close(
                    dot(
                        std::array::from_fn(|index| frame.origin[index] - transform.origin[index]),
                        normal,
                    ),
                    0.0,
                )
        })
        .then_some(())?;
    close(dot(direction, normal).abs(), 1.0).then_some(())?;
    Some((
        ExtrudeExtent::OneSided {
            side: ExtrudeSide {
                termination: LinearTermination::Blind {
                    length: Length(length),
                },
                draft: None,
            },
        },
        direction,
    ))
}

#[cfg(test)]
mod tests;
