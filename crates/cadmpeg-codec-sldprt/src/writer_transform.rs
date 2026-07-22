// SPDX-License-Identifier: Apache-2.0
//! Bake IR body placements into SLDPRT model-space geometry.

use std::collections::HashMap;

use cadmpeg_ir::codec::CodecError;
use cadmpeg_ir::geometry::{CurveGeometry, SurfaceGeometry};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::CadIr;

pub fn bake(ir: &mut CadIr) -> Result<(), CodecError> {
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
        .map(|value| (value.id.0.as_str(), value))
        .collect::<HashMap<_, _>>();
    let shells = ir
        .model
        .shells
        .iter()
        .map(|value| (value.id.0.as_str(), value))
        .collect::<HashMap<_, _>>();
    let faces = ir
        .model
        .faces
        .iter()
        .map(|value| (value.id.0.as_str(), value))
        .collect::<HashMap<_, _>>();
    let loops = ir
        .model
        .loops
        .iter()
        .map(|value| (value.id.0.as_str(), value))
        .collect::<HashMap<_, _>>();
    let coedges = ir
        .model
        .coedges
        .iter()
        .map(|value| (value.id.0.as_str(), value))
        .collect::<HashMap<_, _>>();
    let edges = ir
        .model
        .edges
        .iter()
        .map(|value| (value.id.0.as_str(), value))
        .collect::<HashMap<_, _>>();
    let vertices = ir
        .model
        .vertices
        .iter()
        .map(|value| (value.id.0.as_str(), value))
        .collect::<HashMap<_, _>>();

    let mut point_transforms = HashMap::new();
    let mut surface_transforms = HashMap::new();
    let mut curve_transforms = HashMap::new();
    for body in &ir.model.bodies {
        let transform = body.transform.unwrap_or_default();
        check_rigid(transform)?;
        for region_id in &body.regions {
            let region = regions
                .get(region_id.0.as_str())
                .ok_or_else(|| CodecError::Malformed("body references missing region".into()))?;
            for shell_id in &region.shells {
                let shell = shells.get(shell_id.0.as_str()).ok_or_else(|| {
                    CodecError::Malformed("region references missing shell".into())
                })?;
                for face_id in &shell.faces {
                    let face = faces.get(face_id.0.as_str()).ok_or_else(|| {
                        CodecError::Malformed("shell references missing face".into())
                    })?;
                    assign(&mut surface_transforms, &face.surface.0, transform)?;
                    for loop_id in &face.loops {
                        let lp = loops.get(loop_id.0.as_str()).ok_or_else(|| {
                            CodecError::Malformed("face references missing loop".into())
                        })?;
                        for coedge_id in &lp.coedges {
                            let coedge = coedges.get(coedge_id.0.as_str()).ok_or_else(|| {
                                CodecError::Malformed("loop references missing coedge".into())
                            })?;
                            let edge = edges.get(coedge.edge.0.as_str()).ok_or_else(|| {
                                CodecError::Malformed("coedge references missing edge".into())
                            })?;
                            if let Some(curve) = &edge.curve {
                                assign(&mut curve_transforms, &curve.0, transform)?;
                            }
                            for vertex_id in [&edge.start, &edge.end] {
                                let vertex =
                                    vertices.get(vertex_id.0.as_str()).ok_or_else(|| {
                                        CodecError::Malformed(
                                            "edge references missing vertex".into(),
                                        )
                                    })?;
                                assign(&mut point_transforms, &vertex.point.0, transform)?;
                            }
                        }
                    }
                }
            }
        }
    }

    for point in &mut ir.model.points {
        if let Some(transform) = point_transforms.get(point.id.0.as_str()) {
            point.position = transform_point(*transform, point.position);
        }
    }
    for surface in &mut ir.model.surfaces {
        let Some(transform) = surface_transforms.get(surface.id.0.as_str()).copied() else {
            continue;
        };
        transform_surface(&mut surface.geometry, transform)?;
    }
    for curve in &mut ir.model.curves {
        let Some(transform) = curve_transforms.get(curve.id.0.as_str()).copied() else {
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
            mesh.vertices
                .iter_mut()
                .for_each(|point| *point = transform_point(transform, *point));
            mesh.normals
                .iter_mut()
                .for_each(|normal| *normal = transform_vector(transform, *normal));
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
    const EPS: f64 = 1e-9;
    if transform
        .rows
        .iter()
        .flatten()
        .any(|value| !value.is_finite())
    {
        return Err(CodecError::NotImplemented(
            "SLDPRT body transform contains a non-finite value".into(),
        ));
    }
    if transform.rows[3]
        .iter()
        .zip([0.0, 0.0, 0.0, 1.0])
        .any(|(actual, expected)| (*actual - expected).abs() > EPS)
    {
        return Err(CodecError::NotImplemented(
            "SLDPRT body transform is not affine".into(),
        ));
    }
    let rows = [
        Vector3::new(
            transform.rows[0][0],
            transform.rows[0][1],
            transform.rows[0][2],
        ),
        Vector3::new(
            transform.rows[1][0],
            transform.rows[1][1],
            transform.rows[1][2],
        ),
        Vector3::new(
            transform.rows[2][0],
            transform.rows[2][1],
            transform.rows[2][2],
        ),
    ];
    if rows.iter().any(|row| (row.norm() - 1.0).abs() > EPS)
        || dot(rows[0], rows[1]).abs() > EPS
        || dot(rows[0], rows[2]).abs() > EPS
        || dot(rows[1], rows[2]).abs() > EPS
        || (dot(rows[0], cross(rows[1], rows[2])) - 1.0).abs() > EPS
    {
        return Err(CodecError::NotImplemented(
            "SLDPRT body transform must be a right-handed rigid transform".into(),
        ));
    }
    Ok(())
}

fn transform_point(transform: Transform, point: Point3) -> Point3 {
    Point3::new(
        transform.rows[0][0] * point.x
            + transform.rows[0][1] * point.y
            + transform.rows[0][2] * point.z
            + transform.rows[0][3],
        transform.rows[1][0] * point.x
            + transform.rows[1][1] * point.y
            + transform.rows[1][2] * point.z
            + transform.rows[1][3],
        transform.rows[2][0] * point.x
            + transform.rows[2][1] * point.y
            + transform.rows[2][2] * point.z
            + transform.rows[2][3],
    )
}

fn transform_vector(transform: Transform, vector: Vector3) -> Vector3 {
    Vector3::new(
        transform.rows[0][0] * vector.x
            + transform.rows[0][1] * vector.y
            + transform.rows[0][2] * vector.z,
        transform.rows[1][0] * vector.x
            + transform.rows[1][1] * vector.y
            + transform.rows[1][2] * vector.z,
        transform.rows[2][0] * vector.x
            + transform.rows[2][1] * vector.y
            + transform.rows[2][2] * vector.z,
    )
}

fn transform_surface(
    geometry: &mut SurfaceGeometry,
    transform: Transform,
) -> Result<(), CodecError> {
    match geometry {
        SurfaceGeometry::Plane {
            origin,
            normal,
            u_axis,
        } => {
            *origin = transform_point(transform, *origin);
            *normal = transform_vector(transform, *normal);
            *u_axis = transform_vector(transform, *u_axis);
        }
        SurfaceGeometry::Cylinder {
            origin,
            axis,
            ref_direction,
            ..
        }
        | SurfaceGeometry::Cone {
            origin,
            axis,
            ref_direction,
            ..
        } => {
            *origin = transform_point(transform, *origin);
            *axis = transform_vector(transform, *axis);
            *ref_direction = transform_vector(transform, *ref_direction);
        }
        SurfaceGeometry::Sphere {
            center,
            axis,
            ref_direction,
            ..
        } => {
            *center = transform_point(transform, *center);
            *axis = transform_vector(transform, *axis);
            *ref_direction = transform_vector(transform, *ref_direction);
        }
        SurfaceGeometry::Torus {
            center,
            axis,
            ref_direction,
            ..
        } => {
            *center = transform_point(transform, *center);
            *axis = transform_vector(transform, *axis);
            *ref_direction = transform_vector(transform, *ref_direction);
        }
        SurfaceGeometry::Nurbs(nurbs) => nurbs
            .control_points
            .iter_mut()
            .for_each(|point| *point = transform_point(transform, *point)),
        SurfaceGeometry::Polygonal { vertices, .. } => vertices
            .iter_mut()
            .for_each(|point| *point = transform_point(transform, *point)),
        SurfaceGeometry::Procedural { .. } | SurfaceGeometry::Unknown { .. } => {
            return Err(CodecError::NotImplemented(
                "SLDPRT cannot transform a non-explicit surface".into(),
            ))
        }
        SurfaceGeometry::Transformed {
            transform: carrier, ..
        } => *carrier = multiply(transform, *carrier),
    }
    Ok(())
}

fn transform_curve(geometry: &mut CurveGeometry, transform: Transform) -> Result<(), CodecError> {
    match geometry {
        CurveGeometry::Line { origin, direction } => {
            *origin = transform_point(transform, *origin);
            *direction = transform_vector(transform, *direction);
        }
        CurveGeometry::Circle { center, axis, .. } => {
            *center = transform_point(transform, *center);
            *axis = transform_vector(transform, *axis);
        }
        CurveGeometry::Ellipse {
            center,
            axis,
            major_direction,
            ..
        } => {
            *center = transform_point(transform, *center);
            *axis = transform_vector(transform, *axis);
            *major_direction = transform_vector(transform, *major_direction);
        }
        CurveGeometry::Nurbs(nurbs) => nurbs
            .control_points
            .iter_mut()
            .for_each(|point| *point = transform_point(transform, *point)),
        CurveGeometry::Polyline { points, .. } => points
            .iter_mut()
            .for_each(|point| *point = transform_point(transform, *point)),
        CurveGeometry::Parabola {
            vertex,
            axis,
            major_direction,
            ..
        } => {
            *vertex = transform_point(transform, *vertex);
            *axis = transform_vector(transform, *axis);
            *major_direction = transform_vector(transform, *major_direction);
        }
        CurveGeometry::Hyperbola {
            center,
            axis,
            major_direction,
            ..
        } => {
            *center = transform_point(transform, *center);
            *axis = transform_vector(transform, *axis);
            *major_direction = transform_vector(transform, *major_direction);
        }
        CurveGeometry::Degenerate { point } => {
            *point = transform_point(transform, *point);
        }
        CurveGeometry::Composite { .. } => {}
        CurveGeometry::Transformed {
            transform: carrier, ..
        } => *carrier = multiply(transform, *carrier),
        CurveGeometry::Procedural { .. } | CurveGeometry::Unknown { .. } => {
            return Err(CodecError::NotImplemented(
                "cannot bake a transform into a non-explicit curve".into(),
            ));
        }
    }
    Ok(())
}

fn multiply(left: Transform, right: Transform) -> Transform {
    let mut rows = [[0.0; 4]; 4];
    for (row, values) in rows.iter_mut().enumerate() {
        for (column, value) in values.iter_mut().enumerate() {
            *value = (0..4)
                .map(|inner| left.rows[row][inner] * right.rows[inner][column])
                .sum();
        }
    }
    Transform { rows }
}

fn dot(left: Vector3, right: Vector3) -> f64 {
    left.x * right.x + left.y * right.y + left.z * right.z
}

fn cross(left: Vector3, right: Vector3) -> Vector3 {
    Vector3::new(
        left.y * right.z - left.z * right.y,
        left.z * right.x - left.x * right.z,
        left.x * right.y - left.y * right.x,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transform_curve_rejects_non_explicit_geometry() {
        let mut geometry = CurveGeometry::Unknown { record: None };
        assert!(matches!(
            transform_curve(&mut geometry, Transform::identity()),
            Err(CodecError::NotImplemented(_))
        ));
    }
}
