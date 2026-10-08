// SPDX-License-Identifier: Apache-2.0
//! Surface prototype parameters and first-instance prototype surfaces.

use crate::vecmath::normalize;
use std::collections::BTreeMap;

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{nurbs::NurbsSurface, SolvedSurfaceGeometry, Surface, SurfaceGeometry};
use cadmpeg_ir::ids::SurfaceId;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::{AnnotationBuilder, Exactness, SourceObjectAssociation};

use crate::container::ContainerScan;
use crate::lane_refusal::JoinedLaneRecords;
use crate::legacy_geometry::LegacySurfaceNamespace;
use crate::surface::SurfaceParameterRecord;

use super::super::native::annotate;
use super::super::sweep::nurbs::interpolation_spline_surface;
use crate::vecmath::{cross, dot, local_system_lanes};

const EPS_PROTOTYPE_AGREEMENT: f64 = 1.0e-10;

fn push_prototype_loss(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
    code: crate::loss::CreoLossCode,
    message: impl std::fmt::Display,
) -> Result<(), cadmpeg_core::CodecError> {
    let message = ctx.format_retained(format_args!("{message}"), "creo prototype loss text")?;
    ctx.reserve_vec(losses, 1, "creo prototype losses")?;
    losses.push(code.note(message));
    Ok(())
}

pub(in super::super) fn prototype_scalar(
    record: &crate::surface::SurfacePrototypeRecord,
    name: &str,
) -> Option<f64> {
    match &record.field(name)?.value {
        crate::surface::SurfaceNamedValue::ScalarSequence(values) if values.len() == 1 => {
            Some(values[0])
        }
        _ => None,
    }
}

fn prototype_vector_array(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    record: &crate::surface::SurfacePrototypeRecord,
    name: &str,
) -> Result<Option<Vec<[f64; 3]>>, cadmpeg_core::CodecError> {
    let Some(field) = record.field(name) else {
        return Ok(None);
    };
    let crate::surface::SurfaceNamedValue::ScalarArray(array) = &field.value else {
        return Ok(None);
    };
    if array.count() != 3 {
        return Ok(None);
    }
    let mut triples = Vec::new();
    for coordinates in
        ctx.admit_iter(array.values(), "creo prototype vector array traversal")?
            .chunks(std::num::NonZeroUsize::new(3).ok_or_else(|| {
                cadmpeg_core::CodecError::malformed("zero prototype vector width")
            })?)
            .filter(|coordinates| coordinates.len() == 3)
    {
        let [Some(x), Some(y), Some(z)] = coordinates else {
            return Ok(None);
        };
        ctx.reserve_vec(&mut triples, 1, "creo prototype vector triples")?;
        triples.push([*x, *y, *z]);
    }
    Ok(Some(triples))
}

fn prototype_parameter_array(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    record: &crate::surface::SurfacePrototypeRecord,
    name: &str,
) -> Result<Option<Vec<f64>>, cadmpeg_core::CodecError> {
    let Some(field) = record.field(name) else {
        return Ok(None);
    };
    let crate::surface::SurfaceNamedValue::CountedScalarArray(array) = &field.value else {
        return Ok(None);
    };
    let mut parameters = Vec::new();
    for value in ctx.admit_iter(array.values(), "creo prototype parameter array traversal")? {
        let Some(value) = value else {
            return Ok(None);
        };
        ctx.reserve_vec(&mut parameters, 1, "creo prototype parameter values")?;
        parameters.push(*value);
    }
    Ok(Some(parameters))
}

