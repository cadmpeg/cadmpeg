// SPDX-License-Identifier: Apache-2.0
//! Subset surfaces restated as the support surface they restrict.
//!
//! A subset that keeps both parameter directions of its support and states
//! increasing ranges restricts the support's parameterization: its faces and
//! parameter curves stand on the support's parameters, and its solved carrier
//! is the support's geometry. IGES has no subset carrier, so the face writes
//! on the support's geometry and the subset definition is reduced.

use std::borrow::Cow;
use std::collections::BTreeMap;

use cadmpeg_ir::geometry::{ProceduralSurfaceDefinition, SurfaceGeometry};
use cadmpeg_ir::ids::{ProceduralSurfaceId, SurfaceId};
use cadmpeg_ir::CadIr;

use super::{IgesLossCode, LossNote};

/// `ir` with each subset surface restated on its support's solved geometry,
/// and a loss for each reduced subset definition.
pub(super) fn lower(ir: &CadIr) -> (Cow<'_, CadIr>, Vec<LossNote>) {
    let supports = ir
        .model
        .procedural_surfaces
        .iter()
        .filter_map(|procedural| match procedural.definition() {
            ProceduralSurfaceDefinition::Subset(subset)
                if *subset.u_sense() != Some(false)
                    && *subset.v_sense() != Some(false)
                    && subset.parameter_ranges().iter().all(|range| {
                        let [start, end] = range.endpoints();
                        start < end
                    }) =>
            {
                Some((procedural.id.clone(), subset.support().clone()))
            }
            _ => None,
        })
        .collect::<BTreeMap<ProceduralSurfaceId, SurfaceId>>();
    if supports.is_empty() {
        return (Cow::Borrowed(ir), Vec::new());
    }
    let mut lowered = BTreeMap::new();
    for surface in &ir.model.surfaces {
        let Some(construction) = surface.geometry.procedural_construction() else {
            continue;
        };
        if let Some(geometry) = support_geometry(ir, &supports, construction) {
            lowered.insert(surface.id.clone(), (construction.clone(), geometry));
        }
    }
    if lowered.is_empty() {
        return (Cow::Borrowed(ir), Vec::new());
    }
    let mut document = ir.clone();
    let mut losses = Vec::new();
    for surface in &mut document.model.surfaces {
        if let Some((construction, geometry)) = lowered.remove(&surface.id) {
            surface.geometry = SurfaceGeometry::Solved(geometry);
            losses.push(IgesLossCode::ProceduralReduced.note(format!(
                "IGES procedural definition {construction} was reduced to the geometry of its support surface"
            )));
            document
                .model
                .procedural_surfaces
                .retain(|procedural| procedural.id != construction);
        }
    }
    (Cow::Owned(document), losses)
}

/// The solved geometry at the end of the subset chain from `construction`.
fn support_geometry(
    ir: &CadIr,
    supports: &BTreeMap<ProceduralSurfaceId, SurfaceId>,
    construction: &ProceduralSurfaceId,
) -> Option<cadmpeg_ir::geometry::SolvedSurfaceGeometry> {
    let mut current = supports.get(construction)?;
    for _ in 0..=supports.len() {
        let surface = ir
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id == *current)?;
        match surface.geometry.procedural_construction() {
            Some(next) if supports.contains_key(next) => current = supports.get(next)?,
            _ => return surface.geometry.solved().cloned(),
        }
    }
    None
}
