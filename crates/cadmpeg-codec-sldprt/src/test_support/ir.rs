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
    use cadmpeg_ir::geometry::{
        CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
    };
    fn translate_curve_x(curve: &mut CurveGeometry, dx: f64) {
        match curve {
            CurveGeometry::Solved(SolvedCurveGeometry::Line(line_curve)) => {
                let origin = line_curve.origin();
                let direction = line_curve.direction();
                let mut origin = *origin;
                origin.x += dx;
                *line_curve = cadmpeg_ir::geometry::LineCurve::try_new(origin, *direction).unwrap();
            }
            CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve)) => {
                let center = circle_curve.center();
                let axis = circle_curve.axis();
                let ref_direction = circle_curve.ref_direction();
                let radius = circle_curve.radius();
                let mut center = *center;
                center.x += dx;
                *circle_curve = cadmpeg_ir::geometry::CircleCurve::try_new(
                    center,
                    *axis,
                    *ref_direction,
                    radius,
                )
                .unwrap();
            }
            CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(ellipse_curve)) => {
                let center = ellipse_curve.center();
                let axis = ellipse_curve.axis();
                let major_direction = ellipse_curve.major_direction();
                let major_radius = ellipse_curve.major_radius();
                let minor_radius = ellipse_curve.minor_radius();
                let mut center = *center;
                center.x += dx;
                *ellipse_curve = cadmpeg_ir::geometry::EllipseCurve::try_new(
                    center,
                    *axis,
                    *major_direction,
                    major_radius,
                    minor_radius,
                )
                .unwrap();
            }
            CurveGeometry::Solved(SolvedCurveGeometry::Hyperbola(hyperbola_curve)) => {
                let center = hyperbola_curve.center();
                let axis = hyperbola_curve.axis();
                let major_direction = hyperbola_curve.major_direction();
                let major_radius = hyperbola_curve.major_radius();
                let minor_radius = hyperbola_curve.minor_radius();
                let mut center = *center;
                center.x += dx;
                *hyperbola_curve = cadmpeg_ir::geometry::HyperbolaCurve::try_new(
                    center,
                    *axis,
                    *major_direction,
                    major_radius,
                    minor_radius,
                )
                .unwrap();
            }
            CurveGeometry::Solved(SolvedCurveGeometry::Parabola(parabola_curve)) => {
                let vertex = parabola_curve.vertex();
                let axis = parabola_curve.axis();
                let major_direction = parabola_curve.major_direction();
                let focal_distance = parabola_curve.focal_distance();
                let mut vertex = *vertex;
                vertex.x += dx;
                *parabola_curve = cadmpeg_ir::geometry::ParabolaCurve::try_new(
                    vertex,
                    *axis,
                    *major_direction,
                    focal_distance,
                )
                .unwrap();
            }
            CurveGeometry::Solved(SolvedCurveGeometry::Degenerate(degenerate_curve)) => {
                let point = degenerate_curve.point();
                let mut point = *point;
                point.x += dx;
                *degenerate_curve = cadmpeg_ir::geometry::DegenerateCurve::try_new(point).unwrap();
            }
            CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)) => {
                nurbs
                    .edit_control_points(|points| {
                        for pole in points {
                            pole.x += dx;
                        }
                    })
                    .unwrap();
            }
            CurveGeometry::Solved(SolvedCurveGeometry::Polyline(polyline)) => {
                polyline
                    .edit_points(|points| {
                        for point in points {
                            point.x += dx;
                        }
                    })
                    .unwrap();
            }
            CurveGeometry::Solved(SolvedCurveGeometry::Transformed { transform, .. }) => {
                let mut rows = transform.rows();
                rows[0][3] += dx;
                *transform =
                    cadmpeg_ir::transform::Transform::from_rows(rows).expect("affine transform");
            }
            CurveGeometry::Solved(SolvedCurveGeometry::Composite { .. }) => {}
            CurveGeometry::Procedural { .. } => {}
            CurveGeometry::Solved(SolvedCurveGeometry::Unknown { .. }) => {}
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
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface)) => {
                let origin = plane_surface.origin();
                let normal = plane_surface.normal();
                let u_axis = plane_surface.u_axis();
                let mut origin = *origin;
                origin.x += dx;
                *plane_surface =
                    cadmpeg_ir::geometry::PlaneSurface::try_new(origin, *normal, *u_axis).unwrap();
            }
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface)) => {
                let origin = cylinder_surface.origin();
                let axis = cylinder_surface.axis();
                let ref_direction = cylinder_surface.ref_direction();
                let radius = cylinder_surface.radius();
                let mut origin = *origin;
                origin.x += dx;
                *cylinder_surface = cadmpeg_ir::geometry::CylinderSurface::try_new(
                    origin,
                    *axis,
                    *ref_direction,
                    radius,
                )
                .unwrap();
            }
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(cone_surface)) => {
                let origin = cone_surface.origin();
                let axis = cone_surface.axis();
                let ref_direction = cone_surface.ref_direction();
                let radius = cone_surface.radius();
                let ratio = cone_surface.ratio();
                let half_angle = cone_surface.half_angle();
                let mut origin = *origin;
                origin.x += dx;
                *cone_surface = cadmpeg_ir::geometry::ConeSurface::try_new(
                    origin,
                    *axis,
                    *ref_direction,
                    radius,
                    ratio,
                    half_angle,
                )
                .unwrap();
            }
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(sphere_surface)) => {
                let center = sphere_surface.center();
                let axis = sphere_surface.axis();
                let ref_direction = sphere_surface.ref_direction();
                let radius = sphere_surface.radius();
                let mut center = *center;
                center.x += dx;
                *sphere_surface = cadmpeg_ir::geometry::SphereSurface::try_new(
                    center,
                    *axis,
                    *ref_direction,
                    radius,
                )
                .unwrap();
            }
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(torus_surface)) => {
                let center = torus_surface.center();
                let axis = torus_surface.axis();
                let ref_direction = torus_surface.ref_direction();
                let major_radius = torus_surface.major_radius();
                let minor_radius = torus_surface.minor_radius();
                let mut center = *center;
                center.x += dx;
                *torus_surface = cadmpeg_ir::geometry::TorusSurface::try_new(
                    center,
                    *axis,
                    *ref_direction,
                    major_radius,
                    minor_radius,
                )
                .unwrap();
            }
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(nurbs)) => {
                nurbs
                    .edit_control_points(|rows| {
                        for pole in rows.iter_mut().flatten() {
                            pole.x += dx;
                        }
                    })
                    .unwrap();
            }
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Polygonal(surface)) => {
                surface
                    .edit_vertices(|points| {
                        for point in points {
                            point.x += dx;
                        }
                    })
                    .unwrap();
            }
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Transformed { transform, .. }) => {
                let mut rows = transform.rows();
                rows[0][3] += dx;
                *transform =
                    cadmpeg_ir::transform::Transform::from_rows(rows).expect("affine transform");
            }
            SurfaceGeometry::Procedural { .. } => {}
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { .. }) => {}
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
    use cadmpeg_ir::geometry::{
        CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
    };
    use cadmpeg_ir::math::Point3;
    let shift = |p: &Point3| Point3::new(p.x + t[0], p.y + t[1], p.z + t[2]);
    for point in &mut ir.model.points {
        point.position = shift(&point.position);
    }
    for curve in &mut ir.model.curves {
        if let CurveGeometry::Solved(SolvedCurveGeometry::Line(line_curve)) = &mut curve.geometry {
            let origin = line_curve.origin();
            let direction = line_curve.direction();
            let mut origin = *origin;
            origin = shift(&origin);
            *line_curve = cadmpeg_ir::geometry::LineCurve::try_new(origin, *direction).unwrap();
        }
    }
    for surface in &mut ir.model.surfaces {
        if let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface)) =
            &mut surface.geometry
        {
            let origin = plane_surface.origin();
            let normal = plane_surface.normal();
            let u_axis = plane_surface.u_axis();
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
        .for_each(|edge| edge.set_param_range(None).unwrap());
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