fn prototype_spline_nurbs(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    record: &crate::surface::SurfacePrototypeRecord,
    refusal: &mut crate::lane_refusal::LaneRefusals,
) -> Result<Option<NurbsSurface>, cadmpeg_core::CodecError> {
    let Some(points) = prototype_vector_array(ctx, record, "i_points")? else {
        return Ok(None);
    };
    let Some(u_parameters) = prototype_parameter_array(ctx, record, "u_params")? else {
        return Ok(None);
    };
    let Some(v_parameters) = prototype_parameter_array(ctx, record, "v_params")? else {
        return Ok(None);
    };
    let Some(u_derivatives) = prototype_vector_array(ctx, record, "end_u_tangts")? else {
        return Ok(None);
    };
    let Some(v_derivatives) = prototype_vector_array(ctx, record, "end_v_tangts")? else {
        return Ok(None);
    };
    let Some(mixed) = prototype_vector_array(ctx, record, "end_uv_deriv")? else {
        return Ok(None);
    };
    let Ok(mixed_derivatives) = <[[f64; 3]; 4]>::try_from(mixed) else {
        return Ok(None);
    };
    let Some(grid) = crate::interpolation_grid::InterpolationGrid::try_new(
        ctx,
        points,
        u_parameters,
        v_parameters,
        u_derivatives,
        v_derivatives,
        mixed_derivatives,
    )?
    else {
        return Ok(None);
    };
    interpolation_spline_surface(
        ctx,
        &grid,
        &format_args!(
            "VisibGeom surface prototype record at offset {}",
            record.offset
        ),
        refusal,
    )
}

fn prototype_local_frame(
    record: &crate::surface::SurfacePrototypeRecord,
) -> Option<([f64; 3], [f64; 3], [f64; 3])> {
    let crate::surface::SurfaceNamedValue::ScalarArray(array) = &record.field("local_sys")?.value
    else {
        return None;
    };
    (array.dimensions() == 4 && array.count() == 3).then_some(())?;
    let values: &[Option<f64>; 12] = array.values().try_into().ok()?;
    let mut slots = [0.0; 12];
    for (slot, value) in slots.iter_mut().zip(values) {
        *slot = (*value)?;
    }
    slots.iter().all(|value| value.is_finite()).then_some(())?;
    let [first, middle, third, origin] = local_system_lanes(slots);
    let first_norm = dot(first, first).sqrt();
    let reference = normalize(first)?;
    let torus = matches!(
        record.family,
        crate::surface::SurfacePrototypeFamily::Torus(_)
    );
    let mut second_candidates =
        [(middle, torus), (third, true)]
            .into_iter()
            .filter_map(|(candidate, eligible)| {
                let candidate_norm = dot(candidate, candidate).sqrt();
                let equal_scale = (first_norm - candidate_norm).abs()
                    <= EPS_PROTOTYPE_AGREEMENT * first_norm.max(candidate_norm);
                eligible
                    .then_some(())
                    .filter(|()| {
                        equal_scale
                            && dot(reference, candidate).abs()
                                <= EPS_PROTOTYPE_AGREEMENT * candidate_norm
                    })
                    .and_then(|()| normalize(candidate))
            });
    let second = second_candidates.next()?;
    second_candidates.next().is_none().then_some(())?;
    let axis = normalize(cross(reference, second))?;
    Some((origin, axis, reference))
}

fn first_instance_surface_row<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    rows: &'a [crate::surface::SurfaceRow],
    frame_start: usize,
    frame_end: usize,
    prototype_offset: usize,
    row_kind: crate::surface::SurfaceKind,
) -> Result<Option<&'a crate::surface::SurfaceRow>, cadmpeg_core::CodecError> {
    let previous = ctx
        .admit_iter(rows, "creo preceding prototype row selection")?
        .filter(|row| {
            row.offset >= frame_start && row.offset < frame_end && row.offset < prototype_offset
        })
        .max_by_key(|row| row.offset);
    if previous.is_some_and(|row| row.kind == row_kind) {
        return Ok(previous);
    }
    Ok(ctx
        .admit_iter(rows, "creo following prototype row selection")?
        .filter(|row| row.offset >= frame_start && row.offset < frame_end)
        .filter(|row| row.offset > prototype_offset && row.kind == row_kind)
        .min_by_key(|row| row.offset))
}

