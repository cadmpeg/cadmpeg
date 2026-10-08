// SPDX-License-Identifier: Apache-2.0
//! Feature plane equations and generated cylinder and cap extents.

use super::super::holes::sweep::blind_extrude_side;
use super::super::uniqueness::exactly_one_by;
use crate::container::ContainerScan;
use crate::decode::analytic::equations::PlaneEquation;
use crate::decode::analytic::planes::{canonical_plane, placed_planes, reconciled_model_plane};
use crate::surface::SurfaceParameterRecord;
use crate::vecmath::dot;
use crate::vecmath::unit_length;
use cadmpeg_core::decode::{DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{ExtrudeExtent, ExtrudeSide, LinearTermination};
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use std::collections::{BTreeMap, BTreeSet};

const EPS_CYLINDER_CARRIER: f64 = 1.0e-9;

type PlaneCarrierIndex<'a, T> = std::collections::HashMap<u32, Option<&'a T>>;

fn plane_carrier_index<'a, T>(
    ctx: &DecodeContext<'_>,
    records: &'a [T],
    id: impl Fn(&T) -> u32,
) -> Result<PlaneCarrierIndex<'a, T>, CodecError> {
    let mut index = std::collections::HashMap::new();
    for record in ctx.admit_iter(records, "creo plane carrier index scan")? {
        ctx.entry_hash_map(&mut index, id(record), "creo plane carrier index")?
            .and_modify(|record| *record = None)
            .or_insert(Some(record));
    }
    Ok(index)
}


const EPS_GEOMETRY_AGREEMENT: f64 = 1.0e-9;
const EPS_AXIS_ALIGNMENT: f64 = 1.0e-10;
const EPS_SIGNED_LENGTH: f64 = 1.0e-9;

type ScopedPlanes<'ctx, T> = (Vec<T>, ScopedReservation<'ctx>);

pub(in super::super) fn feature_plane_equations<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &CadIr,
    source_carriers: &crate::decode::source_carriers::SourceUnitCarriers,
    feature_id: u32,
) -> Result<Option<ScopedPlanes<'ctx, PlaneEquation>>, CodecError> {
    let mut plane_storage = ctx.reserve_scoped(0, "creo feature plane scratch")?;
    let mut ids = BTreeSet::new();
    for row in ctx.admit_iter(&*scan.surfaces.rows, "creo feature plane row scan")?.filter(|row| {
        row.feature_id == feature_id && row.kind == crate::surface::SurfaceKind::Plane
    }) {
        plane_storage.with_storage(|| ctx.insert_btree_set(&mut ids, row.id, "creo feature plane ID nodes"))?;
    }
    let mut local_planes = BTreeMap::new();
    let mut outlines = None;
    let mut local_systems = None;
    let mut plane_ids = ids.iter();
    while let Some(id) = ctx.next_charged(&mut plane_ids, "creo feature plane local ID scan")? {
        if scan.surfaces.rows.unique(*id).is_none() { return Ok(None); }
        if outlines.is_none() {
            outlines = Some(plane_storage.with_storage(|| plane_carrier_index(ctx, &scan.planes.outlines, |plane| plane.surface_id))?);
        }
        let Some(outlines) = &outlines else { return Ok(None); };
        let plane = match outlines.get(id) {
            Some(Some(plane)) => Some(PlaneEquation { origin: plane.origin, normal: plane.normal() }),
            Some(None) => return Ok(None),
            None => {
                if local_systems.is_none() {
                    local_systems = Some(plane_storage.with_storage(|| plane_carrier_index(ctx, &scan.planes.local_systems, |frame| frame.surface_id))?);
                }
                let Some(local_systems) = &local_systems else { return Ok(None); };
                match local_systems.get(id) {
                    Some(Some(frame)) => {
                        let frame = frame.frame();
                        frame.origin.zip(frame.normal()).map(|(origin, normal)| PlaneEquation { origin, normal })
                    }
                    Some(None) => return Ok(None),
                    None => None,
                }
            }
        };
        if let Some(plane) = plane {
            plane_storage.with_storage(|| ctx.insert_btree_map(&mut local_planes, *id, plane, "creo feature local plane nodes"))?;
        }
    }
    let mut equation_storage = ctx.reserve_scoped(0, "creo feature plane equation scratch")?;
    let mut equations = Vec::new();
    let mut plane_ids = ids.into_iter();
    while let Some(id) = ctx.next_charged(&mut plane_ids, "creo feature plane equation ID scan")? {
        let Some(plane) = reconciled_model_plane(ctx, &local_planes, ir, source_carriers, id)?
        else {
            return Ok(None);
        };
        ctx.reserve_scoped_vec(&mut equation_storage, &mut equations, 1, "creo feature plane equations")?;
        equations.push(plane);
    }
    Ok(Some((equations, equation_storage)))
}

