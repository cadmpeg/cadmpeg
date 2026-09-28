// SPDX-License-Identifier: Apache-2.0
//! Pointer-defined analytic surface projection.

use super::geometry::{resolve_transform, source_object, ProjectionOutcome, TransformResolutionError};
use super::pointer;
use super::push_optional_entity_loss;
use crate::decode_resource::{
    insert_optional_btree_map, insert_optional_btree_set, reserve_optional_vec_growth,
};
use crate::directory::DirectoryEntry;
use crate::global::ProjectedGlobal;
use crate::parameter::ParameterRecord;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::FiniteVector3;
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, Surface, SurfaceGeometry};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::{Angle, NonNegativeLength, NonZeroLength, PositiveLength, PositiveReal};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::units::{OrthonormalFrame3, UnitVector3};
use cadmpeg_ir::CadIr;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

fn admit_analytic<T>(
    result: Result<T, &str>,
    entry: &DirectoryEntry,
    losses: &mut Vec<LossNote>,
    ctx: Option<&DecodeContext<'_>>,
) -> Result<Option<T>, CodecError> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(message) => {
            push_optional_entity_loss(ctx, losses, entry, format_args!("{message}"))?;
            Ok(None)
        }
    }
}

fn point(ir: &CadIr, sequence: u32) -> Option<Point3> {
    let mut storage = [0_u8; 64];
    let id = crate::ids::directory_lookup_key("iges:model:point#D", sequence, &mut storage)?;
    ir.model
        .points
        .iter()
        .find(|point| point.id.as_str() == id)
        .map(|point| point.position().get())
}

#[derive(Debug)]
enum DirectionError {
    MissingEntry(u32),
    WrongTypeForm { sequence: u32, entity_type: i64, form: i64 },
    NotDependent(u32),
    Transformed(u32),
    MissingParameters(u32),
    NonNumeric(u32),
    ZeroOrNonFinite(u32),
}

impl fmt::Display for DirectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingEntry(sequence) => write!(formatter, "points to missing Directory entry D{sequence}"),
            Self::WrongTypeForm { sequence, entity_type, form } => write!(formatter, "points to type {entity_type} form {form} at D{sequence}, not type 123 form 0"),
            Self::NotDependent(sequence) => write!(formatter, "points to D{sequence}, which is not physically dependent"),
            Self::Transformed(sequence) => write!(formatter, "points to D{sequence}, which has a prohibited transformation"),
            Self::MissingParameters(sequence) => write!(formatter, "points to D{sequence}, whose Parameter Data record is missing"),
            Self::NonNumeric(sequence) => write!(formatter, "points to D{sequence}, whose direction components are not numeric"),
            Self::ZeroOrNonFinite(sequence) => write!(formatter, "points to D{sequence}, whose direction is zero or non-finite"),
        }
    }
}

#[derive(Debug)]
enum AnalyticDirectionError {
    MissingPointer(&'static str),
    Pointed { role: &'static str, reason: DirectionError },
    Collapse(&'static str),
    SphereAxisCollapse,
}

impl fmt::Display for AnalyticDirectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingPointer(role) => write!(formatter, "{role} pointer is missing, even, or non-integer"),
            Self::Pointed { role, reason } => write!(formatter, "{role} {reason}"),
            Self::Collapse(role) => write!(formatter, "{role} collapses under the surface transformation"),
            Self::SphereAxisCollapse => formatter.write_str("sphere axis collapses under its transformation"),
        }
    }
}

#[allow(clippy::many_single_char_names)]
fn direction(
    sequence: u32,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    records: &BTreeMap<u32, &ParameterRecord>,
) -> Result<UnitVector3, DirectionError> {
    let entry = entries
        .get(&sequence)
        .copied()
        .ok_or(DirectionError::MissingEntry(sequence))?;
    if entry.entity_type != 123 || entry.form != 0 {
        return Err(DirectionError::WrongTypeForm { sequence, entity_type: entry.entity_type, form: entry.form });
    }
    if !entry.status.is_physically_dependent() {
        return Err(DirectionError::NotDependent(sequence));
    }
    if entry.transform != 0 {
        return Err(DirectionError::Transformed(sequence));
    }
    let record = records
        .get(&sequence)
        .copied()
        .ok_or(DirectionError::MissingParameters(sequence))?;
    let components = [record.number(1), record.number(2), record.number(3)];
    let [Some(x), Some(y), Some(z)] = components else {
        return Err(DirectionError::NonNumeric(sequence));
    };
    FiniteVector3::new(Vector3::new(x, y, z))
        .and_then(UnitVector3::normalized_nonzero)
        .ok_or(DirectionError::ZeroOrNonFinite(sequence))
}

