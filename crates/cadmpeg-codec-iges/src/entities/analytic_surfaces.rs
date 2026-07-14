// SPDX-License-Identifier: Apache-2.0
//! Pointer-defined analytic surface projection.

use super::geometry::{entity_loss, resolve_transform, source_object, Affine};
use crate::directory::DirectoryEntry;
use crate::global::Global;
use crate::parameter::ParameterRecord;
use cadmpeg_ir::geometry::{derive_reference_direction, Surface, SurfaceGeometry};
use cadmpeg_ir::ids::{PointId, SurfaceId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::report::LossNote;
use cadmpeg_ir::CadIr;
use std::collections::{BTreeMap, BTreeSet};

fn dot(left: Vector3, right: Vector3) -> f64 {
    left.x * right.x + left.y * right.y + left.z * right.z
}

fn subtract(left: Vector3, right: Vector3) -> Vector3 {
    Vector3::new(left.x - right.x, left.y - right.y, left.z - right.z)
}

fn scale(vector: Vector3, factor: f64) -> Vector3 {
    Vector3::new(vector.x * factor, vector.y * factor, vector.z * factor)
}

fn normalized(vector: Vector3) -> Option<Vector3> {
    let norm = vector.norm();
    (norm.is_finite() && norm > 0.0)
        .then(|| Vector3::new(vector.x / norm, vector.y / norm, vector.z / norm))
}

fn pointer(record: &ParameterRecord, index: usize) -> Option<u32> {
    record.integer(index).and_then(|value| {
        let sequence = u32::try_from(value).ok()?;
        (sequence % 2 == 1).then_some(sequence)
    })
}

fn point(ir: &CadIr, sequence: u32) -> Option<Point3> {
    let id = PointId(format!("iges:model:point#D{sequence}"));
    ir.model
        .points
        .iter()
        .find(|point| point.id == id)
        .map(|point| point.position)
}

fn direction(
    sequence: u32,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    records: &BTreeMap<u32, &ParameterRecord>,
    factor: f64,
) -> Option<Vector3> {
    let entry = entries.get(&sequence).copied()?;
    if entry.entity_type != 123 || entry.form != 0 {
        return None;
    }
    let record = records.get(&sequence).copied()?;
    let vector = Vector3::new(record.number(1)?, record.number(2)?, record.number(3)?);
    let transform = resolve_transform(
        entry.transform,
        entries,
        records,
        factor,
        &mut BTreeSet::new(),
    )
    .ok()?;
    normalized(transform.vector(vector))
}

fn reference_direction(axis: Vector3, candidate: Option<Vector3>) -> Option<Vector3> {
    match candidate {
        Some(candidate) => normalized(subtract(candidate, scale(axis, dot(axis, candidate)))),
        None => Some(derive_reference_direction(axis)),
    }
}

fn surface_transform(
    entry: &DirectoryEntry,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    records: &BTreeMap<u32, &ParameterRecord>,
    factor: f64,
) -> Result<Affine, String> {
    resolve_transform(
        entry.transform,
        entries,
        records,
        factor,
        &mut BTreeSet::new(),
    )
}

pub(super) struct AnalyticSurfaceProjection {
    pub(super) handled: BTreeSet<u32>,
    pub(super) decoded: BTreeSet<u32>,
    pub(super) losses: Vec<LossNote>,
}

pub(super) fn project(
    ir: &mut CadIr,
    directory: &[DirectoryEntry],
    parameters: &[ParameterRecord],
    global: &Global,
) -> AnalyticSurfaceProjection {
    let records = parameters
        .iter()
        .map(|record| (record.directory_sequence, record))
        .collect::<BTreeMap<_, _>>();
    let entries = directory
        .iter()
        .map(|entry| (entry.sequence, entry))
        .collect::<BTreeMap<_, _>>();
    let mut handled = BTreeSet::new();
    let mut decoded = BTreeSet::new();
    let mut losses = Vec::new();

    for entry in directory.iter().filter(|entry| {
        matches!(entry.entity_type, 190 | 192 | 194 | 196 | 198) && matches!(entry.form, 0 | 1)
    }) {
        handled.insert(entry.sequence);
        let Some(factor) = global.length_factor_mm() else {
            losses.push(entity_loss(entry, "units or model scale are unsupported"));
            continue;
        };
        let Some(record) = records.get(&entry.sequence).copied() else {
            losses.push(entity_loss(entry, "Parameter Data record is missing"));
            continue;
        };
        let transform = match surface_transform(entry, &entries, &records, factor) {
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
                let Some(axis) = pointer(record, 2)
                    .and_then(|sequence| direction(sequence, &entries, &records, factor))
                    .and_then(|axis| normalized(transform.vector(axis)))
                else {
                    losses.push(entity_loss(entry, "plane normal direction is missing"));
                    continue;
                };
                let candidate = (entry.form == 1)
                    .then(|| pointer(record, 3))
                    .flatten()
                    .and_then(|sequence| direction(sequence, &entries, &records, factor))
                    .map(|direction| transform.vector(direction));
                let Some(u_axis) = reference_direction(axis, candidate) else {
                    losses.push(entity_loss(
                        entry,
                        "plane reference direction is parallel to its normal",
                    ));
                    continue;
                };
                SurfaceGeometry::Plane {
                    origin: location,
                    normal: axis,
                    u_axis,
                }
            }
            192 => {
                let Some(axis) = pointer(record, 2)
                    .and_then(|sequence| direction(sequence, &entries, &records, factor))
                    .and_then(|axis| normalized(transform.vector(axis)))
                else {
                    losses.push(entity_loss(entry, "cylinder axis direction is missing"));
                    continue;
                };
                let Some(radius) = record
                    .number(3)
                    .map(|radius| radius * factor)
                    .filter(|radius| radius.is_finite() && *radius > 0.0)
                else {
                    losses.push(entity_loss(
                        entry,
                        "cylinder radius is not positive and finite",
                    ));
                    continue;
                };
                let candidate = (entry.form == 1)
                    .then(|| pointer(record, 4))
                    .flatten()
                    .and_then(|sequence| direction(sequence, &entries, &records, factor))
                    .map(|direction| transform.vector(direction));
                let Some(ref_direction) = reference_direction(axis, candidate) else {
                    losses.push(entity_loss(
                        entry,
                        "cylinder reference direction is parallel to its axis",
                    ));
                    continue;
                };
                SurfaceGeometry::Cylinder {
                    origin: location,
                    axis,
                    ref_direction,
                    radius,
                }
            }
            194 => {
                let Some(axis) = pointer(record, 2)
                    .and_then(|sequence| direction(sequence, &entries, &records, factor))
                    .and_then(|axis| normalized(transform.vector(axis)))
                else {
                    losses.push(entity_loss(entry, "cone axis direction is missing"));
                    continue;
                };
                let Some(radius) = record
                    .number(3)
                    .map(|radius| radius * factor)
                    .filter(|radius| radius.is_finite() && *radius >= 0.0)
                else {
                    losses.push(entity_loss(entry, "cone radius is negative or non-finite"));
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
                let candidate = (entry.form == 1)
                    .then(|| pointer(record, 5))
                    .flatten()
                    .and_then(|sequence| direction(sequence, &entries, &records, factor))
                    .map(|direction| transform.vector(direction));
                let Some(ref_direction) = reference_direction(axis, candidate) else {
                    losses.push(entity_loss(
                        entry,
                        "cone reference direction is parallel to its axis",
                    ));
                    continue;
                };
                SurfaceGeometry::Cone {
                    origin: location,
                    axis,
                    ref_direction,
                    radius,
                    ratio: 1.0,
                    half_angle,
                }
            }
            196 => {
                let Some(radius) = record
                    .number(2)
                    .map(|radius| radius * factor)
                    .filter(|radius| radius.is_finite() && *radius > 0.0)
                else {
                    losses.push(entity_loss(
                        entry,
                        "sphere radius is not positive and finite",
                    ));
                    continue;
                };
                let axis = if entry.form == 1 {
                    pointer(record, 3)
                        .and_then(|sequence| direction(sequence, &entries, &records, factor))
                        .and_then(|axis| normalized(transform.vector(axis)))
                } else {
                    normalized(transform.vector(Vector3::new(0.0, 0.0, 1.0)))
                };
                let Some(axis) = axis else {
                    losses.push(entity_loss(entry, "sphere axis direction is missing"));
                    continue;
                };
                let candidate = (entry.form == 1)
                    .then(|| pointer(record, 4))
                    .flatten()
                    .and_then(|sequence| direction(sequence, &entries, &records, factor))
                    .map(|direction| transform.vector(direction));
                let Some(ref_direction) = reference_direction(axis, candidate) else {
                    losses.push(entity_loss(
                        entry,
                        "sphere reference direction is parallel to its axis",
                    ));
                    continue;
                };
                SurfaceGeometry::Sphere {
                    center: location,
                    axis,
                    ref_direction,
                    radius,
                }
            }
            198 => {
                let Some(axis) = pointer(record, 2)
                    .and_then(|sequence| direction(sequence, &entries, &records, factor))
                    .and_then(|axis| normalized(transform.vector(axis)))
                else {
                    losses.push(entity_loss(entry, "torus axis direction is missing"));
                    continue;
                };
                let radii = [record.number(3), record.number(4)];
                let [Some(major_radius), Some(minor_radius)] = radii else {
                    losses.push(entity_loss(entry, "torus radii are not numeric"));
                    continue;
                };
                let (major_radius, minor_radius) = (major_radius * factor, minor_radius * factor);
                if !major_radius.is_finite()
                    || !minor_radius.is_finite()
                    || minor_radius <= 0.0
                    || minor_radius >= major_radius
                {
                    losses.push(entity_loss(
                        entry,
                        "torus radii do not satisfy 0 < minor < major",
                    ));
                    continue;
                }
                let candidate = (entry.form == 1)
                    .then(|| pointer(record, 5))
                    .flatten()
                    .and_then(|sequence| direction(sequence, &entries, &records, factor))
                    .map(|direction| transform.vector(direction));
                let Some(ref_direction) = reference_direction(axis, candidate) else {
                    losses.push(entity_loss(
                        entry,
                        "torus reference direction is parallel to its axis",
                    ));
                    continue;
                };
                SurfaceGeometry::Torus {
                    center: location,
                    axis,
                    ref_direction,
                    major_radius,
                    minor_radius,
                }
            }
            _ => {
                losses.push(entity_loss(entry, "analytic surface type is unsupported"));
                continue;
            }
        };
        ir.model.surfaces.push(Surface {
            id: SurfaceId(format!("iges:model:surface#D{}", entry.sequence)),
            geometry: result,
            source_object: Some(source_object(entry)),
        });
        decoded.insert(entry.sequence);
    }

    AnalyticSurfaceProjection {
        handled,
        decoded,
        losses,
    }
}
