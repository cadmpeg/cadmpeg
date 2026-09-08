// SPDX-License-Identifier: Apache-2.0
//! Source-less IR builders and encode/decode helpers for crate tests.
#![allow(clippy::unwrap_used)]

use cadmpeg_ir::codec::write::TargetRequest;
use std::io::Cursor;

use cadmpeg_ir::codec::write::Encoder;
use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::SldprtCodec;

/// Translate every model-space carrier along x so a forced modification stays
/// geometrically consistent: vertices remain on their edge curves and surfaces.
pub(crate) fn translate_model_x(ir: &mut cadmpeg_ir::document::CadIr, dx: f64) {
    use cadmpeg_ir::geometry::{CurveGeometry, SurfaceGeometry};
    fn translate_curve_x(curve: &mut CurveGeometry, dx: f64) {
        match curve {
            CurveGeometry::Line(line_curve) => {
                let (origin, direction) = line_curve.parts();
                let mut origin = *origin;
                origin.x += dx;
                *line_curve = cadmpeg_ir::geometry::LineCurve::try_new(origin, *direction).unwrap();
            }
            CurveGeometry::Circle(circle_curve) => {
                let (center, axis, ref_direction, radius) = circle_curve.parts();
                let mut center = *center;
                center.x += dx;
                *circle_curve = cadmpeg_ir::geometry::CircleCurve::try_new(
                    center,
                    *axis,
                    *ref_direction,
                    *radius,
                )
                .unwrap();
            }
            CurveGeometry::Ellipse(ellipse_curve) => {
                let (center, axis, major_direction, major_radius, minor_radius) =
                    ellipse_curve.parts();
                let mut center = *center;
                center.x += dx;
                *ellipse_curve = cadmpeg_ir::geometry::EllipseCurve::try_new(
                    center,
                    *axis,
                    *major_direction,
                    *major_radius,
                    *minor_radius,
                )
                .unwrap();
            }
            CurveGeometry::Hyperbola(hyperbola_curve) => {
                let (center, axis, major_direction, major_radius, minor_radius) =
                    hyperbola_curve.parts();
                let mut center = *center;
                center.x += dx;
                *hyperbola_curve = cadmpeg_ir::geometry::HyperbolaCurve::try_new(
                    center,
                    *axis,
                    *major_direction,
                    *major_radius,
                    *minor_radius,
                )
                .unwrap();
            }
            CurveGeometry::Parabola(parabola_curve) => {
                let (vertex, axis, major_direction, focal_distance) = parabola_curve.parts();
                let mut vertex = *vertex;
                vertex.x += dx;
                *parabola_curve = cadmpeg_ir::geometry::ParabolaCurve::try_new(
                    vertex,
                    *axis,
                    *major_direction,
                    *focal_distance,
                )
                .unwrap();
            }
            CurveGeometry::Degenerate(degenerate_curve) => {
                let (point,) = degenerate_curve.parts();
                let mut point = *point;
                point.x += dx;
                *degenerate_curve = cadmpeg_ir::geometry::DegenerateCurve::try_new(point).unwrap();
            }
            CurveGeometry::Nurbs(nurbs) => {
                let _ = nurbs.edit_control_points(|points| {
                    for pole in points {
                        pole.x += dx;
                    }
                });
            }
            CurveGeometry::Polyline(polyline) => {
                for point in polyline.points_mut() {
                    point.x += dx;
                }
            }
            CurveGeometry::Transformed { transform, .. } => {
                let mut rows = transform.rows();
                rows[0][3] += dx;
                *transform =
                    cadmpeg_ir::transform::Transform::from_rows(rows).expect("affine transform");
            }
            CurveGeometry::Composite { .. } => {}
            CurveGeometry::Procedural { .. } => {}
            CurveGeometry::Unknown { .. } => {}
        }
    }
    for point in &mut ir.model.points {
        point.position.x += dx;
    }
    for curve in &mut ir.model.curves {
        translate_curve_x(&mut curve.geometry, dx);
    }
    for surface in &mut ir.model.surfaces {
        match &mut surface.geometry {
            SurfaceGeometry::Plane(plane_surface) => {
                let (origin, normal, u_axis) = plane_surface.parts();
                let mut origin = *origin;
                origin.x += dx;
                *plane_surface =
                    cadmpeg_ir::geometry::PlaneSurface::try_new(origin, *normal, *u_axis).unwrap();
            }
            SurfaceGeometry::Cylinder(cylinder_surface) => {
                let (origin, axis, ref_direction, radius) = cylinder_surface.parts();
                let mut origin = *origin;
                origin.x += dx;
                *cylinder_surface = cadmpeg_ir::geometry::CylinderSurface::try_new(
                    origin,
                    *axis,
                    *ref_direction,
                    *radius,
                )
                .unwrap();
            }
            SurfaceGeometry::Cone(cone_surface) => {
                let (origin, axis, ref_direction, radius, ratio, half_angle) = cone_surface.parts();
                let mut origin = *origin;
                origin.x += dx;
                *cone_surface = cadmpeg_ir::geometry::ConeSurface::try_new(
                    origin,
                    *axis,
                    *ref_direction,
                    *radius,
                    *ratio,
                    *half_angle,
                )
                .unwrap();
            }
            SurfaceGeometry::Sphere(sphere_surface) => {
                let (center, axis, ref_direction, radius) = sphere_surface.parts();
                let mut center = *center;
                center.x += dx;
                *sphere_surface = cadmpeg_ir::geometry::SphereSurface::try_new(
                    center,
                    *axis,
                    *ref_direction,
                    *radius,
                )
                .unwrap();
            }
            SurfaceGeometry::Torus(torus_surface) => {
                let (center, axis, ref_direction, major_radius, minor_radius) =
                    torus_surface.parts();
                let mut center = *center;
                center.x += dx;
                *torus_surface = cadmpeg_ir::geometry::TorusSurface::try_new(
                    center,
                    *axis,
                    *ref_direction,
                    *major_radius,
                    *minor_radius,
                )
                .unwrap();
            }
            SurfaceGeometry::Nurbs(nurbs) => {
                let _ = nurbs.edit_control_points(|points| {
                    for pole in points {
                        pole.x += dx;
                    }
                });
            }
            SurfaceGeometry::Polygonal(surface) => {
                for vertex in surface.vertices_mut() {
                    vertex.x += dx;
                }
            }
            SurfaceGeometry::Transformed { transform, .. } => {
                let mut rows = transform.rows();
                rows[0][3] += dx;
                *transform =
                    cadmpeg_ir::transform::Transform::from_rows(rows).expect("affine transform");
            }
            SurfaceGeometry::Procedural { .. } => {}
            SurfaceGeometry::Unknown { .. } => {}
        }
    }
}

