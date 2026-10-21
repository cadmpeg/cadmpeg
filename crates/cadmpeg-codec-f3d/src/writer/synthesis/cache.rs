// SPDX-License-Identifier: Apache-2.0
//! Make derived target caches explicit in the current synthesized document.

use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use cadmpeg_ir::report::loss::LossNote;

use crate::loss::F3dLossCode;

pub(super) fn prepare(target: &mut CadIr, losses: &mut Vec<LossNote>) -> Result<(), CodecError> {
    super::design::with_context(|ctx| {
        for index in 0..target.model.surfaces.len() {
            let surface = &target.model.surfaces[index];
            let SurfaceGeometry::Procedural {
                construction,
                cache: None,
            } = &surface.geometry
            else {
                continue;
            };
            let Some(procedural) = target
                .model
                .procedural_surfaces
                .iter()
                .find(|row| row.id == *construction)
            else {
                continue;
            };
            let cache = match super::super::generate::native_geometry::cache_generation::generate(
                ctx, target, procedural,
            ) {
                Ok(Some(cache)) => cache,
                Ok(None) => continue,
                // A target limitation remains local synthesis admission; it
                // cannot reject independently supported sibling geometry.
                Err(error) if super::declines(&error) => continue,
                Err(error) => return Err(error),
            };
            losses.push(F3dLossCode::WriterSurfaceCacheGenerated.note(format!(
                "surface {}: generated a NURBS target cache from its unchanged typed construction; original cache was absent, fit {:?} mm is unchanged",
                surface.id, procedural.cache_fit_tolerance().map(cadmpeg_ir::geometry::FitTolerance::get),
            )));
            if let SurfaceGeometry::Procedural { cache: slot, .. } =
                &mut target.model.surfaces[index].geometry
            {
                *slot = Some(SolvedSurfaceGeometry::Nurbs(cache));
            }
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests;
