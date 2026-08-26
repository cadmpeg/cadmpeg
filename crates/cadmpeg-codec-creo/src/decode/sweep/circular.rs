// SPDX-License-Identifier: Apache-2.0
//! Circular extrusion B-rep transfer.

use super::super::analytic::dot;
use super::super::feature_history::feature_allows_additive_linear_extrusion;
use super::super::holes::circular_sweep_geometry;
use super::super::sketch::{normalized, section_point_in_model};
use super::super::sketch_ids::model_sketch_id;
use super::super::sketch_transfer::feature_is_first_material_operation;
use super::super::uniqueness::{
    exactly_one, unique_feature_definition_for_transform, unique_feature_section_transform,
};
use super::extent::resolved_feature_extrusion_span;
use super::pcurves::add_extrusion_pcurve;
use super::profiles::{circular_pcurve, line_pcurve};
use crate::container::ContainerScan;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{Curve, CurveGeometry, Surface, SurfaceGeometry};
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PcurveId, PointId, RegionId, ShellId,
    SurfaceId, VertexId,
};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::sketches::{SketchGeometry, SketchId};
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Edge, Face, Loop as IrLoop, PcurveUse, Point, Region, Sense, Shell,
    Vertex,
};
use cadmpeg_ir::AnnotationBuilder;

const EPS_AXIS_ALIGNMENT: f64 = 1.0e-9;

