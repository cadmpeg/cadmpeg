// SPDX-License-Identifier: Apache-2.0
//! Indexed procedural admission at explicit append-only model boundaries.

use std::collections::{HashMap, HashSet};

use cadmpeg_core::CodecError;

use super::{Model, ProceduralCarrierError};
use crate::geometry::{ProceduralCurve, ProceduralSurface};
use crate::ids::{CurveId, ProceduralCurveId, ProceduralSurfaceId, SurfaceId};

#[derive(Default)]
struct FamilyIndex {
    owners: HashMap<String, usize>,
    constructions: HashSet<String>,
    associations: HashMap<String, Vec<usize>>,
    carrier_count: usize,
    construction_count: usize,
}

impl FamilyIndex {
    fn sync<'text>(
        &mut self,
        carriers: impl ExactSizeIterator<Item = (usize, &'text str, Option<&'text str>)>,
        constructions: impl ExactSizeIterator<Item = &'text str>,
    ) -> Result<(), CodecError> {
        let carrier_count = carriers.len();
        let construction_count = constructions.len();
        if carrier_count < self.carrier_count || construction_count < self.construction_count {
            return Err(CodecError::malformed(
                "procedural admission arenas must only append",
            ));
        }
        for (slot, owner, construction) in carriers.skip(self.carrier_count) {
            self.owners.entry(owner.to_owned()).or_insert(slot);
            if let Some(construction) = construction {
                self.associate(construction, slot);
            }
        }
        for construction in constructions.skip(self.construction_count) {
            self.constructions.insert(construction.to_owned());
        }
        self.carrier_count = carrier_count;
        self.construction_count = construction_count;
        Ok(())
    }

    fn associate(&mut self, construction: &str, slot: usize) {
        self.associations
            .entry(construction.to_owned())
            .or_default()
            .push(slot);
    }

    fn owner<'text>(
        &self,
        noun: &str,
        owner: &str,
        construction: &str,
        owner_at: impl Fn(usize) -> &'text str,
    ) -> Result<usize, ProceduralCarrierError> {
        if self.constructions.contains(construction) {
            return Err(ProceduralCarrierError::new(format!(
                "procedural {noun} construction {construction} already exists"
            )));
        }
        if let Some(group) = self.associations.get(construction) {
            for &slot in group {
                let other = owner_at(slot);
                if owner != other {
                    return Err(ProceduralCarrierError::new(format!(
                        "procedural {noun} construction {construction} already owns {noun} {other}"
                    )));
                }
            }
        }
        self.owners.get(owner).copied().ok_or_else(|| {
            ProceduralCarrierError::new(format!(
                "procedural {noun} {construction} references missing {noun} {owner}"
            ))
        })
    }
}

/// Owned indexes for a scope that only appends carriers and constructions.
/// Existing carrier identities, construction references, and arena order must
/// remain unchanged. Each accepted attachment updates its indexed ownership.
/// Discard this index before moving the model or replacing, removing, or reordering arena entries.
#[derive(Default)]
pub struct ProceduralIndex {
    model: Option<*const Model>,
    curves: FamilyIndex,
    surfaces: FamilyIndex,
}

impl ProceduralIndex {
    fn bind(&mut self, model: &Model) -> Result<(), CodecError> {
        if self
            .model
            .is_some_and(|previous| !std::ptr::eq(previous, model))
        {
            return Err(CodecError::malformed(
                "procedural admission belongs to another model",
            ));
        }
        self.model = Some(model);
        Ok(())
    }

    /// Admit one curve attachment.
    pub fn add_curve(
        &mut self,
        model: &mut Model,
        owner: &CurveId,
        procedural: ProceduralCurve,
    ) -> Result<Result<(), ProceduralCarrierError>, CodecError> {
        self.bind(model)?;
        self.curves.sync(
            model.curves.iter().enumerate().map(|(slot, carrier)| {
                (
                    slot,
                    carrier.id.as_str(),
                    carrier
                        .geometry
                        .procedural_construction()
                        .map(ProceduralCurveId::as_str),
                )
            }),
            model
                .procedural_curves
                .iter()
                .map(|procedural| procedural.id.as_str()),
        )?;
        let slot =
            match self
                .curves
                .owner("curve", owner.as_str(), procedural.id.as_str(), |slot| {
                    model.curves[slot].id.as_str()
                }) {
                Ok(slot) => slot,
                Err(error) => return Ok(Err(error)),
            };
        if model.curves[slot].id.as_str() != owner.as_str() {
            return Err(CodecError::malformed("procedural admission prefix changed"));
        }
        let associated = model.curves[slot]
            .geometry
            .procedural_construction()
            .is_some();
        let result = model.attach_procedural_curve(owner, slot, procedural);
        if result.is_ok() {
            let construction = model
                .procedural_curves
                .last()
                .ok_or_else(|| {
                    CodecError::malformed("procedural attachment appended no construction")
                })?
                .id
                .as_str();
            if !associated {
                self.curves.associate(construction, slot);
            }
            self.curves.constructions.insert(construction.to_owned());
            self.curves.construction_count = model.procedural_curves.len();
        }
        Ok(result)
    }

