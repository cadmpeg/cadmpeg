// SPDX-License-Identifier: Apache-2.0
//! Positional spheres, tori, extrusion planes, and tabulated cylinders.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, ProceduralSurface, ProceduralSurfaceDefinition, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{CurveId, ProceduralSurfaceId, SurfaceId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::{AnnotationBuilder, Exactness, SourceObjectAssociation};

use crate::container::ContainerScan;

use super::super::analytic::cross;
use super::super::feature_history::{
    paired_five_coordinate_sphere_center, round_constant_radius, unique_surface_parameter_record,
};
use super::super::native::annotate;
use super::super::sketch::normalized;
use super::super::sketch_transfer::feature_schema_class;
use super::super::sweep::{extruded_nurbs_surface, placed_tabulated_cylinder_directrix};
use super::super::uniqueness::exactly_one;

use super::prototypes::{
    prototype_scalar, surface_prototype_frame_bounds, unique_surface_prototype_associations,
};

pub(in super::super) fn transfer_paired_envelope_spheres(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> usize {
    if scan.framing.layout != crate::container::Layout::Nd {
        return 0;
    }
    let mut transferred = 0;
    let associations = unique_surface_prototype_associations(scan)
        .into_iter()
        .filter_map(|(prototype, associated_row, section)| {
            let frame = surface_prototype_frame_bounds(scan, section, prototype.offset)?;
            Some((prototype, associated_row, section, frame))
        })
        .collect::<Vec<_>>();
    for (prototype, associated_row, section, (frame_start, frame_end)) in &associations {
        if prototype.family != crate::surface::SurfacePrototypeFamily::Torus
            || prototype_scalar(prototype, "radius1") != Some(0.0)
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
                candidate.family == crate::surface::SurfacePrototypeFamily::Torus
                    && candidate_row.feature_id == associated_row.feature_id
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
            unique_surface_parameter_record(scan, row)?
                .type26_five_coordinate_envelope(row.type_byte)
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
            let id = SurfaceId::mint(format!("creo:visibgeom:surface#{}", row.id))
                .expect("identity grammar");
            if ir.model.surfaces.iter().any(|surface| surface.id == id) {
                continue;
            }
            annotate(
                annotations,
                &id,
                &section.name,
                row.offset as u64,
                "paired_type26_sphere_envelope",
                Exactness::Derived,
            );
            ir.model.surfaces.push(Surface {
                id,
                geometry: SurfaceGeometry::Sphere {
                    center: Point3::new(center[0], center[1], center[2]),
                    axis: Vector3::new(0.0, 0.0, 1.0),
                    ref_direction: Vector3::new(1.0, 0.0, 0.0),
                    radius,
                },
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
    }
    transferred
}

#[cfg(test)]
mod tests;

pub(in super::super) fn transfer_positional_tori(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> usize {
    let constant_round_feature_ids = scan
        .surfaces
        .rows
        .iter()
        .filter(|row| row.kind == crate::surface::SurfaceKind::TorusOrSphere)
        .map(|row| row.feature_id)
        .filter(|feature_id| feature_schema_class(scan, *feature_id) == Some(913))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter(|feature_id| round_constant_radius(scan, ir, *feature_id).is_some())
        .collect::<BTreeSet<_>>();
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
            || record.has_inline_non_plane_local_system_suffix(row.type_byte);
        if row.type_byte == 0x26
            && feature_schema_class(scan, row.feature_id) == Some(913)
            && !constant_round_feature_ids.contains(&row.feature_id)
            && !inline_non_plane
        {
            continue;
        }
        let Some(frame) = record.positional_torus_frame else {
            continue;
        };
        let id = SurfaceId::mint(format!("creo:visibgeom:surface#{}", row.id))
            .expect("identity grammar");
        if ir.model.surfaces.iter().any(|surface| surface.id == id) {
            continue;
        }
        let Some(section) = scan.framing.sections.iter().find(|section| {
            row.offset >= section.offset
                && row.offset < section.offset.saturating_add(section.length)
        }) else {
            continue;
        };
        annotate(
            annotations,
            &id,
            &section.name,
            row.offset as u64,
            "positional_torus_frame",
            Exactness::Derived,
        );
        let geometry = if frame.major_radius == 0.0 {
            SurfaceGeometry::Sphere {
                center: Point3::new(frame.center[0], frame.center[1], frame.center[2]),
                axis: Vector3::new(frame.axis[0], frame.axis[1], frame.axis[2]),
                ref_direction: Vector3::new(
                    frame.ref_direction[0],
                    frame.ref_direction[1],
                    frame.ref_direction[2],
                ),
                radius: frame.minor_radius,
            }
        } else {
            SurfaceGeometry::Torus {
                center: Point3::new(frame.center[0], frame.center[1], frame.center[2]),
                axis: Vector3::new(frame.axis[0], frame.axis[1], frame.axis[2]),
                ref_direction: Vector3::new(
                    frame.ref_direction[0],
                    frame.ref_direction[1],
                    frame.ref_direction[2],
                ),
                major_radius: frame.major_radius,
                minor_radius: frame.minor_radius,
            }
        };
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

pub(in super::super) fn transfer_positional_line_extrusion_planes(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> usize {
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
        let Some(row) = crate::surface::unique_surface_row(&scan.surfaces.rows, record.surface_id)
        else {
            continue;
        };
        if row.kind != crate::surface::SurfaceKind::Extrusion {
            continue;
        }
        let type_byte = row.type_byte;
        let Some(frame) = record.line_extrusion_frame(type_byte) else {
            continue;
        };
        let directrix =
            std::array::from_fn(|axis| frame.directrix[1][axis] - frame.directrix[0][axis]);
        let (Some(_direction), Some(u_axis), Some(normal)) = (
            normalized(frame.direction),
            normalized(directrix),
            normalized(cross(directrix, frame.direction)),
        ) else {
            continue;
        };
        let surface_id = SurfaceId::mint(format!("creo:visibgeom:surface#{}", record.surface_id))
            .expect("identity grammar");
        if ir
            .model
            .surfaces
            .iter()
            .any(|surface| surface.id == surface_id)
        {
            continue;
        }
        let curve_id = CurveId::mint(format!(
            "creo:visibgeom:surface_directrix#{}",
            record.surface_id
        ))
        .expect("identity grammar");
        let procedural_id = ProceduralSurfaceId::mint(format!(
            "creo:visibgeom:surface_extrusion#{}",
            record.surface_id
        ))
        .expect("identity grammar");
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
            geometry: CurveGeometry::Line {
                origin: Point3::new(
                    frame.directrix[0][0],
                    frame.directrix[0][1],
                    frame.directrix[0][2],
                ),
                direction: Vector3::new(u_axis[0], u_axis[1], u_axis[2]),
            },
            source_object: Some(SourceObjectAssociation {
                format: "creo".to_string(),
                object_id: format!("VisibGeom:surface_directrix#{}", record.surface_id),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
        ir.model.surfaces.push(Surface {
            id: surface_id.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(
                    frame.directrix[0][0],
                    frame.directrix[0][1],
                    frame.directrix[0][2],
                ),
                normal: Vector3::new(normal[0], normal[1], normal[2]),
                u_axis: Vector3::new(u_axis[0], u_axis[1], u_axis[2]),
            },
            source_object: Some(SourceObjectAssociation {
                format: "creo".to_string(),
                object_id: format!("VisibGeom:{}", record.surface_id),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
        let _attached = ir.model.add_procedural_surface(
            surface_id,
            ProceduralSurface::new(
                procedural_id,
                ProceduralSurfaceDefinition::Extrusion {
                    directrix: curve_id,
                    parameter_interval: None,
                    direction: Vector3::new(
                        frame.direction[0],
                        frame.direction[1],
                        frame.direction[2],
                    ),
                    native_position: None,
                    revision_form: None,
                },
                None,
            ),
        );
        transferred += 1;
    }
    transferred
}

pub(in super::super) fn section_contains_offset(
    section: &crate::container::Section,
    offset: usize,
) -> bool {
    offset >= section.offset && offset < section.offset.saturating_add(section.length)
}

pub(in super::super) fn unique_tabulated_cylinder_prototype<'a>(
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

pub(in super::super) fn transfer_tabulated_cylinder_spline_extrusions(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> usize {
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
        if row.type_byte != 0x2c || row.offset != replay.surface_row_offset {
            continue;
        }
        let Some(parameters) =
            crate::surface::unique_surface_parameter(&scan.surfaces.parameters, replay.surface_id)
        else {
            continue;
        };
        let chart_origin = unique_tabulated_cylinder_prototype(scan, replay)
            .and_then(crate::surface::SurfacePrototypeRecord::tabulated_cylinder_chart_origin);
        let Some((directrix, sweep)) =
            placed_tabulated_cylinder_directrix(replay, parameters, chart_origin)
        else {
            continue;
        };
        let Some(surface) = extruded_nurbs_surface(&directrix, sweep) else {
            continue;
        };
        let curve_id = CurveId::mint(format!(
            "creo:visibgeom:tabulated_directrix#{}",
            replay.surface_id
        ))
        .expect("identity grammar");
        let surface_id = SurfaceId::mint(format!("creo:visibgeom:surface#{}", replay.surface_id))
            .expect("identity grammar");
        if ir
            .model
            .surfaces
            .iter()
            .any(|surface| surface.id == surface_id)
        {
            continue;
        }
        let procedural_id = ProceduralSurfaceId::mint(format!(
            "creo:visibgeom:tabulated_extrusion#{}",
            replay.surface_id
        ))
        .expect("identity grammar");
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
            geometry: CurveGeometry::Nurbs(directrix),
            source_object: Some(SourceObjectAssociation {
                format: "creo".to_string(),
                object_id: format!("VisibGeom:curve#{}", replay.curve_id),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
        ir.model.surfaces.push(Surface {
            id: surface_id.clone(),
            geometry: SurfaceGeometry::Nurbs(surface),
            source_object: Some(SourceObjectAssociation {
                format: "creo".to_string(),
                object_id: format!("VisibGeom:{}", replay.surface_id),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
        let _attached = ir.model.add_procedural_surface(
            surface_id,
            ProceduralSurface::new(
                procedural_id,
                ProceduralSurfaceDefinition::Extrusion {
                    directrix: curve_id,
                    parameter_interval: Some([0.0, 1.0]),
                    direction: Vector3::new(sweep[0], sweep[1], sweep[2]),
                    native_position: None,
                    revision_form: None,
                },
                None,
            ),
        );
        transferred += 1;
    }
    transferred
}