/// Bounds of the complete surface array that holds one prototype record.
///
/// A section whose declared extent runs past the scanned buffer is a refusal
/// naming the section, its declared end, and the buffer length. `Ok(None)`
/// states that no single complete surface array holds the prototype.
pub(super) fn surface_prototype_frame_bounds(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan<'_>,
    section: &crate::container::Section,
    prototype_offset: usize,
) -> Result<Option<(usize, usize)>, cadmpeg_core::CodecError> {
    let section_end = section.end();
    if scan.framing.data.is_empty() {
        return Ok(Some((section.offset(), section_end)));
    }
    let Some(payload) = crate::container::section_region(&scan.framing.data, section) else {
        return Err(cadmpeg_core::CodecError::Malformed(ctx.format_retained(
            format_args!(
                "creo section `{}` declares the region {}..{}, past the scanned file length {}",
                section.name(),
                section.offset(),
                section.end(),
                scan.framing.data.len(),
            ),
            "creo surface prototype frame bounds error",
        )?));
    };
    let Some(relative_prototype_offset) = prototype_offset.checked_sub(section.offset()) else {
        return Ok(None);
    };
    let complete_bounds = crate::surface::complete_surface_array_bounds(ctx, payload)?;
    let mut matches = complete_bounds.into_iter().filter(|(start, end)| {
        relative_prototype_offset >= *start && relative_prototype_offset < *end
    });
    let Some((start, end)) = matches.next() else {
        return Ok(None);
    };
    if matches.next().is_some() {
        return Ok(None);
    }
    Ok(Some((
        frame_bound(ctx, section, start)?,
        frame_bound(ctx, section, end)?,
    )))
}

/// Absolute address of an offset inside one section payload.
fn frame_bound(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    section: &crate::container::Section,
    relative: usize,
) -> Result<usize, cadmpeg_core::CodecError> {
    let Some(bound) = section.offset().checked_add(relative) else {
        return Err(cadmpeg_core::CodecError::Malformed(ctx.format_retained(
            format_args!(
            "section {} states offset {} and a surface array bound at {relative}, which do not \
             form an address",
            section.name(),
            section.offset()
        ),
            "creo surface prototype frame address error",
        )?));
    };
    Ok(bound)
}