    /// Admit one surface attachment.
    pub fn add_surface(
        &mut self,
        model: &mut Model,
        owner: &SurfaceId,
        procedural: ProceduralSurface,
    ) -> Result<Result<(), ProceduralCarrierError>, CodecError> {
        self.bind(model)?;
        self.surfaces.sync(
            model.surfaces.iter().enumerate().map(|(slot, carrier)| {
                (
                    slot,
                    carrier.id.as_str(),
                    carrier
                        .geometry
                        .procedural_construction()
                        .map(ProceduralSurfaceId::as_str),
                )
            }),
            model
                .procedural_surfaces
                .iter()
                .map(|procedural| procedural.id.as_str()),
        )?;
        let slot =
            match self
                .surfaces
                .owner("surface", owner.as_str(), procedural.id.as_str(), |slot| {
                    model.surfaces[slot].id.as_str()
                }) {
                Ok(slot) => slot,
                Err(error) => return Ok(Err(error)),
            };
        if model.surfaces[slot].id.as_str() != owner.as_str() {
            return Err(CodecError::malformed("procedural admission prefix changed"));
        }
        let associated = model.surfaces[slot]
            .geometry
            .procedural_construction()
            .is_some();
        let result = model.attach_procedural_surface(owner, slot, procedural);
        if result.is_ok() {
            let construction = model
                .procedural_surfaces
                .last()
                .ok_or_else(|| {
                    CodecError::malformed("procedural attachment appended no construction")
                })?
                .id
                .as_str();
            if !associated {
                self.surfaces.associate(construction, slot);
            }
            self.surfaces.constructions.insert(construction.to_owned());
            self.surfaces.construction_count = model.procedural_surfaces.len();
        }
        Ok(result)
    }
}

/// Append-only admission bound to one decode context.
pub struct ProceduralAdmission {
    index: ProceduralIndex,
}

impl ProceduralAdmission {
    /// Start an index for one model. Each family indexes on first use.
    pub fn new(model: &Model) -> Self {
        Self {
            index: ProceduralIndex {
                model: Some(model),
                ..ProceduralIndex::default()
            },
        }
    }
    /// Admit one attachment while preserving the append-only prefix.
    pub fn add_curve(
        &mut self,
        model: &mut Model,
        owner: &CurveId,
        procedural: ProceduralCurve,
    ) -> Result<Result<(), ProceduralCarrierError>, CodecError> {
        self.index.add_curve(model, owner, procedural)
    }
    /// Admit one attachment while preserving the append-only prefix.
    pub fn add_surface(
        &mut self,
        model: &mut Model,
        owner: &SurfaceId,
        procedural: ProceduralSurface,
    ) -> Result<Result<(), ProceduralCarrierError>, CodecError> {
        self.index.add_surface(model, owner, procedural)
    }
}

impl Model {
    /// Attach an owned batch using one index. Semantic rejection keeps
    /// the accepted prefix, as repeated single attachments do.
    pub fn add_procedural_curves(
        &mut self,
        rows: Vec<(CurveId, ProceduralCurve)>,
    ) -> Result<Result<(), ProceduralCarrierError>, CodecError> {
        let mut admission = ProceduralAdmission::new(self);
        for (owner, procedural) in rows {
            if let Err(error) = admission.add_curve(self, &owner, procedural)? {
                return Ok(Err(error));
            }
        }
        Ok(Ok(()))
    }
    /// Attach an owned batch using one index. Semantic rejection keeps
    /// the accepted prefix, as repeated single attachments do.
    pub fn add_procedural_surfaces(
        &mut self,
        rows: Vec<(SurfaceId, ProceduralSurface)>,
    ) -> Result<Result<(), ProceduralCarrierError>, CodecError> {
        let mut admission = ProceduralAdmission::new(self);
        for (owner, procedural) in rows {
            if let Err(error) = admission.add_surface(self, &owner, procedural)? {
                return Ok(Err(error));
            }
        }
        Ok(Ok(()))
    }
}

pub(super) fn standard_owners<'a, T>(
    carriers: impl Iterator<Item = (&'a str, &'a T)>,
) -> HashMap<&'a str, Option<&'a T>> {
    let mut owners = HashMap::new();
    for (construction, owner) in carriers {
        owners
            .entry(construction)
            .and_modify(|previous| *previous = None)
            .or_insert(Some(owner));
    }
    owners
}

// Serde reconstruction has no decode session. It uses standard allocation
// with the same owner selection and semantic rejection as decode admission.
pub(super) struct ReconstructedAdmission {
    owners: HashMap<String, usize>,
    associations: HashMap<String, Vec<String>>,
    constructions: HashSet<String>,
}

impl ReconstructedAdmission {
    pub(super) fn new<'a>(carriers: impl Iterator<Item = (&'a str, Option<&'a str>)>) -> Self {
        let mut owners = HashMap::new();
        let mut associations: HashMap<String, Vec<String>> = HashMap::new();
        for (slot, (owner, construction)) in carriers.enumerate() {
            owners.entry(owner.to_owned()).or_insert(slot);
            if let Some(construction) = construction {
                associations
                    .entry(construction.to_owned())
                    .or_default()
                    .push(owner.to_owned());
            }
        }
        Self {
            owners,
            associations,
            constructions: HashSet::new(),
        }
    }

    pub(super) fn owner(
        &self,
        noun: &str,
        owner: &str,
        construction: &str,
    ) -> Result<usize, ProceduralCarrierError> {
        if self.constructions.contains(construction) {
            return Err(ProceduralCarrierError::new(format!(
                "procedural {noun} construction {construction} already exists"
            )));
        }
        if let Some(associated) = self.associations.get(construction) {
            if let Some(other) = associated.iter().find(|other| other.as_str() != owner) {
                return Err(ProceduralCarrierError::new(format!(
                    "procedural {noun} construction {construction} already owns {noun} {other}"
                )));
            }
        }
        self.owners.get(owner).copied().ok_or_else(|| {
            ProceduralCarrierError::new(format!(
                "procedural {noun} {construction} references missing {noun} {owner}"
            ))
        })
    }

    pub(super) fn accept(&mut self, construction: &str) {
        self.constructions.insert(construction.to_owned());
    }
}

#[cfg(test)]
mod tests;
