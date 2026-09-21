// SPDX-License-Identifier: Apache-2.0
//! Positional spheres, tori, extrusion planes, and tabulated cylinders.

use crate::feature::schema::SchemaClass;
use crate::vecmath::normalize;
use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, ProceduralSurface, ProceduralSurfaceDefinition, SolvedCurveGeometry,
    SolvedSurfaceGeometry, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{CurveId, ProceduralSurfaceId, SurfaceId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::{AnnotationBuilder, Exactness, SourceObjectAssociation};

use crate::container::ContainerScan;

use super::super::feature_history::round::{
    paired_five_coordinate_sphere_center, round_constant_radius, unique_surface_parameter_record,
};
use super::super::native::annotate;
use super::super::sweep::nurbs::{extruded_nurbs_surface, placed_tabulated_cylinder_directrix};
use super::super::uniqueness::exactly_one;
use crate::decode::sketch_transfer::recipe::feature_schema_class;
use crate::vecmath::cross;

use super::prototypes::{
    prototype_scalar, surface_prototype_frame_bounds, unique_surface_prototype_associations,
};

pub(in super::super) fn transfer_paired_envelope_spheres(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> Result<usize, cadmpeg_core::CodecError> {
    if scan.framing.layout != crate::container::Layout::Nd {
        return Ok(0);
    }
    let mut transferred = 0;
    let mut associations = Vec::new();
    for (prototype, associated_row, section) in unique_surface_prototype_associations(scan)? {
        let prototype = prototype.record();
        let Some(frame) = surface_prototype_frame_bounds(scan, section, prototype.offset)? else {
            continue;
        };
        associations.push((prototype, associated_row, section, frame));
    }
    for (prototype, associated_row, section, (frame_start, frame_end)) in &associations {
        if !matches!(
            prototype.family,
            crate::surface::SurfacePrototypeFamily::Torus(_)
        ) || prototype_scalar(prototype, "radius1") != Some(0.0)
        {
            continue;
        }
        let Some(radius) = prototype_scalar(prototype, "radius2")
            .filter(|radius| radius.is_finite() && *radius > 0.0)
        else {
            continue;
        };
        let associated_prototype_count = associations
            .iter()
            .filter(|(candidate, candidate_row, _, candidate_frame)| {
                matches!(
                    candidate.family,
                    crate::surface::SurfacePrototypeFamily::Torus(_)
                ) && candidate_row.feature_id == associated_row.feature_id
                    && candidate_frame == &(*frame_start, *frame_end)
            })
            .count();
        if associated_prototype_count != 1 {
            continue;
        }
        let rows = scan
            .surfaces
            .rows
            .iter()
            .filter(|row| {
                row.offset >= *frame_start
                    && row.offset < *frame_end
                    && row.feature_id == associated_row.feature_id
                    && row.kind == crate::surface::SurfaceKind::TorusOrSphere
            })
            .collect::<Vec<_>>();
        let [first_row, second_row] = rows.as_slice() else {
            continue;
        };
        let envelopes = [first_row, second_row].map(|row| {
            unique_surface_parameter_record(scan, row)?.type26_five_coordinate_envelope()
        });
        let [Some(first_envelope), Some(second_envelope)] = envelopes else {
            continue;
        };
        let Some(center) =
            paired_five_coordinate_sphere_center([first_envelope, second_envelope], radius)
        else {
            continue;
        };
        for row in rows {
            let id = SurfaceId::compose(&crate::identity::VISIBGEOM_SURFACE, row.id);
            if ir.model.surfaces.iter().any(|surface| surface.id == id) {
                continue;
            }
            let Ok(sphere_surface) = cadmpeg_ir::geometry::analytic::SphereSurface::try_new(
                Point3::from(center),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                radius,
            ) else {
                continue;
            };
            annotate(
                annotations,
                &id,
                section.name(),
                row.offset as u64,
                "paired_type26_sphere_envelope",
                Exactness::Derived,
            );
            ir.model.surfaces.push(Surface {
                id,
                geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(sphere_surface)),
                source_object: Some(SourceObjectAssociation {
                    format: cadmpeg_ir::CodecFormat::Creo,
                    object_id: cadmpeg_core::text::NonBlankString::new(format!(
                        "{}:{}",
                        section.name(),
                        row.id
                    ))
                    .ok_or_else(|| {
                        cadmpeg_core::CodecError::malformed("source object_id must not be empty")
                    })?,
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            });
            transferred += 1;
        }
    }
    Ok(transferred)
}

#[cfg(test)]
mod tests;

pub(in super::super) fn transfer_positional_tori(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> Result<usize, cadmpeg_core::CodecError> {
    let round_feature_ids = scan
        .surfaces
        .rows
        .iter()
        .filter(|row| row.kind == crate::surface::SurfaceKind::TorusOrSphere)
        .map(|row| row.feature_id)
        .filter(|feature_id| feature_schema_class(scan, *feature_id) == Some(SchemaClass::Round))
        .collect::<BTreeSet<_>>();
    let mut constant_round_feature_ids = BTreeSet::new();
    for feature_id in round_feature_ids {
        if round_constant_radius(scan, ir, feature_id)?.is_some() {
            constant_round_feature_ids.insert(feature_id);
        }
    }
    let mut transferred = 0;
    for record in &scan.surfaces.parameters {
        let Some(row) = crate::surface::unique_surface_row(&scan.surfaces.rows, record.surface_id)
        else {
            continue;
        };
        if row.kind != crate::surface::SurfaceKind::TorusOrSphere
            || crate::surface::unique_surface_parameter(
                &scan.surfaces.parameters,
                record.surface_id,
            )
            .is_none_or(|unique| unique.offset != record.offset)
        {
            continue;
        }
        // Class-913 type-26 rows can be rolling-radius samples from the same
        // generated round family. A positional torus frame is a neutral
        // carrier only after the complete family proves one constant radius.
        let inline_non_plane = record.has_inline_non_plane_envelope()
            || record.has_inline_non_plane_local_system_suffix();
        if row.kind == crate::surface::SurfaceKind::TorusOrSphere
            && feature_schema_class(scan, row.feature_id) == Some(SchemaClass::Round)
            && !constant_round_feature_ids.contains(&row.feature_id)
            && !inline_non_plane
        {
            continue;
        }
        let Some(frame) = record.positional_torus_frame() else {
            continue;
        };
        let id = SurfaceId::compose(&crate::identity::VISIBGEOM_SURFACE, row.id);
        if ir.model.surfaces.iter().any(|surface| surface.id == id) {
            continue;
        }
        let Some(section) = scan
            .framing
            .sections
            .iter()
            .find(|section| section.contains(row.offset))
        else {
            continue;
        };
        let center = frame.frame().origin_point();
        let axis = frame.frame().axis_vector();
        let ref_direction = frame.frame().ref_direction_vector();
        let geometry = if frame.major_radius() == 0.0 {
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(
                match cadmpeg_ir::geometry::analytic::SphereSurface::try_new(
                    center,
                    axis,
                    ref_direction,
                    frame.minor_radius(),
                ) {
                    Ok(payload) => payload,
                    Err(_) => continue,
                },
            ))
        } else {
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(
                match cadmpeg_ir::geometry::analytic::TorusSurface::try_new(
                    center,
                    axis,
                    ref_direction,
                    frame.major_radius(),
                    frame.minor_radius(),
                ) {
                    Ok(payload) => payload,
                    Err(_) => continue,
                },
            ))
        };
        annotate(
            annotations,
            &id,
            section.name(),
            row.offset as u64,
            "positional_torus_frame",
            Exactness::Derived,
        );
        ir.model.surfaces.push(Surface {
            id,
            geometry,
            source_object: Some(SourceObjectAssociation {
                format: cadmpeg_ir::CodecFormat::Creo,
                object_id: cadmpeg_core::text::NonBlankString::new(format!(
                    "{}:{}",
                    section.name(),
                    row.id
                ))
                .ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed("source object_id must not be empty")
                })?,
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
        transferred += 1;
    }
    Ok(transferred)
}

pub(in super::super) fn transfer_positional_line_extrusion_planes(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> Result<usize, cadmpeg_core::CodecError> {
    let replay_bound_surfaces = scan
        .curves
        .tabulated_cylinder_replays
        .iter()
        .map(|replay| replay.surface_id)
        .collect::<BTreeSet<_>>();
    let mut transferred = 0;
    for record in &scan.surfaces.parameters {
        if replay_bound_surfaces.contains(&record.surface_id) {
            continue;
        }
        if crate::surface::unique_surface_parameter(&scan.surfaces.parameters, record.surface_id)
            .is_none_or(|unique| unique.offset != record.offset)
        {
            continue;
        }
        if crate::surface::unique_surface_row(&scan.surfaces.rows, record.surface_id).is_none() {
            continue;
        }
        let Some(frame) = record.line_extrusion_frame() else {
            continue;
        };
        let directrix =
            std::array::from_fn(|axis| frame.directrix[1][axis] - frame.directrix[0][axis]);
        let (Some(_direction), Some(u_axis), Some(normal)) = (
            normalize(frame.direction),
            normalize(directrix),
            normalize(cross(directrix, frame.direction)),
        ) else {
            continue;
        };
        let surface_id = SurfaceId::compose(&crate::identity::VISIBGEOM_SURFACE, record.surface_id);
        if ir
            .model
            .surfaces
            .iter()
            .any(|surface| surface.id == surface_id)
        {
            continue;
        }
        let curve_id = CurveId::compose(
            &crate::identity::VISIBGEOM_SURFACE_DIRECTRIX,
            record.surface_id,
        );
        let procedural_id = ProceduralSurfaceId::compose(
            &crate::identity::VISIBGEOM_SURFACE_EXTRUSION,
            record.surface_id,
        );
        let Ok(line_curve) = cadmpeg_ir::geometry::analytic::LineCurve::try_new(
            Point3::from(frame.directrix[0]),
            Vector3::from(u_axis),
        ) else {
            continue;
        };
        let Ok(plane_surface) = cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
            Point3::from(frame.directrix[0]),
            Vector3::from(normal),
            Vector3::from(u_axis),
        ) else {
            continue;
        };
        annotate(
            annotations,
            &curve_id,
            "VisibGeom",
            record.body_offset as u64,
            "positional_line_extrusion_directrix",
            Exactness::Derived,
        );
        annotate(
            annotations,
            &surface_id,
            "VisibGeom",
            record.body_offset as u64,
            "positional_line_extrusion_plane",
            Exactness::Derived,
        );
        annotate(
            annotations,
            &procedural_id,
            "VisibGeom",
            record.body_offset as u64,
            "positional_line_extrusion_construction",
            Exactness::Derived,
        );
        ir.model.curves.push(Curve {
            id: curve_id.clone(),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(line_curve)),
            source_object: Some(SourceObjectAssociation {
                format: cadmpeg_ir::CodecFormat::Creo,
                object_id: cadmpeg_core::text::NonBlankString::new(format!(
                    "VisibGeom:surface_directrix#{}",
                    record.surface_id
                ))
                .ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed("source object_id must not be empty")
                })?,
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
        ir.model.surfaces.push(Surface {
            id: surface_id.clone(),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface)),
            source_object: Some(SourceObjectAssociation {
                format: cadmpeg_ir::CodecFormat::Creo,
                object_id: cadmpeg_core::text::NonBlankString::new(format!(
                    "VisibGeom:{}",
                    record.surface_id
                ))
                .ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed("source object_id must not be empty")
                })?,
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
        let _attached = ir.model.add_procedural_surface(
            surface_id,
            cadmpeg_ir::geometry::surface_payloads::ExtrusionSurfaceConstruction::try_new(
                curve_id,
                None,
                Vector3::from(frame.direction),
                None,
                cadmpeg_ir::geometry::CacheContract::from_form(None),
            )
            .map(|admitted_payload| {
                ProceduralSurface::new(
                    procedural_id,
                    ProceduralSurfaceDefinition::Extrusion(admitted_payload),
                    None,
                )
            })
            .map_err(cadmpeg_core::CodecError::malformed)?,
        );
        transferred += 1;
    }
    Ok(transferred)
}

fn section_contains_offset(section: &crate::container::Section, offset: usize) -> bool {
    section.contains(offset)
}

/// Report every refused tabulated-cylinder lane against the row that stated it.
fn note_tabulated_cylinder_refusals(
    surface_id: u32,
    replay_offset: usize,
    lane: &str,
    refused: &[String],
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) {
    for record in refused {
        losses.push(
            crate::loss::CreoLossCode::VisibGeomSurfaceUntransferred.note(format!(
                "VisibGeom surface row {surface_id} states a tabulated-cylinder replay at offset \
                 {replay_offset} whose {lane} lane forms no carrier: {record}"
            )),
        );
    }
}

fn unique_tabulated_cylinder_prototype<'a>(
    scan: &'a ContainerScan<'_>,
    replay: &crate::surface::TabulatedCylinderCurveReplay,
) -> Option<&'a crate::surface::SurfacePrototypeRecord> {
    let section = exactly_one(
        scan.framing
            .sections
            .iter()
            .filter(|section| section_contains_offset(section, replay.surface_row_offset)),
    )?;
    exactly_one(scan.surfaces.prototype_records.iter().filter(|record| {
        section_contains_offset(section, record.offset)
            && record.tabulated_cylinder_control_point_ids() == Some(replay.control_point_ids)
    }))
}

/// Transfer one exact extrusion carrier per tabulated-cylinder spline replay.
///
/// A refused directrix or extrusion lane leaves the surface row without a
/// carrier, which the model carries, so it is a loss note naming the
/// `VisibGeom` surface row and the replay offset.
pub(in super::super) fn transfer_tabulated_cylinder_spline_extrusions(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<usize, cadmpeg_core::CodecError> {
    let mut replay_counts = BTreeMap::<u32, usize>::new();
    for replay in &scan.curves.tabulated_cylinder_replays {
        *replay_counts.entry(replay.surface_id).or_default() += 1;
    }
    let mut transferred = 0;
    for replay in &scan.curves.tabulated_cylinder_replays {
        if replay_counts.get(&replay.surface_id) != Some(&1) {
            continue;
        }
        let Some(row) = crate::surface::unique_surface_row(&scan.surfaces.rows, replay.surface_id)
        else {
            continue;
        };
        if row.kind
            != crate::surface::SurfaceKind::Extrusion(
                crate::surface::ExtrusionVariant::TabulatedCylinder,
            )
            || row.offset != replay.surface_row_offset
        {
            continue;
        }
        let Some(parameters) =
            crate::surface::unique_surface_parameter(&scan.surfaces.parameters, replay.surface_id)
        else {
            continue;
        };
        let chart_origin = unique_tabulated_cylinder_prototype(scan, replay)
            .and_then(crate::surface::SurfacePrototypeRecord::tabulated_cylinder_chart_origin);
        let mut refusal = crate::lane_refusal::LaneRefusals::new();
        let directrix =
            placed_tabulated_cylinder_directrix(replay, parameters, chart_origin, &mut refusal);
        let refused = refusal.take_records();
        let Some((directrix, sweep)) = directrix.filter(|_| refused.is_empty()) else {
            note_tabulated_cylinder_refusals(
                replay.surface_id,
                replay.offset,
                "directrix",
                &refused,
                losses,
            );
            continue;
        };
        let mut refusal = crate::lane_refusal::LaneRefusals::new();
        let surface = extruded_nurbs_surface(
            &directrix,
            sweep,
            &format!(
                "VisibGeom surface row {} tabulated-cylinder replay at offset {}",
                replay.surface_id, replay.offset
            ),
            &mut refusal,
        );
        let refused = refusal.take_records();
        let Some(surface) = surface.filter(|_| refused.is_empty()) else {
            note_tabulated_cylinder_refusals(
                replay.surface_id,
                replay.offset,
                "extrusion",
                &refused,
                losses,
            );
            continue;
        };
        let curve_id = CurveId::compose(
            &crate::identity::VISIBGEOM_TABULATED_DIRECTRIX,
            replay.surface_id,
        );
        let surface_id = SurfaceId::compose(&crate::identity::VISIBGEOM_SURFACE, replay.surface_id);
        if ir
            .model
            .surfaces
            .iter()
            .any(|surface| surface.id == surface_id)
        {
            continue;
        }
        let procedural_id = ProceduralSurfaceId::compose(
            &crate::identity::VISIBGEOM_TABULATED_EXTRUSION,
            replay.surface_id,
        );
        annotate(
            annotations,
            &curve_id,
            "VisibGeom",
            replay.offset as u64,
            "tabulated_cylinder_directrix",
            Exactness::Derived,
        );
        annotate(
            annotations,
            &surface_id,
            "VisibGeom",
            replay.surface_row_offset as u64,
            "tabulated_cylinder_surface",
            Exactness::Derived,
        );
        annotate(
            annotations,
            &procedural_id,
            "VisibGeom",
            replay.surface_row_offset as u64,
            "tabulated_cylinder_extrusion",
            Exactness::Derived,
        );
        ir.model.curves.push(Curve {
            id: curve_id.clone(),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(directrix)),
            source_object: Some(SourceObjectAssociation {
                format: cadmpeg_ir::CodecFormat::Creo,
                object_id: cadmpeg_core::text::NonBlankString::new(format!(
                    "VisibGeom:curve#{}",
                    replay.curve_id
                ))
                .ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed("source object_id must not be empty")
                })?,
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
        ir.model.surfaces.push(Surface {
            id: surface_id.clone(),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(surface)),
            source_object: Some(SourceObjectAssociation {
                format: cadmpeg_ir::CodecFormat::Creo,
                object_id: cadmpeg_core::text::NonBlankString::new(format!(
                    "VisibGeom:{}",
                    replay.surface_id
                ))
                .ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed("source object_id must not be empty")
                })?,
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
        let _attached = ir.model.add_procedural_surface(
            surface_id,
            cadmpeg_ir::geometry::surface_payloads::ExtrusionSurfaceConstruction::try_new(
                curve_id,
                Some([0.0, 1.0]),
                Vector3::from(sweep),
                None,
                cadmpeg_ir::geometry::CacheContract::from_form(None),
            )
            .map(|admitted_payload| {
                ProceduralSurface::new(
                    procedural_id,
                    ProceduralSurfaceDefinition::Extrusion(admitted_payload),
                    None,
                )
            })
            .map_err(cadmpeg_core::CodecError::malformed)?,
        );
        transferred += 1;
    }
    Ok(transferred)
}
