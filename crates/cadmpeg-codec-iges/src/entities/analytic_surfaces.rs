// SPDX-License-Identifier: Apache-2.0
//! Pointer-defined analytic surface projection.

use super::geometry::{entity_loss, resolve_transform, source_object, Affine, ProjectionOutcome};
use crate::directory::DirectoryEntry;
use crate::global::ProjectedGlobal;
use crate::parameter::ParameterRecord;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_ir::geometry::{derive_reference_direction, Surface, SurfaceGeometry};
use cadmpeg_ir::ids::{PointId, SurfaceId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::CadIr;
use std::collections::{BTreeMap, BTreeSet};

fn pointer(record: &ParameterRecord, index: usize) -> Option<u32> {
    record.integer(index).and_then(|value| {
        let sequence = u32::try_from(value).ok()?;
        (sequence % 2 == 1).then_some(sequence)
    })
}

fn point(ir: &CadIr, sequence: u32) -> Option<Point3> {
    let id = PointId::mint(format!("iges:model:point#D{sequence}")).expect("identity grammar");
    ir.model
        .points
        .iter()
        .find(|point| point.id == id)
        .map(|point| point.position)
}

#[allow(clippy::many_single_char_names)]
fn direction(
    sequence: u32,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    records: &BTreeMap<u32, &ParameterRecord>,
) -> Result<Vector3, String> {
    let entry = entries
        .get(&sequence)
        .copied()
        .ok_or_else(|| format!("points to missing Directory entry D{sequence}"))?;
    if entry.entity_type != 123 || entry.form != 0 {
        return Err(format!(
            "points to type {} form {} at D{sequence}, not type 123 form 0",
            entry.entity_type, entry.form
        ));
    }
    if !entry.status.is_physically_dependent() {
        return Err(format!(
            "points to D{sequence}, which is not physically dependent"
        ));
    }
    if entry.transform != 0 {
        return Err(format!(
            "points to D{sequence}, which has a prohibited transformation"
        ));
    }
    let record = records
        .get(&sequence)
        .copied()
        .ok_or_else(|| format!("points to D{sequence}, whose Parameter Data record is missing"))?;
    let components = [record.number(1), record.number(2), record.number(3)];
    let [Some(x), Some(y), Some(z)] = components else {
        return Err(format!(
            "points to D{sequence}, whose direction components are not numeric"
        ));
    };
    {
        let v = Vector3::new(x, y, z);
        let n = v.norm();
        (n.is_finite() && n > 0.0).then(|| v.scale(1.0 / n))
    }
    .ok_or_else(|| format!("points to D{sequence}, whose direction is zero or non-finite"))
}

fn required_direction(
    record: &ParameterRecord,
    index: usize,
    role: &str,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    records: &BTreeMap<u32, &ParameterRecord>,
) -> Result<Vector3, String> {
    let sequence = pointer(record, index)
        .ok_or_else(|| format!("{role} pointer is missing, even, or non-integer"))?;
    direction(sequence, entries, records).map_err(|message| format!("{role} {message}"))
}

fn transformed_direction(
    record: &ParameterRecord,
    index: usize,
    role: &str,
    transform: Affine,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    records: &BTreeMap<u32, &ParameterRecord>,
) -> Result<Vector3, String> {
    let direction = required_direction(record, index, role, entries, records)?;
    {
        let v = transform.vector(direction);
        let n = v.norm();
        (n.is_finite() && n > 0.0).then(|| v.scale(1.0 / n))
    }
    .ok_or_else(|| format!("{role} collapses under the surface transformation"))
}

fn form_reference_direction(
    form: i64,
    record: &ParameterRecord,
    index: usize,
    role: &str,
    transform: Affine,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    records: &BTreeMap<u32, &ParameterRecord>,
) -> Result<Option<Vector3>, String> {
    if form == 0 {
        Ok(None)
    } else {
        transformed_direction(record, index, role, transform, entries, records).map(Some)
    }
}

fn reference_direction(axis: Vector3, candidate: Option<Vector3>) -> Option<Vector3> {
    match candidate {
        Some(candidate) => {
            let v = candidate - axis.scale(axis.dot(candidate));
            let n = v.norm();
            (n.is_finite() && n > 0.0).then(|| v.scale(1.0 / n))
        }
        None => Some(derive_reference_direction(axis)),
    }
}

fn surface_transform(
    entry: &DirectoryEntry,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    records: &BTreeMap<u32, &ParameterRecord>,
    global: &ProjectedGlobal,
    ctx: Option<&DecodeContext<'_>>,
) -> Result<Affine, String> {
    resolve_transform(
        entry.transform,
        entries,
        records,
        global.length_factor_mm(),
        global.real_precision(),
        &mut BTreeSet::new(),
        ctx,
    )
}

pub(super) fn project(
    ir: &mut CadIr,
    directory: &[DirectoryEntry],
    parameters: &[ParameterRecord],
    global: &ProjectedGlobal,
    ctx: Option<&DecodeContext<'_>>,
) -> ProjectionOutcome {
    let records = parameters
        .iter()
        .map(|record| (record.directory_sequence, record))
        .collect::<BTreeMap<_, _>>();
    let entries = directory
        .iter()
        .map(|entry| (entry.sequence, entry))
        .collect::<BTreeMap<_, _>>();
    let mut decoded = BTreeSet::new();
    let mut losses = Vec::new();

    for entry in directory.iter().filter(|entry| {
        matches!(entry.entity_type, 190 | 192 | 194 | 196 | 198) && matches!(entry.form, 0 | 1)
    }) {
        let factor = global.length_factor_mm();
        let Some(record) = records.get(&entry.sequence).copied() else {
            losses.push(entity_loss(entry, "Parameter Data record is missing"));
            continue;
        };
        let transform = match surface_transform(entry, &entries, &records, global, ctx) {
            Ok(transform) => transform,
            Err(message) => {
                losses.push(entity_loss(entry, message));
                continue;
            }
        };
        let location_index = pointer(record, 1);
        let Some(location) = location_index.and_then(|sequence| point(ir, sequence)) else {
            losses.push(entity_loss(
                entry,
                "analytic surface location point is missing",
            ));
            continue;
        };
        let location = transform.point(location);
        let result = match entry.entity_type {
            190 => {
                let axis = match transformed_direction(
                    record,
                    2,
                    "plane normal",
                    transform,
                    &entries,
                    &records,
                ) {
                    Ok(axis) => axis,
                    Err(message) => {
                        losses.push(entity_loss(entry, message));
                        continue;
                    }
                };
                let candidate = match form_reference_direction(
                    entry.form,
                    record,
                    3,
                    "plane reference direction",
                    transform,
                    &entries,
                    &records,
                ) {
                    Ok(candidate) => candidate,
                    Err(message) => {
                        losses.push(entity_loss(entry, message));
                        continue;
                    }
                };
                let Some(u_axis) = reference_direction(axis, candidate) else {
                    losses.push(entity_loss(
                        entry,
                        "plane reference direction is parallel to its normal",
                    ));
                    continue;
                };
                SurfaceGeometry::Plane(
                    match cadmpeg_ir::geometry::PlaneSurface::try_new(location, axis, u_axis) {
                        Ok(payload) => payload,
                        Err(message) => {
                            losses.push(entity_loss(entry, message));
                            continue;
                        }
                    },
                )
            }
            192 => {
                let axis = match transformed_direction(
                    record,
                    2,
                    "cylinder axis",
                    transform,
                    &entries,
                    &records,
                ) {
                    Ok(axis) => axis,
                    Err(message) => {
                        losses.push(entity_loss(entry, message));
                        continue;
                    }
                };
                let Some(radius) = record.number(3).map(|radius| radius * factor) else {
                    losses.push(entity_loss(entry, "cylinder radius is not numeric"));
                    continue;
                };
                let candidate = match form_reference_direction(
                    entry.form,
                    record,
                    4,
                    "cylinder reference direction",
                    transform,
                    &entries,
                    &records,
                ) {
                    Ok(candidate) => candidate,
                    Err(message) => {
                        losses.push(entity_loss(entry, message));
                        continue;
                    }
                };
                let Some(ref_direction) = reference_direction(axis, candidate) else {
                    losses.push(entity_loss(
                        entry,
                        "cylinder reference direction is parallel to its axis",
                    ));
                    continue;
                };
                SurfaceGeometry::Cylinder(
                    match cadmpeg_ir::geometry::CylinderSurface::try_new(
                        location,
                        axis,
                        ref_direction,
                        radius,
                    ) {
                        Ok(payload) => payload,
                        Err(message) => {
                            losses.push(entity_loss(entry, message));
                            continue;
                        }
                    },
                )
            }
            194 => {
                let axis = match transformed_direction(
                    record,
                    2,
                    "cone axis",
                    transform,
                    &entries,
                    &records,
                ) {
                    Ok(axis) => axis,
                    Err(message) => {
                        losses.push(entity_loss(entry, message));
                        continue;
                    }
                };
                let Some(radius) = record.number(3).map(|radius| radius * factor) else {
                    losses.push(entity_loss(entry, "cone radius is not numeric"));
                    continue;
                };
                let Some(half_angle) = record.number(4).map(f64::to_radians).filter(|angle| {
                    angle.is_finite() && *angle > 0.0 && *angle < std::f64::consts::FRAC_PI_2
                }) else {
                    losses.push(entity_loss(
                        entry,
                        "cone semi-angle is outside (0, 90) degrees",
                    ));
                    continue;
                };
                let candidate = match form_reference_direction(
                    entry.form,
                    record,
                    5,
                    "cone reference direction",
                    transform,
                    &entries,
                    &records,
                ) {
                    Ok(candidate) => candidate,
                    Err(message) => {
                        losses.push(entity_loss(entry, message));
                        continue;
                    }
                };
                let Some(ref_direction) = reference_direction(axis, candidate) else {
                    losses.push(entity_loss(
                        entry,
                        "cone reference direction is parallel to its axis",
                    ));
                    continue;
                };
                SurfaceGeometry::Cone(
                    match cadmpeg_ir::geometry::ConeSurface::try_new(
                        location,
                        axis,
                        ref_direction,
                        radius,
                        1.0,
                        half_angle,
                    ) {
                        Ok(payload) => payload,
                        Err(message) => {
                            losses.push(entity_loss(entry, message));
                            continue;
                        }
                    },
                )
            }
            196 => {
                let Some(radius) = record.number(2).map(|radius| radius * factor) else {
                    losses.push(entity_loss(entry, "sphere radius is not numeric"));
                    continue;
                };
                let axis = if entry.form == 1 {
                    transformed_direction(record, 3, "sphere axis", transform, &entries, &records)
                } else {
                    {
                        let v = transform.vector(Vector3::new(0.0, 0.0, 1.0));
                        let n = v.norm();
                        (n.is_finite() && n > 0.0).then(|| v.scale(1.0 / n))
                    }
                    .ok_or_else(|| "sphere axis collapses under its transformation".to_owned())
                };
                let axis = match axis {
                    Ok(axis) => axis,
                    Err(message) => {
                        losses.push(entity_loss(entry, message));
                        continue;
                    }
                };
                let candidate = match form_reference_direction(
                    entry.form,
                    record,
                    4,
                    "sphere reference direction",
                    transform,
                    &entries,
                    &records,
                ) {
                    Ok(candidate) => candidate,
                    Err(message) => {
                        losses.push(entity_loss(entry, message));
                        continue;
                    }
                };
                let Some(ref_direction) = reference_direction(axis, candidate) else {
                    losses.push(entity_loss(
                        entry,
                        "sphere reference direction is parallel to its axis",
                    ));
                    continue;
                };
                SurfaceGeometry::Sphere(
                    match cadmpeg_ir::geometry::SphereSurface::try_new(
                        location,
                        axis,
                        ref_direction,
                        radius,
                    ) {
                        Ok(payload) => payload,
                        Err(message) => {
                            losses.push(entity_loss(entry, message));
                            continue;
                        }
                    },
                )
            }
            198 => {
                let axis = match transformed_direction(
                    record,
                    2,
                    "torus axis",
                    transform,
                    &entries,
                    &records,
                ) {
                    Ok(axis) => axis,
                    Err(message) => {
                        losses.push(entity_loss(entry, message));
                        continue;
                    }
                };
                let radii = [record.number(3), record.number(4)];
                let [Some(major_radius), Some(minor_radius)] = radii else {
                    losses.push(entity_loss(entry, "torus radii are not numeric"));
                    continue;
                };
                let (major_radius, minor_radius) = (major_radius * factor, minor_radius * factor);
                if minor_radius <= 0.0 || minor_radius >= major_radius {
                    losses.push(entity_loss(
                        entry,
                        "torus radii do not satisfy 0 < minor < major",
                    ));
                    continue;
                }
                let candidate = match form_reference_direction(
                    entry.form,
                    record,
                    5,
                    "torus reference direction",
                    transform,
                    &entries,
                    &records,
                ) {
                    Ok(candidate) => candidate,
                    Err(message) => {
                        losses.push(entity_loss(entry, message));
                        continue;
                    }
                };
                let Some(ref_direction) = reference_direction(axis, candidate) else {
                    losses.push(entity_loss(
                        entry,
                        "torus reference direction is parallel to its axis",
                    ));
                    continue;
                };
                SurfaceGeometry::Torus(
                    match cadmpeg_ir::geometry::TorusSurface::try_new(
                        location,
                        axis,
                        ref_direction,
                        major_radius,
                        minor_radius,
                    ) {
                        Ok(payload) => payload,
                        Err(message) => {
                            losses.push(entity_loss(entry, message));
                            continue;
                        }
                    },
                )
            }
            _ => {
                losses.push(entity_loss(entry, "analytic surface type is unsupported"));
                continue;
            }
        };
        ir.model.surfaces.push(Surface {
            id: SurfaceId::mint(format!("iges:model:surface#D{}", entry.sequence))
                .expect("identity grammar"),
            geometry: result,
            source_object: Some(match source_object(entry) {
                Ok(source) => source,
                Err(error) => {
                    losses.push(entity_loss(entry, error.to_string()));
                    continue;
                }
            }),
        });
        decoded.insert(entry.sequence);
    }

    ProjectionOutcome { decoded, losses }
}

#[cfg(test)]
mod tests;
