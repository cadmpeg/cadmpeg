// SPDX-License-Identifier: Apache-2.0
//! Resolve the first face carrier and compare fixed plane geometry.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::analytic::PlaneSurface;
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, Surface};
use cadmpeg_ir::ids::FaceId;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::sketches::Sketch;
use cadmpeg_ir::topology::Face;

pub(super) fn plane_for_face(
    ctx: &DecodeContext<'_>,
    candidate: &FaceId,
    faces: &[Face],
    surfaces: &[Surface],
) -> Result<Option<(usize, PlaneSurface)>, CodecError> {
    ctx.charge_work(0, "resolve F3D face plane")?;
    if faces.is_empty() || surfaces.is_empty() {
        return Ok(None);
    }
    let Some((index, face)) = ctx.find_map(
        faces.iter().enumerate(),
        |(index, face)| Ok(ctx.equal_bytes(face.id.as_str().as_bytes(), candidate.as_str().as_bytes(),
            "compare F3D plane face identity")?.then_some((index, face))),
        "visit F3D plane face",
    )? else {
        return Ok(None);
    };
    let Some(surface) = ctx.find_by(
        surfaces,
        |surface| ctx.equal_bytes(surface.id.as_str().as_bytes(), face.surface.as_str().as_bytes(),
            "compare F3D plane surface identity"),
        "visit F3D plane surface",
    )? else {
        return Ok(None);
    };
    Ok(match surface.geometry.solved() {
        Some(SolvedSurfaceGeometry::Plane(plane)) => Some((index, *plane)),
        _ => None,
    })
}

pub(super) fn face_coincident_with_sketch(
    ctx: &DecodeContext<'_>,
    candidate: &FaceId,
    sketch: &Sketch,
    faces: &[Face],
    surfaces: &[Surface],
    linear_tolerance: f64,
    angular_tolerance: f64,
) -> Result<bool, CodecError> {
    let Some((_, plane_surface)) = plane_for_face(ctx, candidate, faces, surfaces)? else {
        return Ok(false);
    };
    let origin = plane_surface.origin().get();
    let normal = plane_surface.frame().axis().as_raw();
    let Some((sketch_origin, sketch_normal, _)) = sketch.resolved_placement() else {
        return Ok(false);
    };
    Ok(parallel_vectors(*normal, sketch_normal.get(), angular_tolerance)
        && point_plane_distance(origin, sketch_origin.get(), sketch_normal.get())
            <= linear_tolerance)
}

pub(super) fn parallel_vectors(left: Vector3, right: Vector3, tolerance: f64) -> bool {
    let left_length = left.norm();
    let right_length = right.norm();
    let cross_length = left.cross(right).norm();
    left_length > 0.0
        && right_length > 0.0
        && cross_length <= tolerance * left_length * right_length
}

fn point_plane_distance(point: Point3, origin: Point3, normal: Vector3) -> f64 {
    let normal_length = normal.norm();
    if normal_length == 0.0 {
        return f64::INFINITY;
    }
    point.vector_from(origin).dot(normal).abs() / normal_length
}

#[cfg(test)]
mod tests;