pub(crate) fn strict_options() -> DecodeOptions {
    use cadmpeg_core::decode::{DecodeMode, DecodePolicy};
    DecodeOptions {
        container_only: false,
        policy: DecodePolicy {
            mode: DecodeMode::Strict,
            ..DecodePolicy::desktop()
        },
    }
}

/// Translate every positional carrier in the model by `t`. Directions and
/// normals are invariant under translation, so a pure translation is a rigid
/// motion of the whole body.
pub(crate) fn translate_model(ir: &mut cadmpeg_ir::CadIr, t: [f64; 3]) {
    use cadmpeg_ir::geometry::{CurveGeometry, SurfaceGeometry};
    use cadmpeg_ir::math::Point3;
    let shift = |p: &Point3| Point3::new(p.x + t[0], p.y + t[1], p.z + t[2]);
    for point in &mut ir.model.points {
        point.position = shift(&point.position);
    }
    for curve in &mut ir.model.curves {
        if let CurveGeometry::Line(line_curve) = &mut curve.geometry {
            let (origin, direction) = line_curve.parts();
            let mut origin = *origin;
            origin = shift(&origin);
            *line_curve = cadmpeg_ir::geometry::LineCurve::try_new(origin, *direction).unwrap();
        }
    }
    for surface in &mut ir.model.surfaces {
        if let SurfaceGeometry::Plane(plane_surface) = &mut surface.geometry {
            let (origin, normal, u_axis) = plane_surface.parts();
            let mut origin = *origin;
            origin = shift(&origin);
            *plane_surface =
                cadmpeg_ir::geometry::PlaneSurface::try_new(origin, *normal, *u_axis).unwrap();
        }
    }
}

pub(crate) fn source_less_cube() -> cadmpeg_ir::CadIr {
    let mut ir = cadmpeg_ir::examples::unit_cube();
    ir.model.bodies[0].name = None;
    ir.model.faces.iter_mut().for_each(|face| face.name = None);
    ir.model
        .edges
        .iter_mut()
        .for_each(|edge| edge.param_range = None);
    ir
}

pub(crate) fn encode_decode_result(ir: &cadmpeg_ir::CadIr) -> cadmpeg_ir::codec::DecodeResult {
    let mut encoded = Vec::new();
    SldprtCodec
        .plan(
            cadmpeg_ir::codec::write::EncodeInput { ir, fidelity: None },
            TargetRequest::Inherit,
        )
        .and_then(|plan| plan.write_to(&mut encoded))
        .unwrap();
    SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap()
}

pub(crate) fn encode_decode(ir: &cadmpeg_ir::CadIr) -> cadmpeg_ir::CadIr {
    encode_decode_result(ir).into_parts().0
}

pub(crate) fn sorted_point_positions(ir: &cadmpeg_ir::CadIr) -> Vec<[f64; 3]> {
    let mut positions: Vec<[f64; 3]> = ir
        .model
        .points
        .iter()
        .map(|point| [point.position.x, point.position.y, point.position.z])
        .collect();
    positions.sort_by(|a, b| a.partial_cmp(b).unwrap());
    positions
}
