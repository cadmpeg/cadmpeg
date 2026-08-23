// SPDX-License-Identifier: Apache-2.0
//! Surface prototype parameters and first-instance prototype surfaces.

use std::collections::BTreeMap;

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{NurbsSurface, Surface, SurfaceGeometry};
use cadmpeg_ir::ids::SurfaceId;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::{AnnotationBuilder, Exactness, SourceObjectAssociation};

use crate::container::ContainerScan;

use super::super::analytic::{cross, dot};
use super::super::native::annotate;
use super::super::sketch::normalized;
use super::super::sweep::interpolation_spline_surface;

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

pub(in super::super) fn prototype_vector_array(
    record: &crate::surface::SurfacePrototypeRecord,
    name: &str,
) -> Option<Vec<[f64; 3]>> {
    let crate::surface::SurfaceNamedValue::ScalarArray {
        dimensions,
        count: 3,
        values,
        ..
    } = &record.field(name)?.value
    else {
        return None;
    };
    let vector_count = usize::try_from(*dimensions).ok()?;
    (values.len() == vector_count.checked_mul(3)?).then_some(())?;
    values
        .chunks_exact(3)
        .map(|coordinates| Some([coordinates[0]?, coordinates[1]?, coordinates[2]?]))
        .collect()
}

pub(in super::super) fn prototype_parameter_array(
    record: &crate::surface::SurfacePrototypeRecord,
    name: &str,
) -> Option<Vec<f64>> {
    let crate::surface::SurfaceNamedValue::CountedScalarArray { count, values, .. } =
        &record.field(name)?.value
    else {
        return None;
    };
    (values.len() == usize::try_from(*count).ok()?).then_some(())?;
    values.iter().copied().collect()
}

pub(in super::super) fn prototype_spline_nurbs(
    record: &crate::surface::SurfacePrototypeRecord,
) -> Option<NurbsSurface> {
    interpolation_spline_surface(
        &prototype_vector_array(record, "i_points")?,
        &prototype_parameter_array(record, "u_params")?,
        &prototype_parameter_array(record, "v_params")?,
        &prototype_vector_array(record, "end_u_tangts")?,
        &prototype_vector_array(record, "end_v_tangts")?,
        &prototype_vector_array(record, "end_uv_deriv")?,
    )
}

pub(in super::super) fn prototype_local_frame(
    record: &crate::surface::SurfacePrototypeRecord,
) -> Option<([f64; 3], [f64; 3], [f64; 3])> {
    let crate::surface::SurfaceNamedValue::ScalarArray {
        dimensions: 4,
        count: 3,
        values,
        ..
    } = &record.field("local_sys")?.value
    else {
        return None;
    };
    let slots = values.iter().copied().collect::<Option<Vec<_>>>()?;
    let slots: [f64; 12] = slots.try_into().ok()?;
    slots.iter().all(|value| value.is_finite()).then_some(())?;
    let first: [f64; 3] = slots[0..3].try_into().ok()?;
    let middle: [f64; 3] = slots[3..6].try_into().ok()?;
    let third: [f64; 3] = slots[6..9].try_into().ok()?;
    let first_norm = dot(first, first).sqrt();
    let reference = normalized(first)?;
    let torus = matches!(record.family, crate::surface::SurfacePrototypeFamily::Torus);
    let mut second_candidates =
        [(middle, torus), (third, true)]
            .into_iter()
            .filter_map(|(candidate, eligible)| {
                let candidate_norm = dot(candidate, candidate).sqrt();
                let equal_scale =
                    (first_norm - candidate_norm).abs() <= 1e-10 * first_norm.max(candidate_norm);
                eligible
                    .then_some(())
                    .filter(|()| {
                        equal_scale && dot(reference, candidate).abs() <= 1e-10 * candidate_norm
                    })
                    .and_then(|()| normalized(candidate))
            });
    let second = second_candidates.next()?;
    second_candidates.next().is_none().then_some(())?;
    let axis = normalized(cross(reference, second))?;
    let origin: [f64; 3] = slots[9..12].try_into().ok()?;
    origin.into_iter().all(f64::is_finite).then_some(())?;
    Some((origin, axis, reference))
}

pub(in super::super) fn first_instance_surface_row(
    rows: &[crate::surface::SurfaceRow],
    frame_start: usize,
    frame_end: usize,
    prototype_offset: usize,
    row_kind: crate::surface::SurfaceKind,
) -> Option<&crate::surface::SurfaceRow> {
    let rows = rows
        .iter()
        .filter(|row| row.offset >= frame_start && row.offset < frame_end)
        .collect::<Vec<_>>();
    let previous = rows
        .iter()
        .copied()
        .filter(|row| row.offset < prototype_offset)
        .max_by_key(|row| row.offset);
    if previous.is_some_and(|row| row.kind == row_kind) {
        return previous;
    }
    rows.into_iter()
        .filter(|row| row.offset > prototype_offset && row.kind == row_kind)
        .min_by_key(|row| row.offset)
}

