// SPDX-License-Identifier: Apache-2.0
//! Feature plane equations and generated cylinder and cap extents.

use super::super::holes::sweep::blind_extrude_side;
use super::super::uniqueness::exactly_one;
use crate::container::ContainerScan;
use crate::decode::analytic::equations::PlaneEquation;
use crate::decode::analytic::planes::{canonical_plane, placed_planes, reconciled_model_plane};
use crate::surface::SurfaceParameterRecord;
use crate::vecmath::dot;
use crate::vecmath::unit_length;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{ExtrudeExtent, ExtrudeSide, LinearTermination};
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use std::collections::{BTreeMap, BTreeSet};

const EPS_CYLINDER_CARRIER: f64 = 1.0e-9;

fn feature_local_plane(scan: &ContainerScan, surface_id: u32) -> Result<Option<PlaneEquation>, ()> {
    if crate::surface::unique_surface_row(&scan.surfaces.rows, surface_id).is_none() {
        return Err(());
    }
    let mut outlines = scan
        .planes
        .outlines
        .iter()
        .filter(|plane| plane.surface_id == surface_id);
    match (outlines.next(), outlines.next()) {
        (Some(plane), None) => Ok(Some(PlaneEquation {
            origin: plane.origin,
            normal: plane.normal(),
        })),
        (None, None) => {
            let mut frames = scan
                .planes
                .local_systems
                .iter()
                .filter(|frame| frame.surface_id == surface_id);
            match (frames.next(), frames.next()) {
                (None, None) => Ok(None),
                (Some(frame), None) => {
                    let frame = frame.frame();
                    Ok(frame
                        .origin
                        .zip(frame.normal())
                        .map(|(origin, normal)| PlaneEquation { origin, normal }))
                }
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
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &CadIr,
    source_carriers: &crate::decode::source_carriers::SourceUnitCarriers,
    feature_id: u32,
) -> Result<Option<Vec<crate::decode::analytic::equations::PlaneEquation>>, CodecError> {
    let mut ids = BTreeSet::new();
    for row in scan.surfaces.rows.iter().filter(|row| {
        row.feature_id == feature_id && row.kind == crate::surface::SurfaceKind::Plane
    }) {
        ctx.insert_btree_set(&mut ids, row.id, "creo feature plane ID nodes")?;
    }
    let mut local_planes = BTreeMap::new();
    for id in &ids {
        match feature_local_plane(scan, *id) {
            Ok(Some(plane)) => {
                ctx.insert_btree_map(
                    &mut local_planes,
                    *id,
                    plane,
                    "creo feature local plane nodes",
                )?;
            }
            Ok(None) => {}
            Err(()) => return Ok(None),
        }
    }
    let mut equations = Vec::new();
    for id in ids {
        let Some(plane) =
            reconciled_model_plane(ctx, &local_planes, ir, source_carriers, id)?
        else {
            return Ok(None);
        };
        ctx.reserve_vec(&mut equations, 1, "creo feature plane equations")?;
        equations.push(plane);
    }
    Ok(Some(equations))
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
    let mut outlines = scan
        .planes
        .outlines
        .iter()
        .filter(|plane| plane.surface_id == surface_id);
    let outline = outlines.next();
    if outlines.next().is_some() {
        return None;
    }
    let mut positional_frames = scan
        .planes
        .positional_frames
        .iter()
        .filter(|plane| plane.surface_id == surface_id);
    let positional = positional_frames.next();
    if positional_frames.next().is_some() {
        return None;
    }
    let agrees = |left: &crate::surface::OutlinePlane, right: &crate::surface::OutlinePlane| {
        left.origin
            .into_iter()
            .zip(right.origin)
            .all(|(left, right)| {
                (left - right).abs()
                    <= EPS_GEOMETRY_AGREEMENT * left.abs().max(right.abs()).max(1.0)
            })
            && left
                .normal()
                .into_iter()
                .zip(right.normal())
                .all(|(left, right)| {
                    (left - right).abs()
                        <= EPS_GEOMETRY_AGREEMENT * left.abs().max(right.abs()).max(1.0)
                })
    };
    let plane = match (outline, positional) {
        (None, None) => return None,
        (Some(plane), None) | (None, Some(plane)) => plane,
        (Some(outline), Some(positional)) if agrees(outline, positional) => outline,
        _ => return None,
    };
    Some((surface_id, plane.origin, plane.normal()))
}

/// Collect every same-feature plane row only when all rows have complete,
/// unambiguous placed equations. Partial collections cannot establish ordered
/// caps.
pub(in super::super) fn feature_outline_planes(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    feature_id: u32,
) -> Result<Option<Vec<FeatureOutlinePlane>>, CodecError> {
    let mut planes = Vec::new();
    for row in scan.surfaces.rows.iter().filter(|row| {
        row.feature_id == feature_id && row.kind == crate::surface::SurfaceKind::Plane
    }) {
        let Some(plane) = feature_outline_plane(scan, feature_id, row.id) else {
            return Ok(None);
        };
        ctx.reserve_vec(&mut planes, 1, "creo feature outline planes")?;
        planes.push(plane);
    }
    Ok(Some(planes))
}

pub(in super::super) fn generated_arc_cylinder_extent(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &CadIr,
    source_carriers: &crate::decode::source_carriers::SourceUnitCarriers,
    definition: &crate::feature::definitions::FeatureDefinition,
    transform: &crate::placement::FeatureSectionTransform,
) -> Result<Option<(ExtrudeExtent, [f64; 3])>, CodecError> {
    let Some(feature_id) = definition.identity.owner_feature_id() else {
        return Ok(None);
    };
    let Some(segments) = definition
        .segments
        .as_ref()
        .filter(|segments| segments.is_complete())
    else {
        return Ok(None);
    };
    let mut surface_ids = BTreeSet::new();
    for (_, entry) in scan
        .features
        .entity_tables
        .iter()
        .filter(|table| table.feature_id == feature_id)
        .flat_map(|table| table.entries.iter().map(move |entry| (table, entry)))
        .filter(|(table, entry)| table.contains_surface_id(entry.entity_id))
    {
        let Some(source_id) = entry.source_entity_id() else {
            continue;
        };
        let Some(segment) = segments.segment(source_id) else {
            continue;
        };
        if !matches!(
            segment.kind,
            crate::feature::definitions::FeatureSegmentKind::Arc(_)
        ) {
            continue;
        }
        let Some(row) = crate::surface::unique_surface_row(&scan.surfaces.rows, entry.entity_id)
            .filter(|row| {
                row.feature_id == feature_id && row.kind == crate::surface::SurfaceKind::Cylinder
            })
        else {
            continue;
        };
        if surface_ids.contains(&row.id) {
            return Ok(None);
        }
        ctx.insert_btree_set(
            &mut surface_ids,
            row.id,
            "creo generated arc cylinder ID nodes",
        )?;
    }
    let Some(frame_records) = unique_available_positional_cylinder_frame_records(
        ctx,
        &surface_ids,
        &scan.surfaces.parameters,
    )?
    else {
        return Ok(None);
    };
    if frame_records.is_empty()
        || !frame_records.iter().all(|(surface_id, frame)| {
            cylinder_frame_agrees_with_model(ir, *surface_id, frame, source_carriers)
        })
    {
        return Ok(None);
    }
    Ok(agreed_generated_cylinder_extent(
        transform,
        frame_records.iter().map(|(_, frame)| frame),
    ))
}

fn cylinder_frame_agrees_with_model(
    ir: &CadIr,
    surface_id: u32,
    frame: &crate::surface::PositionalCylinderFrame,
    source_carriers: &crate::decode::source_carriers::SourceUnitCarriers,
) -> bool {
    let mut model_surfaces = ir.model.surfaces.iter().filter(|surface| {
        crate::identity::matches_numbered_identity(
            surface.id.as_str(),
            "creo:visibgeom:surface#",
            surface_id,
        )
    });
    let surface = match (model_surfaces.next(), model_surfaces.next()) {
        (None, None) => return true,
        (Some(surface), None) => surface,
        _ => return false,
    };
    let geometry = source_carriers.surface_geometry(surface);
    let Some(SolvedSurfaceGeometry::Cylinder(cylinder_surface)) = geometry.solved() else {
        return matches!(
            geometry,
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { .. })
        );
    };
    let origin = cylinder_surface.origin().get();
    let radius = cylinder_surface.radius().get();
    let frame_axis = unit_length(*frame.frame().orthonormal_frame().axis());
    let model_axis = unit_length(*cylinder_surface.frame().axis());
    let frame_ref = unit_length(*frame.frame().orthonormal_frame().reference());
    let model_ref = unit_length(*cylinder_surface.frame().reference());
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
        || !close(frame.radius().get(), radius)
    {
        return false;
    }
    let model_origin = [origin.x, origin.y, origin.z];
    let relative = std::array::from_fn(|index| model_origin[index] - frame.frame().origin()[index]);
    let axial = dot(relative, frame_axis);
    let radial = std::array::from_fn(|index| relative[index] - axial * frame_axis[index]);
    let scale = frame
        .frame()
        .origin()
        .into_iter()
        .chain(model_origin)
        .map(f64::abs)
        .fold(1.0, f64::max);
    dot(radial, radial).sqrt() <= EPS_CYLINDER_CARRIER * scale
}

pub(super) fn ordered_parallel_cap_extent(
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
            side: blind_extrude_side(signed_length.abs())?,
        },
        start
            .normal
            .map(|component| component * signed_length.signum()),
    ))
}

