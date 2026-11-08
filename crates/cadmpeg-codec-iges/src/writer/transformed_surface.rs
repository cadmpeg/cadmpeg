// SPDX-License-Identifier: Apache-2.0

use super::{placement_entities, surface_entities, DecodeContext, Entity, Placement};
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::{PlacedSurface, SolvedSurfaceGeometry};
use cadmpeg_ir::scalar::FiniteReal;

pub(super) fn entities(
    ctx: &DecodeContext<'_>,
    placed: &PlacedSurface,
    base_index: usize,
    version: crate::IgesVersion,
) -> Result<Vec<Entity>, CodecError> {
    let unsupported =
        || CodecError::InvalidInput("IGES cannot preserve this surface placement".into());
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