pub(in super::super) fn surface_prototype_frame_bounds(
    scan: &ContainerScan<'_>,
    section: &crate::container::Section,
    prototype_offset: usize,
) -> Option<(usize, usize)> {
    if scan.framing.data.is_empty() {
        return Some((
            section.offset,
            section.offset.saturating_add(section.length),
        ));
    }
    let section_end = section
        .offset
        .saturating_add(section.length)
        .min(scan.framing.data.len());
    let payload = scan.framing.data.get(section.offset..section_end)?;
    let relative_prototype_offset = prototype_offset.checked_sub(section.offset)?;
    let mut matches = crate::surface::complete_surface_array_bounds(payload)
        .into_iter()
        .filter(|(start, end)| {
            relative_prototype_offset >= *start && relative_prototype_offset < *end
        });
    let (start, end) = matches.next()?;
    matches.next().is_none().then_some((
        section.offset.saturating_add(start),
        section.offset.saturating_add(end),
    ))
}

pub(in super::super) fn unique_surface_prototype_associations<'a>(
    scan: &'a ContainerScan<'_>,
) -> Vec<(
    &'a crate::surface::SurfacePrototypeRecord,
    &'a crate::surface::SurfaceRow,
    &'a crate::container::Section,
)> {
    let mut associations = Vec::new();
    for record in &scan.surfaces.prototype_records {
        let row_kind = match record.family {
            crate::surface::SurfacePrototypeFamily::Plane => crate::surface::SurfaceKind::Plane,
            crate::surface::SurfacePrototypeFamily::Cylinder => {
                crate::surface::SurfaceKind::Cylinder
            }
            crate::surface::SurfacePrototypeFamily::Torus => {
                crate::surface::SurfaceKind::TorusOrSphere
            }
            crate::surface::SurfacePrototypeFamily::Cone => crate::surface::SurfaceKind::Cone,
            crate::surface::SurfacePrototypeFamily::Spline => crate::surface::SurfaceKind::Spline,
            _ => continue,
        };
        let Some(section) = scan.framing.sections.iter().find(|section| {
            record.offset >= section.offset
                && record.offset < section.offset.saturating_add(section.length)
        }) else {
            continue;
        };
        let Some((adjacent_start, adjacent_end)) =
            surface_prototype_frame_bounds(scan, section, record.offset)
        else {
            continue;
        };
        let Some(row) = first_instance_surface_row(
            &scan.surfaces.rows,
            adjacent_start,
            adjacent_end,
            record.offset,
            row_kind,
        ) else {
            continue;
        };
        if crate::surface::unique_surface_row(&scan.surfaces.rows, row.id)
            .is_none_or(|unique| unique.offset != row.offset)
        {
            continue;
        }
        associations.push((record, row, section));
    }
    let mut association_counts = BTreeMap::<usize, usize>::new();
    for (_, row, _) in &associations {
        *association_counts.entry(row.offset).or_default() += 1;
    }
    associations
        .into_iter()
        .filter(|(_, row, _)| association_counts.get(&row.offset) == Some(&1))
        .collect()
}