pub(in super::super) type FeatureOutlinePlane = (u32, [f64; 3], [f64; 3]);

/// Resolve one uniquely identified plane row to one unambiguous placed plane
/// equation. The equation may be carried by one outline and one positional
/// frame; both carriers are valid only when they agree on origin and normal.
pub(in super::super) fn feature_outline_plane(
    ctx: &DecodeContext<'_>, scan: &ContainerScan, feature_id: u32, surface_id: u32,
) -> Result<Option<FeatureOutlinePlane>, CodecError> {
    let Some(row) = scan.surfaces.rows.unique(surface_id) else { return Ok(None); };
    if row.feature_id != feature_id || row.kind != crate::surface::SurfaceKind::Plane { return Ok(None); }
    let outline_index = ctx.position_by(&scan.planes.outlines, |plane| Ok(plane.surface_id == surface_id), "creo feature outline plane search")?;
    if let Some(index) = outline_index {
        if ctx.any_by(&scan.planes.outlines[index + 1..], |plane| Ok(plane.surface_id == surface_id), "creo feature outline plane search")? { return Ok(None); }
    }
    let positional_index = ctx.position_by(&scan.planes.positional_frames, |plane| Ok(plane.surface_id == surface_id), "creo feature positional plane search")?;
    if let Some(index) = positional_index {
        if ctx.any_by(&scan.planes.positional_frames[index + 1..], |plane| Ok(plane.surface_id == surface_id), "creo feature positional plane search")? { return Ok(None); }
    }
    let outline = outline_index.map(|index| &scan.planes.outlines[index]);
    let positional = positional_index.map(|index| &scan.planes.positional_frames[index]);
    Ok(placed_outline_plane(surface_id, outline, positional))
}