pub(in super::super) fn generated_cap_plane_extent(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &CadIr,
    source_carriers: &crate::decode::source_carriers::SourceUnitCarriers,
    feature_id: u32,
) -> Result<Option<(ExtrudeExtent, [f64; 3])>, CodecError> {
    let Some((start_id, end_id)) = (|| {
        let table = exactly_one(
            scan.features
                .entity_tables
                .iter()
                .filter(|table| table.feature_id == feature_id && table.table_class_id == 29),
        )?;
        let mut start_id = None;
        let mut end_id = None;
        let mut side_count = 0_usize;
        for entry in &table.entries {
            match (entry.class_id(), entry.source_entity_id()) {
                (204, None) if start_id.replace(entry.entity_id).is_none() => {}
                (203, None) if end_id.replace(entry.entity_id).is_none() => {}
                (200, Some(_)) => side_count += 1,
                _ => return None,
            }
        }
        (side_count > 0
            && table.contains_surface_id(start_id?)
            && table.contains_surface_id(end_id?))
        .then_some(())?;
        Some((start_id?, end_id?))
    })() else {
        return Ok(None);
    };
    let local_planes = placed_planes(ctx, scan)?;
    let plane = |surface_id: u32| -> Result<Option<PlaneEquation>, CodecError> {
        let Some(row) = crate::surface::unique_surface_row(&scan.surfaces.rows, surface_id) else {
            return Ok(None);
        };
        if row.feature_id != feature_id || row.kind != crate::surface::SurfaceKind::Plane {
            return Ok(None);
        }
        reconciled_model_plane(ctx, &local_planes, ir, source_carriers, surface_id)
    };
    let start = plane(start_id)?;
    let end = plane(end_id)?;
    Ok(start
        .zip(end)
        .and_then(|(start, end)| ordered_parallel_cap_extent(start, end)))
}

