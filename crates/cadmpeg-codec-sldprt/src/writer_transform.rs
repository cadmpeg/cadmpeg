// SPDX-License-Identifier: Apache-2.0
//! Bake IR body placements into SLDPRT model-space geometry.

use std::collections::HashMap;

use cadmpeg_core::CodecError;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::{
    nurbs::NurbsError, sampled::GeometryLayoutError, CurveGeometry, SolvedCurveGeometry,
    SolvedSurfaceGeometry, SurfaceGeometry,
};
use cadmpeg_ir::math::Vector3;
use cadmpeg_ir::tessellation::TessellationError;
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::units::{OrthonormalFrame3, UnitVector3};
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
                            if let Some(curve) = edge.curve() {
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
            let placed = placed_point(*transform, point.position())?;
            point.set_position(placed);
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
        let mut values = transforms.values();
        let sole_transform = match (values.next(), values.next()) {
            (Some(transform), None) => Some(*transform),
            _ => None,
        };
        for mesh in &mut ir.model.tessellations {
            let transform = match &mesh.body {
                Some(body) => transforms.get(body).copied().ok_or_else(|| {
                    CodecError::Malformed("tessellation references missing body".into())
                })?,
                None => sole_transform.ok_or_else(|| {
                    CodecError::NotImplemented(
                        "SLDPRT cannot assign an unowned tessellation to transformed bodies".into(),
                    )
                })?,
            };
            mesh.edit_vertices(|point| {
                *point = transform
                    .apply_point(*point)
                    .ok_or_else(|| TessellationError::EditRefused(NON_FINITE_POINT.to_string()))?;
                Ok(())
            })
            .map_err(|error| match error {
                TessellationError::EditRefused(message) => CodecError::NotImplemented(message),
                error @ TessellationError::Admission { .. } => {
                    CodecError::malformed(format_args!("invalid transformed tessellation: {error}"))
                }
            })?;
            if !mesh.vertex_normals().is_empty() || !mesh.per_corner_normals().is_empty() {
                mesh.edit_normals(|normal| {
                    *normal = transform.apply_vector(*normal).ok_or_else(|| {
                        TessellationError::EditRefused(NON_FINITE_DIRECTION.to_string())
                    })?;
                    Ok(())
                })
                .map_err(|error| match error {
                    TessellationError::EditRefused(message) => CodecError::NotImplemented(message),
                    error @ TessellationError::Admission { .. } => CodecError::malformed(
                        format_args!("invalid transformed tessellation: {error}"),
                    ),
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

const NON_FINITE_POINT: &str = "baked body placement produced a non-finite point";
const NON_FINITE_DIRECTION: &str = "baked body placement produced a non-finite direction";

fn non_finite_point() -> CodecError {
    CodecError::NotImplemented(NON_FINITE_POINT.into())
}

fn non_finite_vector() -> CodecError {
    CodecError::NotImplemented(NON_FINITE_DIRECTION.into())
}

fn sampled_edit_error(error: GeometryLayoutError) -> CodecError {
    if let GeometryLayoutError::EditRefused(message) = error {
        CodecError::NotImplemented(message)
    } else {
        CodecError::Malformed(error.to_string())
    }
}

/// Places a point, refusing a placement that leaves the finite range.
fn placed_point(transform: Transform, point: FinitePoint3) -> Result<FinitePoint3, CodecError> {
    point.transformed(transform).ok_or_else(non_finite_point)
}

/// Places a direction, refusing a placement that leaves the finite range.
fn placed_vector(transform: Transform, vector: Vector3) -> Result<Vector3, CodecError> {
    transform.apply_vector(vector).ok_or_else(non_finite_vector)
}

fn placed_frame(
    transform: Transform,
    frame: OrthonormalFrame3,
    refusal: &'static str,
) -> Result<OrthonormalFrame3, CodecError> {
    let axis = placed_vector(transform, *frame.axis().as_raw())?;
    let reference = placed_vector(transform, *frame.reference().as_raw())?;
    OrthonormalFrame3::new(axis, reference).ok_or_else(|| CodecError::malformed(refusal))
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
            let origin = placed_point(transform, plane_surface.origin())?;
            let frame = placed_frame(
                transform,
                *plane_surface.frame(),
                "PlaneSurface.normal/u_axis must form an orthonormal frame",
            )?;
            *plane_surface = cadmpeg_ir::geometry::analytic::PlaneSurface::new(origin, frame);
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface)) => {
            let origin = placed_point(transform, cylinder_surface.origin())?;
            let frame = placed_frame(
                transform,
                *cylinder_surface.frame(),
                "CylinderSurface.axis/ref_direction must form an orthonormal frame",
            )?;
            *cylinder_surface = cadmpeg_ir::geometry::analytic::CylinderSurface::new(
                origin,
                frame,
                cylinder_surface.radius(),
            );
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(cone_surface)) => {
            let origin = placed_point(transform, cone_surface.origin())?;
            let frame = placed_frame(
                transform,
                *cone_surface.frame(),
                "ConeSurface.axis/ref_direction must form an orthonormal frame",
            )?;
            *cone_surface = cadmpeg_ir::geometry::analytic::ConeSurface::new(
                origin,
                frame,
                cone_surface.radius(),
                cone_surface.ratio(),
                cone_surface.half_angle(),
            );
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(sphere_surface)) => {
            let center = placed_point(transform, sphere_surface.center())?;
            let frame = placed_frame(
                transform,
                *sphere_surface.frame(),
                "SphereSurface.axis/ref_direction must form an orthonormal frame",
            )?;
            *sphere_surface = cadmpeg_ir::geometry::analytic::SphereSurface::new(
                center,
                frame,
                sphere_surface.radius(),
            );
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(torus_surface)) => {
            let center = placed_point(transform, torus_surface.center())?;
            let frame = placed_frame(
                transform,
                *torus_surface.frame(),
                "TorusSurface.axis/ref_direction must form an orthonormal frame",
            )?;
            *torus_surface = cadmpeg_ir::geometry::analytic::TorusSurface::new(
                center,
                frame,
                torus_surface.major_radius(),
                torus_surface.minor_radius(),
            );
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(nurbs)) => {
            nurbs
                .edit_control_points(|point| {
                    *point = transform
                        .apply_point(*point)
                        .ok_or_else(|| NurbsError::EditRefused(NON_FINITE_POINT.to_string()))?;
                    Ok(())
                })
                .map_err(|error| match error {
                    NurbsError::EditRefused(message) => CodecError::NotImplemented(message),
                    error => {
                        CodecError::malformed(format_args!("invalid transformed NURBS: {error}"))
                    }
                })?;
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Polygonal(surface)) => {
            surface
                .edit_vertices(|points| {
                    for point in points {
                        *point = transform.apply_point(*point).ok_or_else(|| {
                            GeometryLayoutError::EditRefused(NON_FINITE_POINT.to_string())
                        })?;
                    }
                    Ok(())
                })
                .map_err(sampled_edit_error)?;
        }
        SurfaceGeometry::Procedural { .. }
        | SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { .. }) => {
            return Err(CodecError::NotImplemented(
                "SLDPRT cannot transform a non-explicit surface".into(),
            ))
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Transformed(placed)) => {
            let composed = transform.compose(*placed.transform()).map_err(|error| {
                CodecError::NotImplemented(format!("invalid transformed carrier: {error}"))
            })?;
            placed.set_transform(composed);
        }
    }
    Ok(())
}

fn transform_curve(geometry: &mut CurveGeometry, transform: Transform) -> Result<(), CodecError> {
    match geometry {
        CurveGeometry::Solved(SolvedCurveGeometry::Line(line_curve)) => {
            let origin = placed_point(transform, line_curve.origin())?;
            let direction =
                UnitVector3::new(placed_vector(transform, *line_curve.direction().as_raw())?)
                    .ok_or_else(|| {
                        CodecError::malformed("LineCurve.direction must have unit length")
                    })?;
            *line_curve = cadmpeg_ir::geometry::analytic::LineCurve::new(origin, direction);
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve)) => {
            let center = placed_point(transform, circle_curve.center())?;
            let frame = placed_frame(
                transform,
                *circle_curve.frame(),
                "CircleCurve.axis/ref_direction must form an orthonormal frame",
            )?;
            *circle_curve = cadmpeg_ir::geometry::analytic::CircleCurve::new(
                center,
                frame,
                circle_curve.radius(),
            );
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(ellipse_curve)) => {
            let center = placed_point(transform, ellipse_curve.center())?;
            let frame = placed_frame(
                transform,
                *ellipse_curve.frame(),
                "EllipseCurve.axis/major_direction must form an orthonormal frame",
            )?;
            *ellipse_curve = cadmpeg_ir::geometry::analytic::EllipseCurve::try_from_parts(
                center,
                frame,
                ellipse_curve.major_radius(),
                ellipse_curve.minor_radius(),
            )
            .map_err(CodecError::malformed)?;
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)) => {
            nurbs
                .edit_control_points(|point| {
                    *point = transform
                        .apply_point(*point)
                        .ok_or_else(|| NurbsError::EditRefused(NON_FINITE_POINT.to_string()))?;
                    Ok(())
                })
                .map_err(|error| match error {
                    NurbsError::EditRefused(message) => CodecError::NotImplemented(message),
                    error => {
                        CodecError::malformed(format_args!("invalid transformed NURBS: {error}"))
                    }
                })?;
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Polyline(polyline)) => {
            polyline
                .edit_samples(|samples| {
                    samples.edit_points(|point| {
                        *point = transform.apply_point(*point).ok_or_else(|| {
                            GeometryLayoutError::EditRefused(NON_FINITE_POINT.to_string())
                        })?;
                        Ok(())
                    })
                })
                .map_err(sampled_edit_error)?;
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Parabola(parabola_curve)) => {
            let vertex = placed_point(transform, parabola_curve.vertex())?;
            let frame = placed_frame(
                transform,
                *parabola_curve.frame(),
                "ParabolaCurve.axis/major_direction must form an orthonormal frame",
            )?;
            *parabola_curve = cadmpeg_ir::geometry::analytic::ParabolaCurve::new(
                vertex,
                frame,
                parabola_curve.focal_distance(),
            );
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Hyperbola(hyperbola_curve)) => {
            let center = placed_point(transform, hyperbola_curve.center())?;
            let frame = placed_frame(
                transform,
                *hyperbola_curve.frame(),
                "HyperbolaCurve.axis/major_direction must form an orthonormal frame",
            )?;
            *hyperbola_curve = cadmpeg_ir::geometry::analytic::HyperbolaCurve::new(
                center,
                frame,
                hyperbola_curve.major_radius(),
                hyperbola_curve.minor_radius(),
            );
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Degenerate(degenerate_curve)) => {
            let point = placed_point(transform, degenerate_curve.point())?;
            *degenerate_curve = cadmpeg_ir::geometry::analytic::DegenerateCurve::new(point);
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Composite { .. }) => {}
        CurveGeometry::Solved(SolvedCurveGeometry::Transformed(placed)) => {
            let composed = transform.compose(*placed.transform()).map_err(|error| {
                CodecError::NotImplemented(format!("invalid transformed carrier: {error}"))
            })?;
            placed.set_transform(composed);
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
    use super::{placed_point, placed_vector, transform_curve, transform_surface};
    use cadmpeg_core::CodecError;
    use cadmpeg_ir::features::FinitePoint3;
    use cadmpeg_ir::geometry::analytic::LineCurve;
    use cadmpeg_ir::geometry::sampled::{PolygonalSurface, PolylineCurve, PolylineSamples};
    use cadmpeg_ir::geometry::{
        CurveGeometry, PlacedCurve, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
    };
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::transform::Transform;

    fn maximum_translation() -> Transform {
        Transform::affine([
            [1.0, 0.0, 0.0, f64::MAX],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ])
        .expect("a finite translation is admitted")
    }

    #[test]
    fn admitted_point_overflow_is_not_implemented() {
        let error = placed_point(
            maximum_translation(),
            FinitePoint3::new(Point3::new(f64::MAX, 0.0, 0.0)).expect("a finite point"),
        )
        .expect_err("the admitted operands overflow");
        assert!(matches!(error, CodecError::NotImplemented(_)));
    }

    #[test]
    fn admitted_direction_overflow_is_not_implemented() {
        let rotation = Transform::affine([
            [0.8, 0.6, 0.0, 0.0],
            [-0.6, 0.8, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ])
        .expect("a finite rotation is admitted");
        let error = placed_vector(rotation, Vector3::new(f64::MAX, f64::MAX, 0.0))
            .expect_err("the admitted operands overflow");
        assert!(matches!(error, CodecError::NotImplemented(_)));
    }

    #[test]
    fn admitted_sampled_geometry_overflow_is_not_implemented() {
        let mut surface = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Polygonal(
            PolygonalSurface::new(
                vec![
                    Point3::new(f64::MAX, 0.0, 0.0),
                    Point3::new(0.0, 1.0, 0.0),
                    Point3::new(0.0, 0.0, 1.0),
                ],
                vec![[0, 1, 2]],
                0.0,
            )
            .expect("finite polygonal geometry is admitted"),
        ));
        let error = transform_surface(&mut surface, maximum_translation())
            .expect_err("the admitted placement overflows a vertex");
        assert!(matches!(error, CodecError::NotImplemented(_)));

        let mut curve = CurveGeometry::Solved(SolvedCurveGeometry::Polyline(
            PolylineCurve::new(
                PolylineSamples::Unparameterized {
                    points: vec![Point3::new(f64::MAX, 0.0, 0.0), Point3::new(0.0, 0.0, 0.0)]
                        .try_into()
                        .expect("the polyline has samples"),
                },
                0.0,
            )
            .expect("finite polyline geometry is admitted"),
        ));
        let error = transform_curve(&mut curve, maximum_translation())
            .expect_err("the admitted placement overflows a sample");
        assert!(matches!(error, CodecError::NotImplemented(_)));
    }

    #[test]
    fn admitted_transform_composition_overflow_is_not_implemented() {
        let basis = SolvedCurveGeometry::Line(
            LineCurve::try_new(Point3::new(0.0, 0.0, 0.0), Vector3::new(1.0, 0.0, 0.0))
                .expect("a finite line is admitted"),
        );
        let placed = PlacedCurve::try_new(Box::new(basis), maximum_translation())
            .expect("a finite placement is admitted");
        let mut curve = CurveGeometry::Solved(SolvedCurveGeometry::Transformed(placed));
        let error = transform_curve(&mut curve, maximum_translation())
            .expect_err("the admitted transform composition overflows");
        assert!(matches!(error, CodecError::NotImplemented(_)));
    }

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
        use cadmpeg_ir::geometry::analytic::CircleCurve;

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
