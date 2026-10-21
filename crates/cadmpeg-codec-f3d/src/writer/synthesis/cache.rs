// SPDX-License-Identifier: Apache-2.0
//! Make derived target caches explicit in the current synthesized document.

use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{
    ProceduralSurfaceDefinition, RevisionCacheForm, SolvedSurfaceGeometry, SurfaceGeometry,
};
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
            let construction_index = target
                .model
                .procedural_surfaces
                .iter()
                .position(|row| row.id == *construction)
                .ok_or_else(|| CodecError::malformed("cache construction disappeared"))?;
            let mut generated_contract = procedural.clone();
            let revision_form = match generated_contract.definition() {
                ProceduralSurfaceDefinition::Extrusion(payload) => payload.revision_form(),
                ProceduralSurfaceDefinition::Sum(payload) => payload.revision_form(),
                ProceduralSurfaceDefinition::Revolution(payload) => payload.revision_form(),
                _ => None,
            };
            let contract_added = revision_form
                .is_some_and(|form| matches!(form.cache, RevisionCacheForm::Absent {}));
            if generated_contract.cache_fit_tolerance().is_none() && !contract_added {
                // Legacy extrusion, sum and ruled forms have a genuine native
                // cacheless encoding. Parameterization retains its own enums.
                // Legacy revolution needs a cache, but its fit field is optional.
                let legacy_revolution = matches!(
                    generated_contract.definition(),
                    ProceduralSurfaceDefinition::Revolution(payload)
                        if payload.revision_form().is_none()
                );
                if !legacy_revolution {
                    continue;
                }
            }
            if contract_added {
                // A target cache gets a new contract from the current document,
                // never from a rejected source cache or an invented native form.
                // Parameterized forms retain their own actual enums unchanged.
                if let Err(error) =
                    generated_contract.set_cache_fit_tolerance(Some(target.tolerances.linear.get()))
                {
                    if matches!(
                        error,
                        cadmpeg_ir::geometry::CacheContractError::Parameterized
                            | cadmpeg_ir::geometry::CacheContractError::Layout(_)
                    ) {
                        continue;
                    }
                    return Err(CodecError::malformed(error));
                }
            }
            let cache = match super::super::generate::native_geometry::cache_generation::generate(
                ctx,
                target,
                &generated_contract,
            ) {
                Ok(Some(cache)) => cache,
                Ok(None) => continue,
                // A target limitation remains local synthesis admission; it
                // cannot reject independently supported sibling geometry.
                Err(error) if super::declines(&error) => continue,
                Err(error) => return Err(error),
            };
            let fit = generated_contract
                .cache_fit_tolerance()
                .map(cadmpeg_ir::geometry::FitTolerance::get);
            let contract_detail = if contract_added {
                "a new target fit contract is derived from the current document linear tolerance"
            } else {
                "the existing target fit contract is unchanged"
            };
            losses.push(F3dLossCode::WriterSurfaceCacheGenerated.note(format!(
                "surface {}: generated a NURBS target cache from its unchanged typed construction; original cache was absent, fit {fit:?} mm; {contract_detail}",
                surface.id,
            )));
            target.model.procedural_surfaces[construction_index] = generated_contract;
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