pub(in super::super) fn transfer_first_instance_prototype_surfaces(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> usize {
    if scan.framing.layout != crate::container::Layout::Nd {
        return 0;
    }
    let mut transferred = 0;
    for (record, row, section) in unique_surface_prototype_associations(scan) {
        let geometry = match record.family {
            crate::surface::SurfacePrototypeFamily::Plane => {
                let Some((origin, axis, reference)) = prototype_local_frame(record) else {
                    continue;
                };
                SurfaceGeometry::Plane {
                    origin: Point3::new(origin[0], origin[1], origin[2]),
                    normal: Vector3::new(axis[0], axis[1], axis[2]),
                    u_axis: Vector3::new(reference[0], reference[1], reference[2]),
                }
            }
            crate::surface::SurfacePrototypeFamily::Cylinder => {
                let Some((origin, axis, reference)) = prototype_local_frame(record) else {
                    continue;
                };
                let Some(radius) = prototype_scalar(record, "radius")
                    .filter(|radius| radius.is_finite() && *radius > 0.0)
                else {
                    continue;
                };
                SurfaceGeometry::Cylinder {
                    origin: Point3::new(origin[0], origin[1], origin[2]),
                    axis: Vector3::new(axis[0], axis[1], axis[2]),
                    ref_direction: Vector3::new(reference[0], reference[1], reference[2]),
                    radius,
                }
            }
            crate::surface::SurfacePrototypeFamily::Torus => {
                let Some((origin, axis, reference)) = prototype_local_frame(record) else {
                    continue;
                };
                let point = Point3::new(origin[0], origin[1], origin[2]);
                let axis = Vector3::new(axis[0], axis[1], axis[2]);
                let reference = Vector3::new(reference[0], reference[1], reference[2]);
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
                        .and_then(|parameter| parameter.torus_radius_overrides(row.type_byte))
                        .map(|overrides| [overrides.radius1, overrides.radius2])
                        .or(prototype_radii);
                let Some([radius1, radius2]) = radii else {
                    continue;
                };
                if radius1 == 0.0 {
                    SurfaceGeometry::Sphere {
                        center: point,
                        axis,
                        ref_direction: reference,
                        radius: radius2,
                    }
                } else {
                    SurfaceGeometry::Torus {
                        center: point,
                        axis,
                        ref_direction: reference,
                        major_radius: radius1,
                        minor_radius: radius2,
                    }
                }
            }
            crate::surface::SurfacePrototypeFamily::Cone => {
                let Some(frame) = crate::surface::prototype_cone_frame(record) else {
                    continue;
                };
                SurfaceGeometry::Cone {
                    origin: Point3::new(frame.apex[0], frame.apex[1], frame.apex[2]),
                    axis: Vector3::new(frame.axis[0], frame.axis[1], frame.axis[2]),
                    ref_direction: Vector3::new(
                        frame.ref_direction[0],
                        frame.ref_direction[1],
                        frame.ref_direction[2],
                    ),
                    radius: 0.0,
                    ratio: 1.0,
                    half_angle: frame.half_angle,
                }
            }
            crate::surface::SurfacePrototypeFamily::Spline => {
                let Some(nurbs) = prototype_spline_nurbs(record) else {
                    continue;
                };
                SurfaceGeometry::Nurbs(nurbs)
            }
            _ => unreachable!("prototype family was filtered above"),
        };
        let id = SurfaceId(format!("creo:visibgeom:surface#{}", row.id));
        if ir.model.surfaces.iter().any(|surface| surface.id == id) {
            continue;
        }
        annotate(
            annotations,
            &id,
            &section.name,
            record.offset as u64,
            "first_instance_surface_prototype",
            Exactness::Derived,
        );
        ir.model.surfaces.push(Surface {
            id,
            geometry,
            source_object: Some(SourceObjectAssociation {
                format: "creo".to_string(),
                object_id: format!("{}:{}", section.name, row.id),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
        transferred += 1;
    }
    transferred
}

pub(in super::super) fn transfer_positional_spline_replays(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> usize {
    if scan.framing.layout != crate::container::Layout::Nd {
        return 0;
    }
    let mut transferred = 0;
    for parameter in &scan.surfaces.parameters {
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
        let sections = scan
            .framing
            .sections
            .iter()
            .filter(|section| {
                row.offset >= section.offset
                    && row.offset < section.offset.saturating_add(section.length)
            })
            .collect::<Vec<_>>();
        let [section] = sections.as_slice() else {
            continue;
        };
        let section_end = section
            .offset
            .saturating_add(section.length)
            .min(scan.framing.data.len());
        let Some(payload) = scan.framing.data.get(section.offset..section_end) else {
            continue;
        };
        let Some(relative_row_offset) = row.offset.checked_sub(section.offset) else {
            continue;
        };
        let relative_row = {
            let mut row = row.clone();
            row.offset = relative_row_offset;
            row
        };
        let relative_rows = scan
            .surfaces
            .rows
            .iter()
            .filter(|candidate| {
                candidate.offset >= section.offset
                    && candidate.offset < section.offset.saturating_add(section.length)
            })
            .filter_map(|candidate| {
                let mut candidate = candidate.clone();
                candidate.offset = candidate.offset.checked_sub(section.offset)?;
                Some(candidate)
            })
            .collect::<Vec<_>>();
        let Some(prototype) = crate::surface::positional_spline_replay_prototype(
            payload,
            &relative_rows,
            &relative_row,
        ) else {
            continue;
        };
        let cache = crate::scalar::ScalarCache::from_section(payload);
        let Some(replay) =
            crate::surface::decode_positional_spline_replay(&parameter.body, &prototype, &cache)
        else {
            continue;
        };
        let Some(nurbs) = interpolation_spline_surface(
            &replay.points,
            &replay.u_parameters,
            &replay.v_parameters,
            &replay.u_derivatives,
            &replay.v_derivatives,
            &replay.mixed_derivatives,
        ) else {
            continue;
        };
        let id = SurfaceId(format!("creo:visibgeom:surface#{}", row.id));
        if ir.model.surfaces.iter().any(|surface| surface.id == id) {
            continue;
        }
        annotate(
            annotations,
            &id,
            &section.name,
            parameter.body_offset as u64,
            "positional_spline_prototype_replay",
            Exactness::Derived,
        );
        ir.model.surfaces.push(Surface {
            id,
            geometry: SurfaceGeometry::Nurbs(nurbs),
            source_object: Some(SourceObjectAssociation {
                format: "creo".to_string(),
                object_id: format!("{}:{}", section.name, row.id),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
        transferred += 1;
    }
    transferred
}

pub(in super::super) fn transfer_legacy_ascii_surface_carriers(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> usize {
    if scan.framing.layout != crate::container::Layout::LegacyAscii {
        return 0;
    }
    let mut carrier_counts = BTreeMap::<u32, usize>::new();
    for carrier in &scan.surfaces.legacy_carriers {
        *carrier_counts.entry(carrier.surface_id).or_default() += 1;
    }

    let mut transferred = 0;
    for carrier in &scan.surfaces.legacy_carriers {
        if carrier_counts.get(&carrier.surface_id) != Some(&1) {
            continue;
        }
        let Some(row) = crate::surface::unique_surface_row(&scan.surfaces.rows, carrier.surface_id)
        else {
            continue;
        };
        let geometry = match &carrier.geometry {
            crate::legacy_geometry::LegacySurfaceGeometry::Plane {
                origin,
                normal,
                u_axis,
            } if row.kind == crate::surface::SurfaceKind::Plane => SurfaceGeometry::Plane {
                origin: Point3::new(origin[0], origin[1], origin[2]),
                normal: Vector3::new(normal[0], normal[1], normal[2]),
                u_axis: Vector3::new(u_axis[0], u_axis[1], u_axis[2]),
            },
            crate::legacy_geometry::LegacySurfaceGeometry::Cylinder {
                origin,
                axis,
                ref_direction,
                radius,
            } if row.kind == crate::surface::SurfaceKind::Cylinder => SurfaceGeometry::Cylinder {
                origin: Point3::new(origin[0], origin[1], origin[2]),
                axis: Vector3::new(axis[0], axis[1], axis[2]),
                ref_direction: Vector3::new(ref_direction[0], ref_direction[1], ref_direction[2]),
                radius: *radius,
            },
            crate::legacy_geometry::LegacySurfaceGeometry::Cone {
                apex,
                axis,
                ref_direction,
                half_angle,
                ..
            } if row.kind == crate::surface::SurfaceKind::Cone => SurfaceGeometry::Cone {
                origin: Point3::new(apex[0], apex[1], apex[2]),
                axis: Vector3::new(axis[0], axis[1], axis[2]),
                ref_direction: Vector3::new(ref_direction[0], ref_direction[1], ref_direction[2]),
                radius: 0.0,
                ratio: 1.0,
                half_angle: *half_angle,
            },
            crate::legacy_geometry::LegacySurfaceGeometry::Spline {
                points,
                u_parameters,
                v_parameters,
                u_derivatives,
                v_derivatives,
                mixed_derivatives,
            } if row.kind == crate::surface::SurfaceKind::Spline => {
                let Some(nurbs) = interpolation_spline_surface(
                    points,
                    u_parameters,
                    v_parameters,
                    u_derivatives,
                    v_derivatives,
                    mixed_derivatives,
                ) else {
                    continue;
                };
                SurfaceGeometry::Nurbs(nurbs)
            }
            _ => continue,
        };
        let id = SurfaceId(format!("creo:visibgeom:surface#{}", carrier.surface_id));
        if ir.model.surfaces.iter().any(|surface| surface.id == id) {
            continue;
        }
        annotate(
            annotations,
            &id,
            "legacy_ascii",
            carrier.offset as u64,
            "legacy_surface_prototype_carrier",
            Exactness::Derived,
        );
        ir.model.surfaces.push(Surface {
            id,
            geometry,
            source_object: Some(SourceObjectAssociation {
                format: "creo".to_string(),
                object_id: format!("VisibGeom:{}", carrier.surface_id),
                name: None,
                color: None,
                visible: Some(true),
                layer: None,
                instance_path: Vec::new(),
            }),
        });
        transferred += 1;
    }
    transferred
}

#[cfg(test)]
mod tests;
