// SPDX-License-Identifier: Apache-2.0
//! Bake IR body placements into SLDPRT model-space geometry.

use std::collections::HashMap;

use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::{
    CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::CadIr;

pub(crate) fn bake(ir: &mut CadIr) -> Result<(), CodecError> {
    if !ir.model.bodies.iter().any(|body| {
        body.transform
            .is_some_and(|value| value != Transform::identity())
    }) {
        return Ok(());
    }

    let regions = ir
        .model
        .regions
        .iter()
        .map(|value| (value.id.as_str(), value))
        .collect::<HashMap<_, _>>();
    let shells = ir
        .model
        .shells
        .iter()
        .map(|value| (value.id.as_str(), value))
        .collect::<HashMap<_, _>>();
    let faces = ir
        .model
        .faces
        .iter()
        .map(|value| (value.id.as_str(), value))
        .collect::<HashMap<_, _>>();
    let loops = ir
        .model
        .loops
        .iter()
        .map(|value| (value.id.as_str(), value))
        .collect::<HashMap<_, _>>();
    let coedges = ir
        .model
        .coedges
        .iter()
        .map(|value| (value.id.as_str(), value))
        .collect::<HashMap<_, _>>();
    let edges = ir
        .model
        .edges
        .iter()
        .map(|value| (value.id.as_str(), value))
        .collect::<HashMap<_, _>>();
    let vertices = ir
        .model
        .vertices
        .iter()
        .map(|value| (value.id.as_str(), value))
        .collect::<HashMap<_, _>>();

    let mut point_transforms = HashMap::new();
    let mut surface_transforms = HashMap::new();
    let mut curve_transforms = HashMap::new();
    for body in &ir.model.bodies {
        let transform = body.transform.unwrap_or_default();
        check_rigid(transform)?;
        for region_id in &body.regions {
            let region = regions
                .get(region_id.as_str())
                .ok_or_else(|| CodecError::Malformed("body references missing region".into()))?;
            for shell_id in &region.shells {
                let shell = shells.get(shell_id.as_str()).ok_or_else(|| {
                    CodecError::Malformed("region references missing shell".into())
                })?;
                for face_id in shell.faces() {
                    let face = faces.get(face_id.as_str()).ok_or_else(|| {
                        CodecError::Malformed("shell references missing face".into())
                    })?;
                    assign(&mut surface_transforms, face.surface.as_str(), transform)?;
                    for loop_id in &face.loops {
                        let lp = loops.get(loop_id.as_str()).ok_or_else(|| {
                            CodecError::Malformed("face references missing loop".into())
                        })?;
                        for coedge_id in lp.coedges() {
                            let coedge = coedges.get(coedge_id.as_str()).ok_or_else(|| {
                                CodecError::Malformed("loop references missing coedge".into())
                            })?;
                            let edge = edges.get(coedge.edge.as_str()).ok_or_else(|| {
                                CodecError::Malformed("coedge references missing edge".into())
                            })?;
                            if let Some(curve) = &edge.curve() {
                                assign(&mut curve_transforms, curve.as_str(), transform)?;
                            }
                            for vertex_id in [&edge.start, &edge.end] {
                                let vertex = vertices.get(vertex_id.as_str()).ok_or_else(|| {
                                    CodecError::Malformed("edge references missing vertex".into())
                                })?;
                                assign(&mut point_transforms, vertex.point.as_str(), transform)?;
                            }
                        }
                    }
                }
            }
        }
    }

    for point in &mut ir.model.points {
        if let Some(transform) = point_transforms.get(point.id.as_str()) {
            point.position = transform.apply_point(point.position);
        }
    }
    for surface in &mut ir.model.surfaces {
        let Some(transform) = surface_transforms.get(surface.id.as_str()).copied() else {
            continue;
        };
        transform_surface(&mut surface.geometry, transform)?;
    }
    for curve in &mut ir.model.curves {
        let Some(transform) = curve_transforms.get(curve.id.as_str()).copied() else {
            continue;
        };
        transform_curve(&mut curve.geometry, transform)?;
    }
    if !ir.model.tessellations.is_empty() {
        let transforms = ir
            .model
            .bodies
            .iter()
            .map(|body| (body.id.clone(), body.transform.unwrap_or_default()))
            .collect::<HashMap<_, _>>();
        for mesh in &mut ir.model.tessellations {
            let transform = match &mesh.body {
                Some(body) => transforms.get(body).copied().ok_or_else(|| {
                    CodecError::Malformed("tessellation references missing body".into())
                })?,
                None if transforms.len() == 1 => *transforms
                    .values()
                    .next()
                    .expect("one body transform exists"),
                None => {
                    return Err(CodecError::NotImplemented(
                        "SLDPRT cannot assign an unowned tessellation to transformed bodies".into(),
                    ))
                }
            };
            mesh.edit_vertices(|point| *point = transform.apply_point(*point))
                .map_err(|error| {
                    CodecError::malformed(format_args!("invalid transformed tessellation: {error}"))
                })?;
            if !mesh.vertex_normals().is_empty() || !mesh.per_corner_normals().is_empty() {
                mesh.edit_normals(|normal| *normal = transform.apply_vector(*normal))
                    .map_err(|error| {
                        CodecError::malformed(format_args!(
                            "invalid transformed tessellation: {error}"
                        ))
                    })?;
            }
        }
    }
    ir.model
        .bodies
        .iter_mut()
        .for_each(|body| body.transform = None);
    Ok(())
}

fn assign(
    assignments: &mut HashMap<String, Transform>,
    id: &str,
    transform: Transform,
) -> Result<(), CodecError> {
    if assignments
        .insert(id.to_string(), transform)
        .is_some_and(|current| current != transform)
    {
        return Err(CodecError::NotImplemented(format!(
            "entity {id} is shared by bodies with different transforms"
        )));
    }
    Ok(())
}

fn check_rigid(transform: Transform) -> Result<(), CodecError> {
    if !transform.is_proper_rigid() {
        return Err(CodecError::NotImplemented(
            "SLDPRT body transform must be a right-handed rigid transform".into(),
        ));
    }
    Ok(())
}

fn transform_surface(
    geometry: &mut SurfaceGeometry,
    transform: Transform,
) -> Result<(), CodecError> {
    match geometry {
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface)) => {
            let origin = plane_surface.origin();
            let normal = plane_surface.normal();
            let u_axis = plane_surface.u_axis();
            *plane_surface = cadmpeg_ir::geometry::PlaneSurface::try_new(
                transform.apply_point(*origin),
                transform.apply_vector(*normal),
                transform.apply_vector(*u_axis),
            )
            .map_err(CodecError::malformed)?;
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface)) => {
            let origin = cylinder_surface.origin();
            let axis = cylinder_surface.axis();
            let ref_direction = cylinder_surface.ref_direction();
            let radius = cylinder_surface.radius();
            *cylinder_surface = cadmpeg_ir::geometry::CylinderSurface::try_new(
                transform.apply_point(*origin),
                transform.apply_vector(*axis),
                transform.apply_vector(*ref_direction),
                radius,
            )
            .map_err(CodecError::malformed)?;
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(cone_surface)) => {
            let origin = cone_surface.origin();
            let axis = cone_surface.axis();
            let ref_direction = cone_surface.ref_direction();
            let radius = cone_surface.radius();
            let ratio = cone_surface.ratio();
            let half_angle = cone_surface.half_angle();
            *cone_surface = cadmpeg_ir::geometry::ConeSurface::try_new(
                transform.apply_point(*origin),
                transform.apply_vector(*axis),
                transform.apply_vector(*ref_direction),
                radius,
                ratio,
                half_angle,
            )
            .map_err(CodecError::malformed)?;
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(sphere_surface)) => {
            let center = sphere_surface.center();
            let axis = sphere_surface.axis();
            let ref_direction = sphere_surface.ref_direction();
            let radius = sphere_surface.radius();
            *sphere_surface = cadmpeg_ir::geometry::SphereSurface::try_new(
                transform.apply_point(*center),
                transform.apply_vector(*axis),
                transform.apply_vector(*ref_direction),
                radius,
            )
            .map_err(CodecError::malformed)?;
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(torus_surface)) => {
            let center = torus_surface.center();
            let axis = torus_surface.axis();
            let ref_direction = torus_surface.ref_direction();
            let major_radius = torus_surface.major_radius();
            let minor_radius = torus_surface.minor_radius();
            *torus_surface = cadmpeg_ir::geometry::TorusSurface::try_new(
                transform.apply_point(*center),
                transform.apply_vector(*axis),
                transform.apply_vector(*ref_direction),
                major_radius,
                minor_radius,
            )
            .map_err(CodecError::malformed)?;
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(nurbs)) => nurbs
            .edit_control_points(|point| {
                *point = transform.apply_point(*point);
            })
            .map_err(|error| {
                CodecError::malformed(format_args!("invalid transformed NURBS: {error}"))
            })?,
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Polygonal(surface)) => surface
            .edit_vertices(|points| {
                for point in points {
                    *point = transform.apply_point(*point);
                }
            })
            .map_err(|error| CodecError::malformed(error.to_string()))?,
        SurfaceGeometry::Procedural { .. }
        | SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { .. }) => {
            return Err(CodecError::NotImplemented(
                "SLDPRT cannot transform a non-explicit surface".into(),
            ))
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Transformed {
            transform: carrier, ..
        }) => {
            *carrier = transform.compose(*carrier).map_err(|error| {
                CodecError::malformed(format_args!("invalid transformed carrier: {error}"))
            })?;
        }
    }
    Ok(())
}