fn placed_outline_plane(
    surface_id: u32,
    outline: Option<&crate::surface::OutlinePlane>,
    positional: Option<&crate::surface::OutlinePlane>,
) -> Option<FeatureOutlinePlane> {
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
pub(in super::super) fn feature_outline_planes<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    scan: &ContainerScan,
    feature_id: u32,
) -> Result<Option<ScopedPlanes<'ctx, FeatureOutlinePlane>>, CodecError> {
    let (mut planes, mut storage) = ctx.temporary_vec(0, "creo feature outline planes")?;
    let mut index_storage = ctx.reserve_scoped(0, "creo feature outline carrier scratch")?;
    let mut carriers = None;
    let mut rows = scan.surfaces.rows.iter();
    while let Some(row) = ctx.next_charged(&mut rows, "creo feature outline row scan")? {
        if row.feature_id != feature_id || row.kind != crate::surface::SurfaceKind::Plane { continue; }
        if scan.surfaces.rows.unique(row.id).is_none() { return Ok(None); }
        if carriers.is_none() {
            carriers = Some(index_storage.with_storage(|| {
                let outlines = plane_carrier_index(ctx, &scan.planes.outlines, |plane| plane.surface_id)?;
                let positional = plane_carrier_index(ctx, &scan.planes.positional_frames, |plane| plane.surface_id)?;
                Ok::<_, CodecError>((outlines, positional))
            })?);
        }
        let Some((outlines, positional)) = &carriers else { return Ok(None); };
        let outline = match outlines.get(&row.id) { Some(None) => return Ok(None), value => value.copied().flatten() };
        let positional = match positional.get(&row.id) { Some(None) => return Ok(None), value => value.copied().flatten() };
        let Some(plane) = placed_outline_plane(row.id, outline, positional) else { return Ok(None); };
        ctx.reserve_scoped_vec(&mut storage, &mut planes, 1, "creo feature outline planes")?;
        planes.push(plane);
    }
    Ok(Some((planes, storage)))
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
    let mut id_storage = ctx.reserve_scoped(0, "creo generated arc ID scratch")?;
    let mut surface_ids = BTreeSet::new();
    let mut tables = scan.features.entity_tables.iter();
    while let Some(table) = ctx.next_charged(&mut tables, "creo generated arc table scan")? {
        if table.feature_id != feature_id { continue; }
        let mut entries = table.entries.iter();
        while let Some(entry) = ctx.next_charged(&mut entries, "creo generated arc entry scan")? {
            if !ctx.contains_btree_set(table.unique_surface_ids(), &entry.entity_id, "creo generated arc surface membership")? { continue; }
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
        if ctx.contains_btree_set(&surface_ids, &row.id, "creo generated arc duplicate surface lookup")? {
            return Ok(None);
        }
        id_storage.with_storage(|| ctx.insert_btree_set(
            &mut surface_ids,
            row.id,
            "creo generated arc cylinder ID nodes",
        ))?;
    }
    }
    let (frame_records, _frame_storage) = ctx.with_scoped_storage("creo generated arc frame scratch", || unique_available_positional_cylinder_frame_records(ctx, &surface_ids, &scan.surfaces.parameters))?;
    let Some(frame_records) = frame_records
    else {
        return Ok(None);
    };
    if frame_records.is_empty() { return Ok(None); }
    let (geometries, _geometry_storage) = ctx.with_scoped_storage("creo generated arc surface index scratch", || super::extent::source_surface_geometries(ctx, ir, source_carriers))?;
    if !ctx.all_by(
            &(frame_records)[..],
            |(surface_id, frame)| -> Result<bool, cadmpeg_core::CodecError> {
                Ok(cylinder_frame_agrees_with_geometry(super::extent::unique_source_surface_geometry(&geometries, *surface_id), frame))
            },
            "creo numbered identity candidate scan",
        )?
    {
        return Ok(None);
    }
    agreed_generated_cylinder_extent(
        ctx, transform,
        frame_records.iter().map(|(_, frame)| frame),
    )
}

fn cylinder_frame_agrees_with_geometry(
    geometry: Option<super::extent::SourceSurfaceGeometry<'_>>,
    frame: &crate::surface::PositionalCylinderFrame,
) -> bool {
    let geometry = match geometry {
        Some(super::extent::SourceSurfaceGeometry::Present(geometry)) => geometry,
        Some(super::extent::SourceSurfaceGeometry::Missing) => return true,
        None => return false,
    };
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
    let Some(table) = exactly_one_by(ctx, &scan.features.entity_tables,
        |table| Ok(table.feature_id == feature_id && table.table_class_id == 29), "creo generated cap table search")? else { return Ok(None); };
    let mut start_id = None;
    let mut end_id = None;
    let mut side_count = 0_usize;
    let mut entries = table.entries.iter();
    while let Some(entry) = ctx.next_charged(&mut entries, "creo generated cap entry scan")? {
        match (entry.class_id(), entry.source_entity_id()) {
            (204, None) if start_id.replace(entry.entity_id).is_none() => {},
            (203, None) if end_id.replace(entry.entity_id).is_none() => {},
            (200, Some(_)) => side_count += 1,
            _ => return Ok(None),
        }
    }
    let (Some(start_id), Some(end_id)) = (start_id, end_id) else { return Ok(None); };
    if side_count == 0
        || !ctx.contains_btree_set(table.unique_surface_ids(), &start_id, "creo generated cap surface membership")?
        || !ctx.contains_btree_set(table.unique_surface_ids(), &end_id, "creo generated cap surface membership")? { return Ok(None); }
    let (local_planes, _local_plane_storage) = ctx.with_scoped_storage("creo cap local plane scratch", || placed_planes(ctx, scan))?;
    let plane = |surface_id: u32| -> Result<Option<PlaneEquation>, cadmpeg_core::CodecError> {
        let Some(row) = crate::surface::unique_surface_row(&scan.surfaces.rows, surface_id) else {
            return Ok(None);
        };
        if row.feature_id != feature_id || row.kind != crate::surface::SurfaceKind::Plane {
            return Ok(None);
        }
        reconciled_model_plane(ctx, &local_planes, ir, source_carriers, surface_id)
    };
    Ok(plane(start_id)?
        .zip(plane(end_id)?)
        .and_then(|(start, end)| ordered_parallel_cap_extent(start, end)))
}