#[derive(Clone, Copy)]
pub(in super::super) enum SupportedPrototype<'a> {
    Plane(&'a crate::surface::SurfacePrototypeRecord),
    Cylinder(&'a crate::surface::SurfacePrototypeRecord),
    Cone(&'a crate::surface::SurfacePrototypeRecord),
    Torus(&'a crate::surface::SurfacePrototypeRecord),
    Spline(&'a crate::surface::SurfacePrototypeRecord),
}

impl<'a> SupportedPrototype<'a> {
    pub(in super::super) fn record(self) -> &'a crate::surface::SurfacePrototypeRecord {
        match self {
            Self::Plane(record)
            | Self::Cylinder(record)
            | Self::Cone(record)
            | Self::Torus(record)
            | Self::Spline(record) => record,
        }
    }
}

/// Every surface prototype record that binds to exactly one first-instance row.
///
/// A section whose declared extent runs past the scanned buffer is a refusal,
/// because the prototype frame it states cannot be read.
pub(in super::super) fn unique_surface_prototype_associations<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &'a ContainerScan<'_>,
) -> Result<
    Vec<(
        SupportedPrototype<'a>,
        &'a crate::surface::SurfaceRow,
        &'a crate::container::Section,
    )>,
    cadmpeg_core::CodecError,
> {
    let mut associations = Vec::new();
    for record in ctx.admit_iter(
        &scan.surfaces.prototype_records,
        "creo unique surface prototype associations prototype records traversal",
    )? {
        let (prototype, row_kind) = match record.family {
            crate::surface::SurfacePrototypeFamily::Plane => (
                SupportedPrototype::Plane(record),
                crate::surface::SurfaceKind::Plane,
            ),
            crate::surface::SurfacePrototypeFamily::Cylinder => (
                SupportedPrototype::Cylinder(record),
                crate::surface::SurfaceKind::Cylinder,
            ),
            crate::surface::SurfacePrototypeFamily::Cone => (
                SupportedPrototype::Cone(record),
                crate::surface::SurfaceKind::Cone,
            ),
            crate::surface::SurfacePrototypeFamily::Torus(_) => (
                SupportedPrototype::Torus(record),
                crate::surface::SurfaceKind::TorusOrSphere,
            ),
            crate::surface::SurfacePrototypeFamily::Spline(_) => (
                SupportedPrototype::Spline(record),
                crate::surface::SurfaceKind::Spline,
            ),
            _ => continue,
        };
        let Some(section) = ctx.find_by(
            &scan.framing.sections,
            |section| Ok(section.contains(record.offset)),
            "creo prototype section search",
        )?
        else {
            continue;
        };
        let Some((adjacent_start, adjacent_end)) =
            surface_prototype_frame_bounds(ctx, scan, section, record.offset)?
        else {
            continue;
        };
        let Some(row) = first_instance_surface_row(
            ctx,
            &scan.surfaces.rows,
            adjacent_start,
            adjacent_end,
            record.offset,
            row_kind,
        )?
        else {
            continue;
        };
        if crate::surface::unique_surface_row(&scan.surfaces.rows, row.id)
            .is_none_or(|unique| unique.offset != row.offset)
        {
            continue;
        }
        ctx.reserve_vec(&mut associations, 1, "creo surface prototype associations")?;
        associations.push((prototype, row, section));
    }
    let mut association_counts = BTreeMap::<usize, usize>::new();
    for (_, row, _) in ctx.admit_iter(
        &associations,
        "creo unique surface prototype associations associations traversal",
    )? {
        let count = ctx
            .entry_btree_map(
                &mut association_counts,
                row.offset,
                "creo surface prototype row counts",
            )?
            .or_default();
        *count += 1;
    }
    ctx.retain_vec(
        &mut associations,
        |(_, row, _)| Ok(ctx.get_btree_map(&association_counts, &row.offset, "creo association counts lookup")? == Some(&1)),
        "creo unique surface prototype associations retention",
    )?;
    Ok(associations)
}

/// Transfer one exact surface carrier per first-instance prototype record.
///
/// A prototype spline whose lanes the IR carrier refuses states no carrier.
/// The model carries the surface row without it, so the refusal is a loss note
/// naming the `VisibGeom` surface row and the prototype offset.
pub(in super::super) fn transfer_first_instance_prototype_surfaces(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
    source_carriers: &mut crate::decode::source_carriers::SourceUnitCarriers,
) -> Result<usize, cadmpeg_core::CodecError> {
    let mut surfaces_index = super::model_ids::ModelIdentityIndex::new(ctx)?;
    if !matches!(scan.framing.layout, crate::container::Layout::Nd) {
        return Ok(0);
    }
    let mut transferred = 0;
    let associations = unique_surface_prototype_associations(ctx, scan)?;
    for (prototype, row, section) in ctx
        .admit_iter(&associations, "creo prototype association traversal")?
        .copied()
    {
        let record = prototype.record();
        let geometry = match prototype {
            SupportedPrototype::Plane(_) => {
                let Some((origin, axis, reference)) = prototype_local_frame(record) else {
                    continue;
                };
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                    match cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                        Point3::from(origin),
                        Vector3::from(axis),
                        Vector3::from(reference),
                    ) {
                        Ok(payload) => payload,
                        Err(_) => continue,
                    },
                ))
            }
            SupportedPrototype::Cylinder(_) => {
                let Some((origin, axis, reference)) = prototype_local_frame(record) else {
                    continue;
                };
                let Some(radius) = prototype_scalar(record, "radius")
                    .filter(|radius| radius.is_finite() && *radius > 0.0)
                else {
                    continue;
                };
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
                    match cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
                        Point3::from(origin),
                        Vector3::from(axis),
                        Vector3::from(reference),
                        radius,
                    ) {
                        Ok(payload) => payload,
                        Err(_) => continue,
                    },
                ))
            }
            SupportedPrototype::Torus(_) => {
                let Some((origin, axis, reference)) = prototype_local_frame(record) else {
                    continue;
                };
                let point = Point3::from(origin);
                let axis = Vector3::from(axis);
                let reference = Vector3::from(reference);
                let prototype_radii = match (
                    prototype_scalar(record, "radius1")
                        .filter(|radius| radius.is_finite() && *radius >= 0.0),
                    prototype_scalar(record, "radius2")
                        .filter(|radius| radius.is_finite() && *radius > 0.0),
                ) {
                    (Some(radius1), Some(radius2)) => Some([radius1, radius2]),
                    _ => None,
                };
                let radii =
                    crate::surface::unique_surface_parameter(&scan.surfaces.parameters, row.id)
                        .filter(|parameter| parameter.offset == row.offset)
                        .and_then(SurfaceParameterRecord::torus_radius_overrides)
                        .map(|overrides| [overrides.radius1, overrides.radius2])
                        .or(prototype_radii);
                let Some([radius1, radius2]) = radii else {
                    continue;
                };
                if radius1 == 0.0 {
                    SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(
                        match cadmpeg_ir::geometry::analytic::SphereSurface::try_new(
                            point, axis, reference, radius2,
                        ) {
                            Ok(payload) => payload,
                            Err(_) => continue,
                        },
                    ))
                } else {
                    SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(
                        match cadmpeg_ir::geometry::analytic::TorusSurface::try_new(
                            point, axis, reference, radius1, radius2,
                        ) {
                            Ok(payload) => payload,
                            Err(_) => continue,
                        },
                    ))
                }
            }
            SupportedPrototype::Cone(_) => {
                let Some(frame) = crate::surface::prototype_cone_frame(record) else {
                    continue;
                };
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(super::apex_cone(
                    frame.frame(),
                    frame.half_angle(),
                )))
            }
            SupportedPrototype::Spline(_) => {
                let mut refusal = crate::lane_refusal::LaneRefusals::new();
                let nurbs = prototype_spline_nurbs(ctx, record, &mut refusal)?;
                let refused = refusal.take_records_checked()?;
                let Some(nurbs) = nurbs.filter(|_| refused.is_empty()) else {
                    if !refused.is_empty() {
                        push_prototype_loss(
                            ctx,
                            losses,
                            crate::loss::CreoLossCode::VisibGeomSurfaceUntransferred,
                            format_args!(
                                "VisibGeom surface row {} states a spline prototype at offset {} \
                                 that forms no NURBS carrier: {}",
                                row.id,
                                record.offset,
                                JoinedLaneRecords(&refused)
                            ),
                        )?;
                    }
                    continue;
                };
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(nurbs))
            }
        };
        let id = crate::identity::compose_checked::<SurfaceId>(
            ctx,
            &crate::identity::VISIBGEOM_SURFACE,
            row.id,
            "creo decoded model identity",
        )?;
        let identity_present = surfaces_index.lookup(ctx, &ir.model.surfaces, |record| record.id.as_str(), id.as_str())?.is_some();
        if identity_present {
            continue;
        }
        annotate(
            ctx,
            annotations,
            &id,
            section.name(),
            cadmpeg_core::decode::u64_from_index(record.offset),
            "first_instance_surface_prototype",
            Exactness::Derived,
        )?;
        ctx.charge_entities(1, "admit Creo model surfaces")?;
        source_carriers.admit_surface(
            ctx,
            ir,
            Surface {
                id,
                geometry,
                source_object: Some(SourceObjectAssociation {
                    format: cadmpeg_ir::CodecFormat::Creo,
                    object_id: crate::identity::source_object_id_checked(
                        ctx,
                        format_args!("{}:{}", section.name(), row.id),
                        "creo source object identity",
                    )?,
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            },
        )?;
        transferred += 1;
    }
    Ok(transferred)
}