fn required_direction(
    record: &ParameterRecord,
    index: usize,
    role: &'static str,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    records: &BTreeMap<u32, &ParameterRecord>,
) -> Result<UnitVector3, AnalyticDirectionError> {
    let sequence = pointer(record, index)
        .ok_or(AnalyticDirectionError::MissingPointer(role))?;
    direction(sequence, entries, records).map_err(|reason| AnalyticDirectionError::Pointed { role, reason })
}

fn transformed_direction(
    record: &ParameterRecord,
    index: usize,
    role: &'static str,
    transform: Transform,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    records: &BTreeMap<u32, &ParameterRecord>,
) -> Result<UnitVector3, AnalyticDirectionError> {
    let direction = required_direction(record, index, role, entries, records)?;
    transform
        .apply_vector(*direction.as_raw())
        .and_then(UnitVector3::normalized_nonzero)
        .ok_or(AnalyticDirectionError::Collapse(role))
}

fn form_reference_direction(
    form: i64,
    record: &ParameterRecord,
    index: usize,
    role: &'static str,
    transform: Transform,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    records: &BTreeMap<u32, &ParameterRecord>,
) -> Result<Option<UnitVector3>, AnalyticDirectionError> {
    if form == 0 {
        Ok(None)
    } else {
        transformed_direction(record, index, role, transform, entries, records).map(Some)
    }
}

enum ReferenceDirectionError {
    Parallel,
    NotUnit,
}

fn reference_direction(
    axis: UnitVector3,
    candidate: Option<UnitVector3>,
) -> Result<UnitVector3, ReferenceDirectionError> {
    match candidate {
        Some(candidate) => {
            let axis_raw = *axis.as_raw();
            let candidate_raw = *candidate.as_raw();
            let v = candidate_raw - axis_raw.scale(axis_raw.dot(candidate_raw));
            let n = v.norm();
            if !n.is_finite() || n <= 0.0 {
                return Err(ReferenceDirectionError::Parallel);
            }
            UnitVector3::normalized_by_reciprocal(v).ok_or(ReferenceDirectionError::NotUnit)
        }
        None => Ok(axis.derived_reference()),
    }
}

fn reference_frame(
    axis: UnitVector3,
    candidate: Option<UnitVector3>,
    parallel_message: &'static str,
    frame_message: &'static str,
) -> Result<OrthonormalFrame3, &'static str> {
    let reference = reference_direction(axis, candidate).map_err(|error| match error {
        ReferenceDirectionError::Parallel => parallel_message,
        ReferenceDirectionError::NotUnit => frame_message,
    })?;
    OrthonormalFrame3::from_units(axis, reference).ok_or(frame_message)
}