pub(in super::super) fn unique_available_positional_cylinder_frame_records(
    ctx: &DecodeContext<'_>,
    surface_ids: &BTreeSet<u32>,
    parameters: &[crate::surface::SurfaceParameterRecord],
) -> Result<Option<Vec<(u32, crate::surface::PositionalCylinderFrame)>>, CodecError> {
    let mut frames = Vec::new();
    for surface_id in surface_ids {
        let mut matching = parameters
            .iter()
            .filter(|record| record.surface_id == *surface_id);
        let first = matching.next();
        if matching.next().is_some() {
            return Ok(None);
        }
        if let Some(frame) = first.and_then(SurfaceParameterRecord::positional_cylinder_frame) {
            ctx.reserve_vec(&mut frames, 1, "creo available positional cylinder frames")?;
            frames.push((*surface_id, frame));
        }
    }
    Ok(Some(frames))
}

pub(in super::super) fn agreed_generated_cylinder_extent<'a>(
    transform: &crate::placement::FeatureSectionTransform,
    frames: impl IntoIterator<Item = &'a crate::surface::PositionalCylinderFrame>,
) -> Option<(ExtrudeExtent, [f64; 3])> {
    let normal = transform.normal();
    let mut frames = frames.into_iter();
    let first = *frames.next()?;
    let length = first.length()?;
    let direction = unit_length(*first.frame().orthonormal_frame().axis());
    let close = |left: f64, right: f64| {
        (left - right).abs() <= EPS_GEOMETRY_AGREEMENT * left.abs().max(right.abs()).max(1.0)
    };
    let agrees = |frame: &crate::surface::PositionalCylinderFrame| {
        frame
            .length()
            .is_some_and(|candidate| close(candidate.get(), length.get()))
            && unit_length(*frame.frame().orthonormal_frame().axis())
                .iter()
                .zip(direction)
                .all(|(left, right)| close(*left, right))
            && close(
                dot(
                    std::array::from_fn(|index| {
                        frame.frame().origin()[index] - transform.origin()[index]
                    }),
                    normal,
                ),
                0.0,
            )
    };
    (agrees(&first) && frames.all(agrees)).then_some(())?;
    close(dot(direction, normal).abs(), 1.0).then_some(())?;
    Some((
        ExtrudeExtent::OneSided {
            side: ExtrudeSide {
                termination: LinearTermination::Blind {
                    length: cadmpeg_ir::scalar::NonZeroLength::from(length),
                },
                draft: None,
            },
        },
        direction,
    ))
}

#[cfg(test)]
mod tests;