/// Copy section-relative surface rows under the caller's collection budget.
fn relative_surface_rows(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    rows: &[crate::surface::SurfaceRow],
    section: &crate::container::Section,
) -> Result<Vec<crate::surface::SurfaceRow>, cadmpeg_core::CodecError> {
    let mut relative = Vec::new();
    for candidate in ctx
        .admit_iter(rows, "creo relative surface row traversal")?
        .filter(|row| section.contains(row.offset))
    {
        let Some(offset) = candidate.offset.checked_sub(section.offset()) else {
            continue;
        };
        ctx.reserve_vec(&mut relative, 1, "creo positional replay section rows")?;
        let mut row = candidate.clone();
        row.offset = offset;
        relative.push(row);
    }
    Ok(relative)
}

/// Transfer one exact surface carrier per positional spline replay.
///
/// A section whose declared extent runs past the scanned buffer is a refusal.
/// A replay whose lanes the IR carrier refuses states no carrier: the model
/// carries the surface row without it, so that refusal is a loss note naming
/// the `VisibGeom` surface row and the parameter body offset.
pub(in super::super) fn transfer_positional_spline_replays(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
    source_carriers: &mut crate::decode::source_carriers::SourceUnitCarriers,
) -> Result<usize, cadmpeg_core::CodecError> {
    let mut surfaces_index = super::model_ids::ModelIdentityIndex::new(ctx)?;
    if !matches!(scan.framing.layout, crate::container::Layout::Nd) {
        return Ok(0);
    }
    let mut transferred = 0;
    for parameter in ctx.admit_iter(
        &*scan.surfaces.parameters,
        "creo transfer positional spline replays parameters traversal",
    )? {
        if parameter.boundary != crate::surface::SurfaceBodyBoundary::CompoundClose {
            continue;
        }
        let Some(row) =
            crate::surface::unique_surface_row(&scan.surfaces.rows, parameter.surface_id)
        else {
            continue;
        };
        if row.kind != crate::surface::SurfaceKind::Spline || row.offset != parameter.offset {
            continue;
        }
        let mut sections = scan
            .framing
            .sections
            .iter()
            .filter(|section| section.contains(row.offset));
        let Some(section) = sections.next() else {
            continue;
        };
        if sections.next().is_some() {
            continue;
        }
        let Some(payload) = crate::container::section_region(&scan.framing.data, section) else {
            continue;
        };
        let Some(relative_row_offset) = row.offset.checked_sub(section.offset()) else {
            continue;
        };
        let relative_row = {
            let mut row = row.clone();
            row.offset = relative_row_offset;
            row
        };
        let relative_rows = relative_surface_rows(ctx, &scan.surfaces.rows, section)?;
        let Some(prototype) = crate::surface::positional_spline_replay_prototype(
            ctx,
            payload,
            &relative_rows,
            &relative_row,
        )?
        else {
            continue;
        };
        let cache = crate::scalar::ScalarCache::from_section_checked(ctx, payload)?;
        let Some(replay) = crate::surface::decode_positional_spline_replay(
            ctx,
            &parameter.body,
            &prototype,
            &cache,
        )?
        else {
            continue;
        };
        let mut refusal = crate::lane_refusal::LaneRefusals::new();
        let nurbs = interpolation_spline_surface(
            ctx,
            &replay,
            &format_args!(
                "VisibGeom surface row {} positional spline replay at offset {}",
                row.id, parameter.body_offset
            ),
            &mut refusal,
        )?;
        let refused = refusal.take_records_checked()?;
        let Some(nurbs) = nurbs.filter(|_| refused.is_empty()) else {
            if !refused.is_empty() {
                push_prototype_loss(
                    ctx,
                    losses,
                    crate::loss::CreoLossCode::VisibGeomSurfaceUntransferred,
                    format_args!(
                        "VisibGeom surface row {} states a positional spline replay at offset {} \
                         that forms no NURBS carrier: {}",
                        row.id,
                        parameter.body_offset,
                        JoinedLaneRecords(&refused)
                    ),
                )?;
            }
            continue;
        };
        let id = crate::identity::compose_checked::<SurfaceId>(
            ctx,
            &crate::identity::VISIBGEOM_SURFACE,
            row.id,
            "creo decoded model identity",
        )?;
        let identity_present = surfaces_index.lookup(ctx, &ir.model.surfaces, |record| record.id.as_str(), id.as_str())?.is_some();
        if identity_present {
            continue;
        }
        annotate(
            ctx,
            annotations,
            &id,
            section.name(),
            cadmpeg_core::decode::u64_from_index(parameter.body_offset),
            "positional_spline_prototype_replay",
            Exactness::Derived,
        )?;
        ctx.charge_entities(1, "admit Creo model surfaces")?;
        source_carriers.admit_surface(
            ctx,
            ir,
            Surface {
                id,
                geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(nurbs)),
                source_object: Some(SourceObjectAssociation {
                    format: cadmpeg_ir::CodecFormat::Creo,
                    object_id: crate::identity::source_object_id_checked(
                        ctx,
                        format_args!("{}:{}", section.name(), row.id),
                        "creo source object identity",
                    )?,
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            },
        )?;
        transferred += 1;
    }
    Ok(transferred)
}

fn legacy_carrier_counts<T>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    source: &[T],
    id: impl Fn(&T) -> u32,
) -> Result<BTreeMap<u32, usize>, cadmpeg_core::CodecError> {
    let mut counts = BTreeMap::new();
    for item in ctx.admit_iter(source, "creo legacy carrier count traversal")? {
        *ctx.entry_btree_map(&mut counts, id(item), "creo legacy carrier count nodes")?
            .or_insert(0) += 1;
    }
    Ok(counts)
}

