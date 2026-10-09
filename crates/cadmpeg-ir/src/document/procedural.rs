// SPDX-License-Identifier: Apache-2.0
//! Indexed procedural admission at explicit append-only model boundaries.

use std::collections::{hash_map::RandomState, HashMap, HashSet};
use std::hash::BuildHasher;

use cadmpeg_core::decode::{u64_from_index, DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;

use super::{Model, ProceduralCarrierError};
use crate::geometry::{ProceduralCurve, ProceduralSurface};
use crate::ids::{CurveId, ProceduralCurveId, ProceduralSurfaceId, SurfaceId};

// Fingerprints avoid rehashing stored text during table growth. Equal
// fingerprints retain distinct text entries; every text equality is fallible.
struct TextIndex<V> {
    hashes: RandomState,
    buckets: HashMap<u64, Vec<(String, V)>>,
}

impl<V> Default for TextIndex<V> {
    fn default() -> Self {
        Self {
            hashes: RandomState::new(),
            buckets: HashMap::new(),
        }
    }
}

impl<V> TextIndex<V> {
    fn fingerprint(&self, ctx: &DecodeContext<'_>, text: &str) -> Result<u64, CodecError> {
        ctx.charge_work(1, "hash procedural identity")?;
        ctx.charge_work(u64_from_index(text.len()), "hash procedural identity")?;
        // The integer-table probe hashes a u64 and compares fixed-width keys.
        ctx.charge_work(24, "probe procedural identity index")?;
        Ok(self.hashes.hash_one(text))
    }

    fn get<'a>(&'a self, ctx: &DecodeContext<'_>, text: &str) -> Result<Option<&'a V>, CodecError> {
        let hash = self.fingerprint(ctx, text)?;
        if let Some(bucket) = self.buckets.get(&hash) {
            for (stored, value) in bucket {
                if crate::ids::comparison::equal(
                    ctx,
                    stored,
                    text,
                    "compare indexed procedural identities",
                )? {
                    return Ok(Some(value));
                }
            }
        }
        Ok(None)
    }

    fn get_mut<'a>(
        &'a mut self,
        ctx: &DecodeContext<'_>,
        text: &str,
    ) -> Result<Option<&'a mut V>, CodecError> {
        let hash = self.fingerprint(ctx, text)?;
        if let Some(bucket) = self.buckets.get_mut(&hash) {
            for (stored, value) in bucket {
                if crate::ids::comparison::equal(
                    ctx,
                    stored,
                    text,
                    "compare indexed procedural identities",
                )? {
                    return Ok(Some(value));
                }
            }
        }
        Ok(None)
    }

    fn insert_first(
        &mut self,
        ctx: &DecodeContext<'_>,
        text: &str,
        value: V,
    ) -> Result<(), CodecError> {
        if self.get(ctx, text)?.is_some() {
            return Ok(());
        }
        let hash = self.fingerprint(ctx, text)?;
        let text = ctx.copy_retained_text(text, "store procedural index identity")?;
        if self.buckets.len() == self.buckets.capacity() {
            let work = self
                .buckets
                .len()
                .checked_mul(std::mem::size_of::<(u64, Vec<(String, V)>)>() + 32)
                .ok_or_else(|| {
                    ctx.refuse_codec_limit("grow procedural identity index", u64::MAX - 1, u64::MAX)
                })?;
            ctx.charge_work(u64_from_index(work), "grow procedural identity index")?;
        }
        ctx.push_hash_group(
            &mut self.buckets,
            hash,
            (text, value),
            "procedural identity index buckets",
            "procedural identity index entries",
        )
    }
}

#[derive(Default)]
struct FamilyIndex {
    owners: TextIndex<usize>,
    constructions: TextIndex<()>,
    associations: TextIndex<Vec<usize>>,
    carrier_count: usize,
    construction_count: usize,
}