fn surface_transform(
    entry: &DirectoryEntry,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    records: &BTreeMap<u32, &ParameterRecord>,
    global: &ProjectedGlobal,
    ctx: Option<&DecodeContext<'_>>,
) -> Result<Transform, TransformResolutionError> {
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
    sequences: &mut super::geometry::SourceSequences,
) -> Result<ProjectionOutcome, CodecError> {
    let mut records = BTreeMap::new();
    for record in parameters {
        insert_optional_btree_map(
            ctx, &mut records, record.directory_sequence, record,
            "iges analytic-surface parameter index",
        )?;
    }
    let mut entries = BTreeMap::new();
    for entry in directory {
        insert_optional_btree_map(
            ctx, &mut entries, entry.sequence, entry,
            "iges analytic-surface directory index",
        )?;
    }
    let mut decoded = BTreeSet::new();
    let mut losses = Vec::new();

    for entry in directory.iter().filter(|entry| {
        matches!(entry.entity_type, 190 | 192 | 194 | 196 | 198) && matches!(entry.form, 0 | 1)
    }) {
        let factor = global.length_factor_mm();
        let Some(record) = records.get(&entry.sequence).copied() else {
            push_optional_entity_loss(ctx, &mut losses, entry, format_args!("Parameter Data record is missing"))?;
            continue;
        };
        let transform = match surface_transform(entry, &entries, &records, global, ctx) {
            Ok(transform) => transform,
            Err(error) => {
                let message = error.non_resource()?;
                push_optional_entity_loss(ctx, &mut losses, entry, format_args!("{message}"))?;
                continue;
            }
        };
        let location_index = pointer(record, 1);
        let Some(location) = location_index.and_then(|sequence| point(ir, sequence)) else {
            push_optional_entity_loss(ctx, &mut losses, entry, format_args!("analytic surface location point is missing"))?;
            continue;
        };
        let Some(location) = transform.apply_point(location) else {
            push_optional_entity_loss(ctx, &mut losses, entry, format_args!("placement produces a non-finite point"))?;
            continue;
        };
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
                        push_optional_entity_loss(ctx, &mut losses, entry, format_args!("{message}"))?;
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
                        push_optional_entity_loss(ctx, &mut losses, entry, format_args!("{message}"))?;
                        continue;
                    }
                };
                let Some(frame) = admit_analytic(
                    reference_frame(
                        axis,
                        candidate,
                        "plane reference direction is parallel to its normal",
                        "PlaneSurface.normal/u_axis must form an orthonormal frame",
                    ),
                    entry,
                    &mut losses,
                ctx,
                )? else {
                    continue;
                };
                let payload = cadmpeg_ir::geometry::analytic::PlaneSurface::new(location, frame);
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(payload))
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
                        push_optional_entity_loss(ctx, &mut losses, entry, format_args!("{message}"))?;
                        continue;
                    }
                };
                let Some(radius) = record.number(3).map(|radius| radius * factor) else {
                    push_optional_entity_loss(ctx, &mut losses, entry, format_args!("cylinder radius is not numeric"))?;
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
                        push_optional_entity_loss(ctx, &mut losses, entry, format_args!("{message}"))?;
                        continue;
                    }
                };
                let Some(frame) = admit_analytic(
                    reference_frame(
                        axis,
                        candidate,
                        "cylinder reference direction is parallel to its axis",
                        "CylinderSurface.axis/ref_direction must form an orthonormal frame",
                    ),
                    entry,
                    &mut losses,
                ctx,
                )? else {
                    continue;
                };
                let Some(radius) = admit_analytic(
                    PositiveLength::new(radius)
                        .ok_or("CylinderSurface.radius must be positive and finite"),
                    entry,
                    &mut losses,
                ctx,
                )? else {
                    continue;
                };
                let payload =
                    cadmpeg_ir::geometry::analytic::CylinderSurface::new(location, frame, radius);
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(payload))
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
                        push_optional_entity_loss(ctx, &mut losses, entry, format_args!("{message}"))?;
                        continue;
                    }
                };
                let Some(radius) = record.number(3).map(|radius| radius * factor) else {
                    push_optional_entity_loss(ctx, &mut losses, entry, format_args!("cone radius is not numeric"))?;
                    continue;
                };
                let Some(half_angle) = record
                    .number(4)
                    .map(f64::to_radians)
                    .and_then(Angle::new)
                    .filter(|angle| angle.get() > 0.0 && angle.get() < std::f64::consts::FRAC_PI_2)
                else {
                    push_optional_entity_loss(ctx, &mut losses, entry, format_args!("cone semi-angle is outside (0, 90) degrees"))?;
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
                        push_optional_entity_loss(ctx, &mut losses, entry, format_args!("{message}"))?;
                        continue;
                    }
                };
                let Some(frame) = admit_analytic(
                    reference_frame(
                        axis,
                        candidate,
                        "cone reference direction is parallel to its axis",
                        "ConeSurface.axis/ref_direction must form an orthonormal frame",
                    ),
                    entry,
                    &mut losses,
                ctx,
                )? else {
                    continue;
                };
                let Some(radius) = admit_analytic(
                    NonNegativeLength::new(radius)
                        .ok_or("ConeSurface.radius must be nonnegative and finite"),
                    entry,
                    &mut losses,
                ctx,
                )? else {
                    continue;
                };
                let payload = cadmpeg_ir::geometry::analytic::ConeSurface::new(
                    location,
                    frame,
                    radius,
                    PositiveReal::ONE,
                    half_angle,
                );
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(payload))
            }
            196 => {
                let Some(radius) = record
                    .number(2)
                    .map(|radius| radius * factor)
                    .and_then(cadmpeg_ir::scalar::PositiveLength::new)
                else {
                    push_optional_entity_loss(ctx, &mut losses, entry, format_args!("sphere radius is not positive and finite"))?;
                    continue;
                };
                let axis = if entry.form == 1 {
                    transformed_direction(record, 3, "sphere axis", transform, &entries, &records)
                } else {
                    transform
                        .apply_vector(Vector3::new(0.0, 0.0, 1.0))
                        .and_then(UnitVector3::normalized_nonzero)
                        .ok_or(AnalyticDirectionError::SphereAxisCollapse)
                };
                let axis = match axis {
                    Ok(axis) => axis,
                    Err(message) => {
                        push_optional_entity_loss(ctx, &mut losses, entry, format_args!("{message}"))?;
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
                        push_optional_entity_loss(ctx, &mut losses, entry, format_args!("{message}"))?;
                        continue;
                    }
                };
                let Some(frame) = admit_analytic(
                    reference_frame(
                        axis,
                        candidate,
                        "sphere reference direction is parallel to its axis",
                        "SphereSurface.axis/ref_direction must form an orthonormal frame",
                    ),
                    entry,
                    &mut losses,
                ctx,
                )? else {
                    continue;
                };
                let payload = cadmpeg_ir::geometry::analytic::SphereSurface::new(
                    location,
                    frame,
                    NonZeroLength::from(radius),
                );
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(payload))
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
                        push_optional_entity_loss(ctx, &mut losses, entry, format_args!("{message}"))?;
                        continue;
                    }
                };
                let radii = [record.number(3), record.number(4)];
                let [Some(major_radius), Some(minor_radius)] = radii else {
                    push_optional_entity_loss(ctx, &mut losses, entry, format_args!("torus radii are not numeric"))?;
                    continue;
                };
                let (major_radius, minor_radius) = (major_radius * factor, minor_radius * factor);
                if minor_radius <= 0.0 || minor_radius >= major_radius {
                    push_optional_entity_loss(ctx, &mut losses, entry, format_args!("torus radii do not satisfy 0 < minor < major"))?;
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
                        push_optional_entity_loss(ctx, &mut losses, entry, format_args!("{message}"))?;
                        continue;
                    }
                };
                let Some(frame) = admit_analytic(
                    reference_frame(
                        axis,
                        candidate,
                        "torus reference direction is parallel to its axis",
                        "TorusSurface.axis/ref_direction must form an orthonormal frame",
                    ),
                    entry,
                    &mut losses,
                ctx,
                )? else {
                    continue;
                };
                let Some(major_radius) = admit_analytic(
                    PositiveLength::new(major_radius)
                        .ok_or("TorusSurface.major_radius must be positive and finite"),
                    entry,
                    &mut losses,
                ctx,
                )? else {
                    continue;
                };
                let Some(minor_radius) = admit_analytic(
                    NonZeroLength::new(minor_radius)
                        .ok_or("TorusSurface.minor_radius must be finite and nonzero"),
                    entry,
                    &mut losses,
                ctx,
                )? else {
                    continue;
                };
                let payload = cadmpeg_ir::geometry::analytic::TorusSurface::new(
                    location,
                    frame,
                    major_radius,
                    minor_radius,
                );
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(payload))
            }
            _ => {
                push_optional_entity_loss(ctx, &mut losses, entry, format_args!("analytic surface type is unsupported"))?;
                continue;
            }
        };
        sequences.record_surface(
            &crate::ids::surface_admitted(&crate::ids::Stem::directory(entry.sequence), ctx)?,
            entry.sequence, ctx)?;
        reserve_optional_vec_growth(ctx, &mut ir.model.surfaces, 1, "iges analytic-surface slots")?;
        crate::decode_resource::admit_optional_entities(ctx, 1, "iges_geometry_analytic_surfaces")?;
        ir.model.surfaces.push(Surface {
            id: crate::ids::surface_admitted(&crate::ids::Stem::directory(entry.sequence), ctx)?,
            geometry: result,
            source_object: Some(match source_object(entry, ctx) {
                Ok(source) => source,
                Err(error) => {
                    let message = super::non_resource_error(error, ctx)?;
                    push_optional_entity_loss(ctx, &mut losses, entry, format_args!("{message}"))?;
                    continue;
                }
            }),
        });
        insert_optional_btree_set(ctx, &mut decoded, entry.sequence, "iges analytic-surface decoded sequences")?;
    }

    Ok(ProjectionOutcome { decoded, losses })
}

#[cfg(test)]
mod tests;
