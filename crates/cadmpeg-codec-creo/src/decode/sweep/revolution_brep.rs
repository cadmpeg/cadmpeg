// SPDX-License-Identifier: Apache-2.0
//! Resolved revolution B-rep transfer.

use super::super::feature_history::revolution_axis_for_transfer;
use super::super::sketch::section_point_in_model;
use super::super::sketch_ids::model_sketch_id;
use super::super::sketch_transfer::{
    current_additive_feature_recipe, feature_is_first_material_operation,
    feature_revolution_extent, unique_feature_revolution_extent,
};
use super::super::uniqueness::{
    unique_feature_definition_for_transform, unique_feature_section_transform,
};
use super::pcurves::{
    add_extrusion_pcurve, revolution_face_sense, revolution_profile_boundary_pcurve,
    revolved_brep_surface,
};
use super::profiles::{extrusion_profile_signed_area, resolved_sketch_profiles};
use super::surfaces::revolved_section_circle;
use crate::container::ContainerScan;
use cadmpeg_core::decode::alloc_filled;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{Curve, CurveGeometry, Surface};
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PcurveId, PointId, RegionId, ShellId,
    SurfaceId, VertexId,
};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Edge, Face, Loop as IrLoop, PcurveUse, Point, Region, Sense, Shell,
    Vertex,
};
use cadmpeg_ir::AnnotationBuilder;

