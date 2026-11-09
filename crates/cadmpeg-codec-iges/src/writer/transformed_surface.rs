// SPDX-License-Identifier: Apache-2.0

use super::{placement_entities, surface_entities, DecodeContext, Entity, Placement};
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::analytic::PlaneSurface;
use cadmpeg_ir::geometry::{PlacedSurface, SolvedSurfaceGeometry};
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::transform::Transform;

pub(super) fn entities(
    ctx: &DecodeContext<'_>,
    placed: &PlacedSurface,
    base_index: usize,
    version: crate::IgesVersion,
) -> Result<Vec<Entity>, CodecError> {
    let unsupported =
        || CodecError::InvalidInput("IGES cannot preserve this surface placement".into());
    if let SolvedSurfaceGeometry::Plane(plane) = placed.basis() {
        let world = world_plane(*placed.transform(), plane).ok_or_else(unsupported)?;
        return surface_entities(
            ctx,
            &SolvedSurfaceGeometry::Plane(world),
            base_index,
            version,
        );
    }
    if !matches!(
        placed.basis(),
        SolvedSurfaceGeometry::Plane(_)
            | SolvedSurfaceGeometry::Cylinder(_)
            | SolvedSurfaceGeometry::Cone(_)
            | SolvedSurfaceGeometry::Sphere(_)
            | SolvedSurfaceGeometry::Torus(_)
    ) {
        return Err(unsupported());
    }
    let mut rows = [[FiniteReal::ZERO; 4]; 3];
    for (target, source) in rows.iter_mut().zip(placed.transform().affine_rows()) {
        for (target, source) in target.iter_mut().zip(source) {
            *target = FiniteReal::new(source).ok_or_else(unsupported)?;
        }
    }
    let placement = Placement { rows };
    if placement_entities::matrix_form(placement) != Some(0) {
        return Err(unsupported());
    }
    let mut entities = surface_entities(ctx, placed.basis(), base_index, version)?;
    let carrier = entities.last_mut().ok_or_else(unsupported)?;
    if carrier.transform.is_some() || carrier.transform_link.is_some() {
        return Err(unsupported());
    }
    carrier.transform = Some(placement);
    Ok(entities)
}

/// `plane` under the proper-rigid `transform`: the placement applied to its
/// origin and frame, with the frame re-orthonormalized after rounding.
fn world_plane(transform: Transform, plane: &PlaneSurface) -> Option<PlaneSurface> {
    if !transform.is_proper_rigid() {
        return None;
    }
    let origin = transform.apply_point(plane.origin().get())?;
    let normal = transform
        .apply_vector(*plane.frame().axis().as_raw())?
        .get()
        .unit()?;
    let reference = transform
        .apply_vector(*plane.frame().reference().as_raw())?
        .get();
    let reference = (reference - normal.scale(reference.dot(normal))).unit()?;
    PlaneSurface::try_new(origin.get(), normal, reference).ok()
}