/// Transfer one exact surface carrier per unique legacy ASCII carrier record.
///
/// A carrier spline whose lanes the IR carrier refuses states no carrier. The
/// model carries the surface row without it, so the refusal is a loss note
/// naming the surface row and the carrier offset.
pub(in super::super) fn transfer_legacy_ascii_surface_carriers(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
    source_carriers: &mut crate::decode::source_carriers::SourceUnitCarriers,
) -> Result<usize, cadmpeg_core::CodecError> {
    let mut surfaces_index = super::model_ids::ModelIdentityIndex::new(ctx)?;
    if !matches!(
        scan.framing.layout,
        crate::container::Layout::LegacyAscii(_)
    ) {
        return Ok(0);
    }
    let carrier_counts = legacy_carrier_counts(ctx, &scan.surfaces.legacy_carriers, |carrier| {
        carrier.surface_id
    })?;

    let mut transferred = 0;
    for carrier in ctx.admit_iter(
        &scan.surfaces.legacy_carriers,
        "creo transfer legacy ascii surface carriers legacy carriers traversal",
    )? {
        if ctx.get_btree_map(
            &carrier_counts,
            &carrier.surface_id,
            "creo legacy carrier count lookup",
        )? != Some(&1)
        {
            continue;
        }
        let rows = match carrier.namespace {
            LegacySurfaceNamespace::Visible => &scan.surfaces.rows,
            LegacySurfaceNamespace::NonVisible => &scan.surfaces.nonvisible_rows,
        };
        let Some(row) = crate::surface::unique_surface_row(rows, carrier.surface_id) else {
            continue;
        };
        let geometry = match &carrier.geometry {
            crate::legacy_geometry::LegacySurfaceGeometry::Plane { frame }
                if row.kind == crate::surface::SurfaceKind::Plane =>
            {
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                    cadmpeg_ir::geometry::analytic::PlaneSurface::new(
                        frame.finite_origin(),
                        frame.orthonormal_frame(),
                    ),
                ))
            }
            crate::legacy_geometry::LegacySurfaceGeometry::Cylinder { frame, radius }
                if row.kind == crate::surface::SurfaceKind::Cylinder =>
            {
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
                    cadmpeg_ir::geometry::analytic::CylinderSurface::new(
                        frame.finite_origin(),
                        frame.orthonormal_frame(),
                        *radius,
                    ),
                ))
            }
            crate::legacy_geometry::LegacySurfaceGeometry::Cone {
                frame, half_angle, ..
            } if row.kind == crate::surface::SurfaceKind::Cone => SurfaceGeometry::Solved(
                SolvedSurfaceGeometry::Cone(super::apex_cone(frame, *half_angle)),
            ),
            crate::legacy_geometry::LegacySurfaceGeometry::Torus {
                frame,
                major_radius,
                minor_radius,
            } if row.kind == crate::surface::SurfaceKind::TorusOrSphere => SurfaceGeometry::Solved(
                SolvedSurfaceGeometry::Torus(cadmpeg_ir::geometry::analytic::TorusSurface::new(
                    frame.finite_origin(),
                    frame.orthonormal_frame(),
                    *major_radius,
                    (*minor_radius).into(),
                )),
            ),
            crate::legacy_geometry::LegacySurfaceGeometry::Sphere { frame, radius }
                if row.kind == crate::surface::SurfaceKind::TorusOrSphere =>
            {
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(
                    cadmpeg_ir::geometry::analytic::SphereSurface::new(
                        frame.finite_origin(),
                        frame.orthonormal_frame(),
                        (*radius).into(),
                    ),
                ))
            }
            crate::legacy_geometry::LegacySurfaceGeometry::Spline(spline)
                if row.kind == crate::surface::SurfaceKind::Spline =>
            {
                let mut refusal = crate::lane_refusal::LaneRefusals::new();
                let nurbs = interpolation_spline_surface(
                    ctx,
                    spline,
                    &format_args!(
                        "legacy {}{} spline carrier at offset {}",
                        carrier.namespace.source_prefix(),
                        carrier.surface_id,
                        carrier.offset
                    ),
                    &mut refusal,
                )?;
                let refused = refusal.take_records_checked()?;
                let Some(nurbs) = nurbs.filter(|_| refused.is_empty()) else {
                    if !refused.is_empty() {
                        push_prototype_loss(
                            ctx,
                            losses,
                            crate::loss::CreoLossCode::LegacySurfaceCarrierUnresolved,
                            format_args!(
                                "{}{} states a legacy spline carrier at offset {} that forms no \
                                 NURBS carrier: {}",
                                carrier.namespace.source_prefix(),
                                carrier.surface_id,
                                carrier.offset,
                                JoinedLaneRecords(&refused)
                            ),
                        )?;
                    }
                    continue;
                };
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(nurbs))
            }
            _ => continue,
        };
        let namespace = match carrier.namespace {
            LegacySurfaceNamespace::Visible => &crate::identity::VISIBGEOM_SURFACE,
            LegacySurfaceNamespace::NonVisible => &crate::identity::NOVISGEOM_SURFACE,
        };
        let id = crate::identity::compose_checked::<SurfaceId>(
            ctx,
            namespace,
            carrier.surface_id,
            "creo decoded model identity",
        )?;
        let identity_present = surfaces_index.lookup(ctx, &ir.model.surfaces, |record| record.id.as_str(), id.as_str())?.is_some();
        if identity_present {
            continue;
        }
        annotate(
            ctx,
            annotations,
            &id,
            "legacy_ascii",
            cadmpeg_core::decode::u64_from_index(carrier.offset),
            "legacy_surface_prototype_carrier",
            Exactness::Derived,
        )?;
        ctx.charge_entities(1, "admit Creo model surfaces")?;
        source_carriers.admit_surface(
            ctx,
            ir,
            Surface {
                id,
                geometry,
                source_object: Some(SourceObjectAssociation {
                    format: cadmpeg_ir::CodecFormat::Creo,
                    object_id: crate::identity::source_object_id_checked(
                        ctx,
                        format_args!(
                            "{}{}",
                            carrier.namespace.source_prefix(),
                            carrier.surface_id
                        ),
                        "creo source object identity",
                    )?,
                    name: None,
                    color: None,
                    visible: Some(carrier.namespace.is_visible()),
                    layer: None,
                    instance_path: Vec::new(),
                }),
            },
        )?;
        transferred += 1;
    }
    Ok(transferred)
}

#[cfg(test)]
mod tests;