pub(in super::super) fn transfer_resolved_revolution_breps(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> usize {
    let mut transferred = 0;
    for transform in &scan.features.section_transforms {
        if unique_feature_section_transform(
            &scan.features.section_transforms,
            transform.definition_id,
            transform.offset,
        )
        .is_none()
        {
            continue;
        }
        let Some(feature_id) = transform.feature_id else {
            continue;
        };
        if current_additive_feature_recipe(&scan.features.operations, feature_id)
            != Some(crate::feature::FeatureRecipeKind::Revolve)
            || !feature_is_first_material_operation(scan, feature_id)
            || unique_feature_revolution_extent(&scan.features.revolution_extents, feature_id)
                .is_none()
        {
            continue;
        }
        let Some(definition) =
            unique_feature_definition_for_transform(&scan.features.definitions, transform)
        else {
            continue;
        };
        let extent = feature_revolution_extent(scan, feature_id);
        let Some(axis) = revolution_axis_for_transfer(
            scan,
            ir,
            feature_id,
            definition,
            transform,
            extent.as_ref(),
        ) else {
            continue;
        };
        let sketch_id = model_sketch_id(scan, definition);
        let Some(mut profiles) = resolved_sketch_profiles(ir, &sketch_id, 2) else {
            continue;
        };
        let [profile] = profiles.as_mut_slice() else {
            continue;
        };
        let Some(area) = extrusion_profile_signed_area(profile) else {
            continue;
        };
        let vertex_curves = profile
            .iter()
            .map(|(_, _, point, _)| revolved_section_circle(transform, *point, &axis))
            .collect::<Vec<_>>();
        let surface_geometries = profile
            .iter()
            .map(|(geometry, reversed, _, _)| {
                revolved_brep_surface(transform, geometry, *reversed, &axis)
            })
            .collect::<Option<Vec<_>>>();
        let Some(surface_geometries) = surface_geometries else {
            continue;
        };
        let boundaries_are_complete = profile.iter().enumerate().all(|(index, segment)| {
            let next = (index + 1) % profile.len();
            (vertex_curves[index].is_some() || vertex_curves[next].is_some())
                && [
                    (segment.2, vertex_curves[index].is_some(), true),
                    (segment.3, vertex_curves[next].is_some(), false),
                ]
                .into_iter()
                .all(|(section_point, present, at_start)| {
                    !present
                        || revolution_profile_boundary_pcurve(
                            transform,
                            segment,
                            &surface_geometries[index],
                            &axis,
                            section_point,
                            at_start,
                        )
                        .is_some()
                })
        });
        if !boundaries_are_complete {
            continue;
        }
        let face_senses = profile
            .iter()
            .zip(&surface_geometries)
            .map(|(segment, surface)| {
                revolution_face_sense(transform, segment, surface, &axis, area)
            })
            .collect::<Option<Vec<_>>>();
        let Some(face_senses) = face_senses else {
            continue;
        };
        let prefix = format!("creo:feature:revolution#{feature_id}");
        let body_id = BodyId::mint(format!("{prefix}:body")).expect("identity grammar");
        if ir.model.bodies.iter().any(|body| body.id == body_id) {
            continue;
        }
        let region_id = RegionId::mint(format!("{prefix}:region")).expect("identity grammar");
        let shell_id = ShellId::mint(format!("{prefix}:shell")).expect("identity grammar");
        let count = profile.len();
        let Ok(mut edges) = alloc_filled(count, None, "creo revolution profile edges") else {
            continue;
        };
        for (index, ((_, _, point, _), curve_geometry)) in
            profile.iter().zip(vertex_curves).enumerate()
        {
            let Some(curve_geometry) = curve_geometry else {
                continue;
            };
            let CurveGeometry::Circle {
                center,
                axis: curve_axis,
                ref_direction,
                radius,
            } = curve_geometry
            else {
                unreachable!();
            };
            let curve_id =
                CurveId::mint(format!("{prefix}:curve:vertex:{index}")).expect("identity grammar");
            let point_id =
                PointId::mint(format!("{prefix}:point:vertex:{index}")).expect("identity grammar");
            let vertex_id =
                VertexId::mint(format!("{prefix}:vertex:{index}")).expect("identity grammar");
            let edge_id =
                EdgeId::mint(format!("{prefix}:edge:vertex:{index}")).expect("identity grammar");
            let position = section_point_in_model(transform, *point);
            ir.model.curves.push(Curve {
                id: curve_id.clone(),
                geometry: CurveGeometry::Circle {
                    center,
                    axis: curve_axis,
                    ref_direction,
                    radius,
                },
                source_object: None,
            });
            ir.model.points.push(Point {
                id: point_id.clone(),
                position: Point3::new(position[0], position[1], position[2]),
                source_object: None,
            });
            ir.model.vertices.push(Vertex {
                id: vertex_id.clone(),
                point: point_id,
                tolerance: None,
            });
            ir.model.edges.push(Edge {
                id: edge_id.clone(),
                curve: Some(curve_id),
                start: vertex_id.clone(),
                end: vertex_id,
                param_range: Some([0.0, std::f64::consts::TAU]),
                tolerance: None,
            });
            edges[index] = Some(edge_id);
        }
        let mut faces = Vec::new();
        for (index, (((_, _, start, end), surface_geometry), face_sense)) in profile
            .iter()
            .zip(surface_geometries)
            .zip(face_senses)
            .enumerate()
        {
            let next = (index + 1) % count;
            let surface_id =
                SurfaceId::mint(format!("{prefix}:surface:{index}")).expect("identity grammar");
            let face_id = FaceId::mint(format!("{prefix}:face:{index}")).expect("identity grammar");
            ir.model.surfaces.push(Surface {
                id: surface_id.clone(),
                geometry: surface_geometry.clone(),
                source_object: None,
            });
            let mut loops = Vec::new();
            for (boundary, vertex_index, section_point, sense) in [
                ("start", index, *start, Sense::Reversed),
                ("end", next, *end, Sense::Forward),
            ] {
                let Some(edge_id) = edges[vertex_index].clone() else {
                    continue;
                };
                let loop_id = LoopId::mint(format!("{prefix}:loop:{index}:{boundary}"))
                    .expect("identity grammar");
                let coedge_id = CoedgeId::mint(format!("{prefix}:coedge:{index}:{boundary}"))
                    .expect("identity grammar");
                let radial_index = if boundary == "start" {
                    (index + count - 1) % count
                } else {
                    next
                };
                let radial_boundary = if boundary == "start" { "end" } else { "start" };
                let pcurve_geometry = revolution_profile_boundary_pcurve(
                    transform,
                    &profile[index],
                    &surface_geometry,
                    &axis,
                    section_point,
                    boundary == "start",
                )
                .expect("revolution boundary was prevalidated");
                let pcurve = add_extrusion_pcurve(
                    ir,
                    annotations,
                    PcurveId::mint(format!("{prefix}:pcurve:{index}:{boundary}"))
                        .expect("identity grammar"),
                    transform.offset,
                    pcurve_geometry,
                );
                ir.model.loops.push(IrLoop {
                    id: loop_id.clone(),
                    face: face_id.clone(),
                    boundary: cadmpeg_ir::topology::LoopBoundary::Ring {
                        coedges: vec![coedge_id.clone()],
                        vertex_uses: Vec::new(),
                    },
                });
                ir.model.coedges.push(Coedge {
                    id: coedge_id.clone(),
                    owner_loop: loop_id.clone(),
                    edge: edge_id,
                    radial_next: CoedgeId::mint(format!(
                        "{prefix}:coedge:{radial_index}:{radial_boundary}"
                    ))
                    .expect("identity grammar"),
                    sense,
                    pcurves: vec![PcurveUse {
                        pcurve,
                        isoparametric: None,
                        parameter_range: None,
                    }],
                    use_curve: None,
                });
                loops.push(loop_id);
            }
            ir.model.faces.push(Face {
                id: face_id.clone(),
                shell: shell_id.clone(),
                surface: surface_id,
                sense: face_sense,
                loops: loops.into(),
                name: None,
                color: None,
                tolerance: None,
            });
            faces.push(face_id);
        }
        ir.model.shells.push(Shell {
            id: shell_id.clone(),
            region: region_id.clone(),
            faces,
            wire_edges: Vec::new(),
            free_vertices: Vec::new(),
        });
        ir.model.regions.push(Region {
            id: region_id.clone(),
            body: body_id.clone(),
            shells: vec![shell_id],
        });
        ir.model.bodies.push(Body {
            id: body_id,
            kind: BodyKind::Solid,
            regions: vec![region_id],
            transform: None,
            name: None,
            color: None,
            visible: None,
        });
        transferred += 1;
    }
    transferred
}
