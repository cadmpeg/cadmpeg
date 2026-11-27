// SPDX-License-Identifier: Apache-2.0
use std::collections::HashMap;

use cadmpeg_ir::features::evaluation::HistoricalFaceSurface;

pub(super) fn historical_face_samples(
    brep: &cadmpeg_asm::brep::AsmBrep,
) -> Vec<crate::history_records::AsmHistoricalFaceSample> {
    let surfaces = brep
        .surfaces
        .iter()
        .map(|surface| (surface.id.as_str(), surface))
        .collect::<HashMap<_, _>>();
    let mut samples = brep
        .faces
        .iter()
        .filter_map(|face| {
            let geometry = surfaces.get(face.surface.as_str())?.geometry.solved()?;
            Some(crate::history_records::AsmHistoricalFaceSample {
                face: super::body_closures::stable_ref(face.id.as_str())?,
                surface: HistoricalFaceSurface::from_solved(geometry)?,
                reversed: face.sense == cadmpeg_ir::topology::Sense::Reversed,
            })
        })
        .collect::<Vec<_>>();
    samples.sort_by_key(|sample| sample.face);
    samples
}

#[cfg(test)]
mod tests;