fn transform_curve(geometry: &mut CurveGeometry, transform: Transform) -> Result<(), CodecError> {
    match geometry {
        CurveGeometry::Solved(SolvedCurveGeometry::Line(line_curve)) => {
            let origin = line_curve.origin();
            let direction = line_curve.direction();
            *line_curve = cadmpeg_ir::geometry::LineCurve::try_new(
                transform.apply_point(*origin),
                transform.apply_vector(*direction),
            )
            .map_err(CodecError::malformed)?;
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve)) => {
            let center = circle_curve.center();
            let axis = circle_curve.axis();
            let ref_direction = circle_curve.ref_direction();
            let radius = circle_curve.radius();
            *circle_curve = cadmpeg_ir::geometry::CircleCurve::try_new(
                transform.apply_point(*center),
                transform.apply_vector(*axis),
                transform.apply_vector(*ref_direction),
                radius,
            )
            .map_err(CodecError::malformed)?;
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(ellipse_curve)) => {
            let center = ellipse_curve.center();
            let axis = ellipse_curve.axis();
            let major_direction = ellipse_curve.major_direction();
            let major_radius = ellipse_curve.major_radius();
            let minor_radius = ellipse_curve.minor_radius();
            *ellipse_curve = cadmpeg_ir::geometry::EllipseCurve::try_new(
                transform.apply_point(*center),
                transform.apply_vector(*axis),
                transform.apply_vector(*major_direction),
                major_radius,
                minor_radius,
            )
            .map_err(CodecError::malformed)?;
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)) => nurbs
            .edit_control_points(|point| {
                *point = transform.apply_point(*point);
            })
            .map_err(|error| {
                CodecError::malformed(format_args!("invalid transformed NURBS: {error}"))
            })?,
        CurveGeometry::Solved(SolvedCurveGeometry::Polyline(polyline)) => polyline
            .edit_samples(|samples| {
                samples.edit_points(|point| *point = transform.apply_point(*point));
            })
            .map_err(|error| CodecError::malformed(error.to_string()))?,
        CurveGeometry::Solved(SolvedCurveGeometry::Parabola(parabola_curve)) => {
            let vertex = parabola_curve.vertex();
            let axis = parabola_curve.axis();
            let major_direction = parabola_curve.major_direction();
            let focal_distance = parabola_curve.focal_distance();
            *parabola_curve = cadmpeg_ir::geometry::ParabolaCurve::try_new(
                transform.apply_point(*vertex),
                transform.apply_vector(*axis),
                transform.apply_vector(*major_direction),
                focal_distance,
            )
            .map_err(CodecError::malformed)?;
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Hyperbola(hyperbola_curve)) => {
            let center = hyperbola_curve.center();
            let axis = hyperbola_curve.axis();
            let major_direction = hyperbola_curve.major_direction();
            let major_radius = hyperbola_curve.major_radius();
            let minor_radius = hyperbola_curve.minor_radius();
            *hyperbola_curve = cadmpeg_ir::geometry::HyperbolaCurve::try_new(
                transform.apply_point(*center),
                transform.apply_vector(*axis),
                transform.apply_vector(*major_direction),
                major_radius,
                minor_radius,
            )
            .map_err(CodecError::malformed)?;
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Degenerate(degenerate_curve)) => {
            let point = degenerate_curve.point();
            *degenerate_curve =
                cadmpeg_ir::geometry::DegenerateCurve::try_new(transform.apply_point(*point))
                    .map_err(CodecError::malformed)?;
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Composite { .. }) => {}
        CurveGeometry::Solved(SolvedCurveGeometry::Transformed {
            transform: carrier, ..
        }) => {
            *carrier = transform.compose(*carrier).map_err(|error| {
                CodecError::malformed(format_args!("invalid transformed carrier: {error}"))
            })?;
        }
        CurveGeometry::Procedural { .. }
        | CurveGeometry::Solved(SolvedCurveGeometry::Unknown { .. }) => {
            return Err(CodecError::NotImplemented(
                "cannot bake a transform into a non-explicit curve".into(),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transform_curve_rejects_non_explicit_geometry() {
        let mut geometry = CurveGeometry::Solved(SolvedCurveGeometry::Unknown { record: None });
        assert!(matches!(
            transform_curve(&mut geometry, Transform::identity()),
            Err(CodecError::NotImplemented(_))
        ));
    }

    #[test]
    fn circle_body_rotation_transforms_the_zero_angle_direction() {
        use cadmpeg_ir::geometry::CircleCurve;
        use cadmpeg_ir::math::{Point3, Vector3};

        let mut geometry = CurveGeometry::Solved(SolvedCurveGeometry::Circle(
            CircleCurve::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                2.0,
            )
            .unwrap(),
        ));
        let rotation = Transform::affine([
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [-1.0, 0.0, 0.0, 0.0],
        ])
        .unwrap();
        transform_curve(&mut geometry, rotation).unwrap();
        assert_eq!(
            cadmpeg_ir::eval::curve_point(&geometry, 0.0),
            Some(Point3::new(0.0, 0.0, -2.0))
        );
    }
}