impl FamilyIndex {
    fn sync<'text>(
        &mut self,
        ctx: &DecodeContext<'_>,
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
            ctx.charge_work(1, "index appended procedural carriers")?;
            self.owners.insert_first(ctx, owner, slot)?;
            if let Some(construction) = construction {
                self.associate(ctx, construction, slot)?;
            }
        }
        for construction in constructions.skip(self.construction_count) {
            ctx.charge_work(1, "index appended procedural constructions")?;
            self.constructions.insert_first(ctx, construction, ())?;
        }
        self.carrier_count = carrier_count;
        self.construction_count = construction_count;
        Ok(())
    }

    fn associate(
        &mut self,
        ctx: &DecodeContext<'_>,
        construction: &str,
        slot: usize,
    ) -> Result<(), CodecError> {
        if let Some(group) = self.associations.get_mut(ctx, construction)? {
            ctx.push_vec(group, slot, "procedural carrier association slots")?;
        } else {
            let mut group = Vec::new();
            ctx.push_vec(&mut group, slot, "procedural carrier association slots")?;
            self.associations.insert_first(ctx, construction, group)?;
        }
        Ok(())
    }

    fn owner<'text>(
        &self,
        ctx: &DecodeContext<'_>,
        noun: &str,
        owner: &str,
        construction: &str,
        owner_at: impl Fn(usize) -> &'text str,
    ) -> Result<Result<usize, ProceduralCarrierError>, CodecError> {
        if self.constructions.get(ctx, construction)?.is_some() {
            return Ok(Err(ProceduralCarrierError::new(ctx.format_retained(
                format_args!("procedural {noun} construction {construction} already exists"),
                "procedural admission refusal",
            )?)));
        }
        if let Some(group) = self.associations.get(ctx, construction)? {
            for &slot in group {
                let other = owner_at(slot);
                if !crate::ids::comparison::equal(
                    ctx,
                    owner,
                    other,
                    "compare indexed procedural owners",
                )? {
                    return Ok(Err(ProceduralCarrierError::new(ctx.format_retained(
                        format_args!("procedural {noun} construction {construction} already owns {noun} {other}"), "procedural admission refusal")?)));
                }
            }
        }
        match self.owners.get(ctx, owner)?.copied() {
            Some(slot) => Ok(Ok(slot)),
            None => Ok(Err(ProceduralCarrierError::new(ctx.format_retained(
                format_args!("procedural {noun} {construction} references missing {noun} {owner}"),
                "procedural admission refusal",
            )?))),
        }
    }
}

fn with_index_storage<T>(
    storage: &mut Option<&mut ScopedReservation<'_>>,
    work: impl FnOnce() -> Result<T, CodecError>,
) -> Result<T, CodecError> {
    match storage {
        Some(storage) => storage.with_storage(work),
        None => work(),
    }
}

/// Owned indexes for a scope that only appends carriers and constructions.
/// Existing carrier identities, construction references, and arena order must
/// remain unchanged. Each accepted attachment updates its indexed ownership.
/// Discard this index before moving the model or replacing, removing, or reordering arena entries.
/// Index allocations use the supplied context's current storage admission.
#[derive(Default)]
pub struct ProceduralIndex {
    model: Option<*const Model>,
    curves: FamilyIndex,
    surfaces: FamilyIndex,
}