pub(in super::super) fn transfer_resolved_circular_extrusion_breps(
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
        if !feature_allows_additive_linear_extrusion(scan, feature_id)
            || !feature_is_first_material_operation(scan, feature_id)
        {
            continue;
        }
        let Some(definition) =
            unique_feature_definition_for_transform(&scan.features.definitions, transform)
        else {
            continue;
        };
        let sketch_id = model_sketch_id(scan, definition);
        let Some((section_center, radius)) =
            resolved_circular_extrusion_profile(scan, ir, transform, feature_id, &sketch_id)
        else {
            continue;
        };
        let Some(span) = resolved_feature_extrusion_span(scan, ir, definition, transform) else {
            continue;
        };
        let prefix = format!("creo:feature:extrusion#{feature_id}");
        let body_id = BodyId(format!("{prefix}:body"));
        if ir.model.bodies.iter().any(|body| body.id == body_id) {
            continue;
        }
        let region_id = RegionId(format!("{prefix}:region"));
        let shell_id = ShellId(format!("{prefix}:shell"));
        let center = section_point_in_model(transform, section_center);
        let seam =
            std::array::from_fn::<_, 3, _>(|axis| center[axis] + radius * transform.u_axis[axis]);
        let sides = [("bottom", span.lower), ("top", span.upper)];
        let mut face_ids = Vec::new();
        let mut cap_coedges = Vec::new();
        let mut side_coedges = Vec::new();
        for (side_index, (side, offset)) in sides.into_iter().enumerate() {
            let cap_surface = SurfaceId(format!("{prefix}:surface:{side}"));
            let cap_face = FaceId(format!("{prefix}:face:{side}"));
            let cap_loop = LoopId(format!("{prefix}:loop:{side}"));
            let curve_id = CurveId(format!("{prefix}:curve:{side}"));
            let edge_id = EdgeId(format!("{prefix}:edge:{side}"));
            let point_id = PointId(format!("{prefix}:point:{side}"));
            let vertex_id = VertexId(format!("{prefix}:vertex:{side}"));
            let cap_coedge = CoedgeId(format!("{prefix}:coedge:{side}:cap"));
            let side_coedge = CoedgeId(format!("{prefix}:coedge:{side}:side"));
            let cap_pcurve = add_extrusion_pcurve(
                ir,
                annotations,
                PcurveId(format!("{prefix}:pcurve:{side}:cap")),
                transform.offset,
                circular_pcurve(section_center, radius, 0.0, std::f64::consts::TAU),
            );
            let side_pcurve = add_extrusion_pcurve(
                ir,
                annotations,
                PcurveId(format!("{prefix}:pcurve:{side}:side")),
                transform.offset,
                line_pcurve([0.0, offset], [std::f64::consts::TAU, offset]),
            );
            ir.model.surfaces.push(Surface {
                id: cap_surface.clone(),
                geometry: SurfaceGeometry::Plane {
                    origin: Point3::new(
                        transform.origin[0] + offset * transform.normal[0],
                        transform.origin[1] + offset * transform.normal[1],
                        transform.origin[2] + offset * transform.normal[2],
                    ),
                    normal: Vector3::new(
                        transform.normal[0],
                        transform.normal[1],
                        transform.normal[2],
                    ),
                    u_axis: Vector3::new(
                        transform.u_axis[0],
                        transform.u_axis[1],
                        transform.u_axis[2],
                    ),
                },
                source_object: None,
            });
            ir.model.curves.push(Curve {
                id: curve_id.clone(),
                geometry: CurveGeometry::Circle {
                    center: Point3::new(
                        center[0] + offset * transform.normal[0],
                        center[1] + offset * transform.normal[1],
                        center[2] + offset * transform.normal[2],
                    ),
                    axis: Vector3::new(
                        transform.normal[0],
                        transform.normal[1],
                        transform.normal[2],
                    ),
                    ref_direction: Vector3::new(
                        transform.u_axis[0],
                        transform.u_axis[1],
                        transform.u_axis[2],
                    ),
                    radius,
                },
                source_object: None,
            });
            ir.model.points.push(Point {
                id: point_id.clone(),
                position: Point3::new(
                    seam[0] + offset * transform.normal[0],
                    seam[1] + offset * transform.normal[1],
                    seam[2] + offset * transform.normal[2],
                ),
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
            ir.model.loops.push(IrLoop {
                id: cap_loop.clone(),
                face: cap_face.clone(),
                boundary_role: cadmpeg_ir::topology::LoopBoundaryRole::Outer,
                coedges: vec![cap_coedge.clone()],
                vertex_uses: Vec::new(),
            });
            ir.model.coedges.push(Coedge {
                id: cap_coedge.clone(),
                owner_loop: cap_loop.clone(),
                edge: edge_id.clone(),
                next: cap_coedge.clone(),
                previous: cap_coedge.clone(),
                radial_next: side_coedge.clone(),
                sense: if side_index == 0 {
                    Sense::Reversed
                } else {
                    Sense::Forward
                },
                pcurves: vec![PcurveUse {
                    pcurve: cap_pcurve,
                    isoparametric: None,
                    parameter_range: None,
                }],
                use_curve: None,
                use_curve_parameter_range: None,
            });
            ir.model.faces.push(Face {
                id: cap_face.clone(),
                shell: shell_id.clone(),
                surface: cap_surface,
                sense: if side_index == 0 {
                    Sense::Reversed
                } else {
                    Sense::Forward
                },
                loops: vec![cap_loop],
                name: None,
                color: None,
                tolerance: None,
            });
            face_ids.push(cap_face);
            cap_coedges.push(cap_coedge);
            side_coedges.push((side_coedge, edge_id, side_pcurve));
        }
        let side_surface = SurfaceId(format!("{prefix}:surface:side"));
        let side_face = FaceId(format!("{prefix}:face:side"));
        let mut side_loops = Vec::new();
        ir.model.surfaces.push(Surface {
            id: side_surface.clone(),
            geometry: SurfaceGeometry::Cylinder {
                origin: Point3::new(center[0], center[1], center[2]),
                axis: Vector3::new(
                    transform.normal[0],
                    transform.normal[1],
                    transform.normal[2],
                ),
                ref_direction: Vector3::new(
                    transform.u_axis[0],
                    transform.u_axis[1],
                    transform.u_axis[2],
                ),
                radius,
            },
            source_object: None,
        });
        for (side_index, ((side, _), (coedge, edge, pcurve))) in
            sides.into_iter().zip(side_coedges).enumerate()
        {
            let loop_id = LoopId(format!("{prefix}:loop:side:{side}"));
            ir.model.loops.push(IrLoop {
                id: loop_id.clone(),
                face: side_face.clone(),
                boundary_role: cadmpeg_ir::topology::LoopBoundaryRole::Unspecified,
                coedges: vec![coedge.clone()],
                vertex_uses: Vec::new(),
            });
            ir.model.coedges.push(Coedge {
                id: coedge.clone(),
                owner_loop: loop_id.clone(),
                edge,
                next: coedge.clone(),
                previous: coedge.clone(),
                radial_next: cap_coedges[side_index].clone(),
                sense: if side_index == 0 {
                    Sense::Forward
                } else {
                    Sense::Reversed
                },
                pcurves: vec![PcurveUse {
                    pcurve,
                    isoparametric: None,
                    parameter_range: None,
                }],
                use_curve: None,
                use_curve_parameter_range: None,
            });
            side_loops.push(loop_id);
        }
        ir.model.faces.push(Face {
            id: side_face.clone(),
            shell: shell_id.clone(),
            surface: side_surface,
            sense: Sense::Forward,
            loops: side_loops,
            name: None,
            color: None,
            tolerance: None,
        });
        face_ids.push(side_face);
        ir.model.shells.push(Shell {
            id: shell_id.clone(),
            region: region_id.clone(),
            faces: face_ids,
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

pub(in super::super) fn resolved_circular_extrusion_profile(
    scan: &ContainerScan,
    ir: &CadIr,
    transform: &crate::placement::FeatureSectionTransform,
    feature_id: u32,
    sketch_id: &SketchId,
) -> Option<([f64; 2], f64)> {
    if let Some(sketch) = exactly_one(
        ir.model
            .sketches
            .iter()
            .filter(|sketch| sketch.id == *sketch_id),
    ) {
        if let [profile] = sketch.profiles.as_slice() {
            if let [entity_use] = profile.as_slice() {
                if let Some(SketchGeometry::Circle { center, radius }) =
                    exactly_one(ir.model.sketch_entities.iter().filter(|entity| {
                        entity.id == entity_use.entity && entity.sketch == *sketch_id
                    }))
                    .map(|entity| &entity.geometry)
                {
                    return Some(([center.u, center.v], radius.0));
                }
            }
        }
    }
    let sweep = circular_sweep_geometry(scan, feature_id)?;
    sweep
        .section_definition_id
        .is_none_or(|definition_id| definition_id == transform.definition_id)
        .then_some(())?;
    circular_section_profile_from_cylinder(transform, &sweep.geometry)
}

pub(in super::super) fn circular_section_profile_from_cylinder(
    transform: &crate::placement::FeatureSectionTransform,
    geometry: &SurfaceGeometry,
) -> Option<([f64; 2], f64)> {
    let SurfaceGeometry::Cylinder {
        origin,
        axis,
        radius,
        ..
    } = geometry
    else {
        return None;
    };
    let axis = normalized([axis.x, axis.y, axis.z])?;
    (dot(axis, transform.normal).abs() >= 1.0 - EPS_AXIS_ALIGNMENT
        && radius.is_finite()
        && *radius > 0.0)
        .then_some(())?;
    let delta = [
        origin.x - transform.origin[0],
        origin.y - transform.origin[1],
        origin.z - transform.origin[2],
    ];
    Some((
        [dot(delta, transform.u_axis), dot(delta, transform.v_axis)],
        *radius,
    ))
}