pub(in super::super) fn unique_available_positional_cylinder_frame_records(
    ctx: &DecodeContext<'_>,
    surface_ids: &BTreeSet<u32>,
    parameters: &[crate::surface::SurfaceParameterRecord],
) -> Result<Option<Vec<(u32, crate::surface::PositionalCylinderFrame)>>, CodecError> {
    let mut frames = Vec::new();
    if surface_ids.is_empty() { return Ok(Some(frames)); }
    let mut storage = ctx.reserve_scoped(0, "creo positional cylinder parameter index")?;
    let mut by_id = std::collections::HashMap::new();
    let mut records = parameters.iter();
    while let Some(record) = ctx.next_charged(&mut records, "creo positional cylinder parameter scan")? {
        if !ctx.contains_btree_set(surface_ids, &record.surface_id, "creo positional cylinder ID lookup")? { continue; }
        match storage.with_storage(|| ctx.entry_hash_map(&mut by_id, record.surface_id, "creo positional cylinder parameter index"))? {
            std::collections::hash_map::Entry::Vacant(entry) => { entry.insert(record); },
            std::collections::hash_map::Entry::Occupied(_) => return Ok(None),
        }
    }
    let mut ids = surface_ids.iter();
    while let Some(surface_id) = ctx.next_charged(&mut ids, "creo positional cylinder frame ID scan")? {
        let record = by_id.get(surface_id).copied();
        if let Some(frame) = record.and_then(SurfaceParameterRecord::positional_cylinder_frame) {
            ctx.push_vec(&mut frames, (*surface_id, frame), "creo available positional cylinder frames")?;
        }
    }
    Ok(Some(frames))
}

pub(in super::super) fn agreed_generated_cylinder_extent<'a>(
    ctx: &DecodeContext<'_>,
    transform: &crate::placement::FeatureSectionTransform,
    frames: impl IntoIterator<Item = &'a crate::surface::PositionalCylinderFrame>,
) -> Result<Option<(ExtrudeExtent, [f64; 3])>, CodecError> {
    let normal = transform.normal();
    let mut frames = frames.into_iter();
    let Some(first) = ctx.next_charged(&mut frames, "creo generated cylinder frame scan")? else { return Ok(None); };
    let first = *first;
    let Some(length) = first.length() else { return Ok(None); };
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
    if !agrees(&first) || !ctx.all_by(frames, |frame| Ok(agrees(frame)), "creo generated cylinder frame agreement")? { return Ok(None); }
    if !close(dot(direction, normal).abs(), 1.0) { return Ok(None); }
    Ok(Some((
        ExtrudeExtent::OneSided {
            side: ExtrudeSide {
                termination: LinearTermination::Blind {
                    length: cadmpeg_ir::scalar::NonZeroLength::from(length),
                },
                draft: None,
            },
        },
        direction,
    )))
}

#[cfg(test)]
mod tests;