impl ProceduralIndex {
    fn bind(&mut self, ctx: &DecodeContext<'_>, model: &Model) -> Result<(), CodecError> {
        ctx.charge_work(0, "indexed procedural admission")?;
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
    /// Admit one attachment with retained index backing.
    pub fn add_curve(
        &mut self,
        ctx: &DecodeContext<'_>,
        model: &mut Model,
        owner: &CurveId,
        procedural: ProceduralCurve,
    ) -> Result<Result<(), ProceduralCarrierError>, CodecError> {
        self.add_curve_with_storage(ctx, model, owner, procedural, None)
    }

    fn add_curve_with_storage(
        &mut self,
        ctx: &DecodeContext<'_>,
        model: &mut Model,
        owner: &CurveId,
        procedural: ProceduralCurve,
        mut storage: Option<&mut ScopedReservation<'_>>,
    ) -> Result<Result<(), ProceduralCarrierError>, CodecError> {
        self.bind(ctx, model)?;
        with_index_storage(&mut storage, || {
            self.curves.sync(
                ctx,
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
            )
        })?;
        let slot = match self.curves.owner(
            ctx,
            "curve",
            owner.as_str(),
            procedural.id.as_str(),
            |slot| model.curves[slot].id.as_str(),
        )? {
            Ok(slot) => slot,
            Err(error) => return Ok(Err(error)),
        };
        if !crate::ids::comparison::equal(
            ctx,
            model.curves[slot].id.as_str(),
            owner.as_str(),
            "check indexed procedural owner slot",
        )? {
            return Err(CodecError::malformed("procedural admission prefix changed"));
        }
        let associated = model.curves[slot]
            .geometry
            .procedural_construction()
            .is_some();
        let result = model.attach_procedural_curve(ctx, owner, slot, procedural)?;
        if result.is_ok() {
            let construction = model
                .procedural_curves
                .last()
                .expect("successful attachment appends its construction")
                .id
                .as_str();
            with_index_storage(&mut storage, || {
                if !associated {
                    self.curves.associate(ctx, construction, slot)?;
                }
                self.curves
                    .constructions
                    .insert_first(ctx, construction, ())
            })?;
            self.curves.construction_count = model.procedural_curves.len();
        }
        Ok(result)
    }
    /// Admit one attachment with retained index backing.
    pub fn add_surface(
        &mut self,
        ctx: &DecodeContext<'_>,
        model: &mut Model,
        owner: &SurfaceId,
        procedural: ProceduralSurface,
    ) -> Result<Result<(), ProceduralCarrierError>, CodecError> {
        self.add_surface_with_storage(ctx, model, owner, procedural, None)
    }

    fn add_surface_with_storage(
        &mut self,
        ctx: &DecodeContext<'_>,
        model: &mut Model,
        owner: &SurfaceId,
        procedural: ProceduralSurface,
        mut storage: Option<&mut ScopedReservation<'_>>,
    ) -> Result<Result<(), ProceduralCarrierError>, CodecError> {
        self.bind(ctx, model)?;
        with_index_storage(&mut storage, || {
            self.surfaces.sync(
                ctx,
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
            )
        })?;
        let slot = match self.surfaces.owner(
            ctx,
            "surface",
            owner.as_str(),
            procedural.id.as_str(),
            |slot| model.surfaces[slot].id.as_str(),
        )? {
            Ok(slot) => slot,
            Err(error) => return Ok(Err(error)),
        };
        if !crate::ids::comparison::equal(
            ctx,
            model.surfaces[slot].id.as_str(),
            owner.as_str(),
            "check indexed procedural owner slot",
        )? {
            return Err(CodecError::malformed("procedural admission prefix changed"));
        }
        let associated = model.surfaces[slot]
            .geometry
            .procedural_construction()
            .is_some();
        let result = model.attach_procedural_surface(ctx, owner, slot, procedural)?;
        if result.is_ok() {
            let construction = model
                .procedural_surfaces
                .last()
                .expect("successful attachment appends its construction")
                .id
                .as_str();
            with_index_storage(&mut storage, || {
                if !associated {
                    self.surfaces.associate(ctx, construction, slot)?;
                }
                self.surfaces
                    .constructions
                    .insert_first(ctx, construction, ())
            })?;
            self.surfaces.construction_count = model.procedural_surfaces.len();
        }
        Ok(result)
    }
}

/// Append-only admission whose index storage is scoped to the caller's session.
pub struct ProceduralAdmission<'ctx> {
    ctx: &'ctx DecodeContext<'ctx>,
    index: ProceduralIndex,
    storage: ScopedReservation<'ctx>,
}

impl<'ctx> ProceduralAdmission<'ctx> {
    /// Start a scoped index for one model. Each family indexes on first use.
    pub fn new(ctx: &'ctx DecodeContext<'_>, model: &Model) -> Result<Self, CodecError> {
        Ok(Self {
            ctx,
            index: ProceduralIndex {
                model: Some(model),
                ..ProceduralIndex::default()
            },
            storage: ctx.reserve_scoped(0, "indexed procedural admission")?,
        })
    }
    /// Admit one attachment while preserving the append-only prefix.
    pub fn add_curve(
        &mut self,
        model: &mut Model,
        owner: &CurveId,
        procedural: ProceduralCurve,
    ) -> Result<Result<(), ProceduralCarrierError>, CodecError> {
        self.index.add_curve_with_storage(
            self.ctx,
            model,
            owner,
            procedural,
            Some(&mut self.storage),
        )
    }
    /// Admit one attachment while preserving the append-only prefix.
    pub fn add_surface(
        &mut self,
        model: &mut Model,
        owner: &SurfaceId,
        procedural: ProceduralSurface,
    ) -> Result<Result<(), ProceduralCarrierError>, CodecError> {
        self.index.add_surface_with_storage(
            self.ctx,
            model,
            owner,
            procedural,
            Some(&mut self.storage),
        )
    }
}

impl Model {
    /// Attach an owned batch using one index. Semantic rejection keeps
    /// the accepted prefix, as repeated single attachments do.
    pub fn add_procedural_curves(
        &mut self,
        ctx: &DecodeContext<'_>,
        rows: Vec<(CurveId, ProceduralCurve)>,
    ) -> Result<Result<(), ProceduralCarrierError>, CodecError> {
        let mut admission = ProceduralAdmission::new(ctx, self)?;
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
        ctx: &DecodeContext<'_>,
        rows: Vec<(SurfaceId, ProceduralSurface)>,
    ) -> Result<Result<(), ProceduralCarrierError>, CodecError> {
        let mut admission = ProceduralAdmission::new(ctx, self)?;
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
