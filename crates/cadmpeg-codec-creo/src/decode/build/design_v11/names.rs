// SPDX-License-Identifier: Apache-2.0
//! Names from the geometry ids the file stores.
//!
//! Creo gives each surface and each edge of the model a geometry id and
//! records the feature that generated it (`srf_array.feat_id`, the curve
//! topology row's `feat_id`). A face carries its surface's id. A name states
//! that feature and no role, since the file names no role; the id is an
//! allocation and stays in the native name (§5.7).

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::design::names::{
    NameEntry, NameKind, NameTable, NativeName, NativeNameValue, TopoName,
};
use cadmpeg_ir::design::refs::EntityRef;
use cadmpeg_ir::design::selection::{PickTarget, Picked};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::FeatureId;

use crate::container::ContainerScan;

pub(super) const FACE: &str = "creo:visibgeom:face#";
pub(super) const SURFACE: &str = "creo:visibgeom:surface#";
pub(super) const EDGE: &str = "creo:visibgeom:edge#";
const FEATURE: &str = "creo:model:feature#";

pub(super) fn native(id: u32) -> NativeName {
    NativeName {
        codec: cadmpeg_core::nonblank_literal!("creo").into(),
        value: NativeNameValue::Integers(vec![i64::from(id)]),
    }
}

/// The producing feature of each geometry id that exactly one source row
/// owns, and the timeline index of each feature the design holds.
pub(super) struct Names {
    surfaces: BTreeMap<u32, u32>,
    edges: BTreeMap<u32, u32>,
    order: BTreeMap<FeatureId, usize>,
}

fn unique(rows: impl Iterator<Item = (u32, u32)>) -> BTreeMap<u32, u32> {
    let mut owners = BTreeMap::new();
    let mut repeated = BTreeSet::new();
    for (id, feature) in rows {
        if owners.insert(id, feature).is_some() {
            repeated.insert(id);
        }
    }
    owners.retain(|id, _| !repeated.contains(id));
    owners
}

impl Names {
    /// `order` lists the design's features in timeline order.
    pub(super) fn new<'a>(
        scan: &ContainerScan,
        order: impl IntoIterator<Item = &'a FeatureId>,
    ) -> Self {
        Self {
            surfaces: unique(
                scan.surfaces
                    .rows
                    .iter()
                    .map(|row| (row.id, row.feature_id)),
            ),
            edges: unique(
                scan.curves
                    .topology_rows
                    .iter()
                    .map(|row| (row.id, row.feature_id)),
            ),
            order: order
                .into_iter()
                .enumerate()
                .map(|(index, feature)| (feature.clone(), index))
                .collect(),
        }
    }

    fn producer(&self, kind: NameKind, id: u32) -> Option<FeatureId> {
        let owner = match kind {
            NameKind::Face | NameKind::Surface => self.surfaces.get(&id),
            NameKind::Edge => self.edges.get(&id),
            _ => None,
        }?;
        FeatureId::mint(format!("{FEATURE}{owner}"))
            .ok()
            .filter(|feature| self.order.contains_key(feature))
    }

    fn name(kind: NameKind, feature: FeatureId) -> TopoName {
        TopoName::Generated {
            kind,
            feature,
            role: None,
            generator: None,
            index: None,
        }
    }

    fn entry(&self, kind: NameKind, id: u32) -> NameEntry {
        NameEntry {
            name: self
                .producer(kind, id)
                .map(|feature| Self::name(kind, feature)),
            native: Some(native(id)),
        }
    }

    /// A pick, by the selecting feature's input state, of the entity with
    /// geometry id `id`: by its name where its producer comes before the
    /// selecting feature, and by its native name otherwise.
    pub(super) fn pick(&self, kind: NameKind, id: u32, holder: &FeatureId) -> Picked {
        let before = |feature: &FeatureId| {
            self.order
                .get(feature)
                .zip(self.order.get(holder))
                .is_some_and(|(producer, holder)| producer < holder)
        };
        let target = match self.producer(kind, id).filter(before) {
            Some(feature) => PickTarget::Entity(Self::name(kind, feature)),
            None => PickTarget::Native { within: None },
        };
        Picked {
            target,
            context: None,
            native: Some(native(id)),
            domain: None,
            witness: None,
            resolved: None,
        }
    }

    /// The name table of the current geometry: every face, surface, and edge
    /// whose record id carries a geometry id.
    pub(super) fn table(
        &self,
        ctx: &DecodeContext<'_>,
        ir: &CadIr,
    ) -> Result<NameTable, CodecError> {
        let mut entries = BTreeMap::new();
        let numbered = |id: &str, prefix: &str| crate::identity::numbered_identity(id, prefix);
        for face in &ir.geometry.faces {
            if let Some(id) = numbered(face.id.as_str(), FACE) {
                ctx.charge_entities(1, "project Creo v11 name")?;
                entries.insert(
                    EntityRef::Face(face.id.clone()),
                    self.entry(NameKind::Face, id),
                );
            }
        }
        for surface in &ir.geometry.surfaces {
            if let Some(id) = numbered(surface.id.as_str(), SURFACE) {
                ctx.charge_entities(1, "project Creo v11 name")?;
                entries.insert(
                    EntityRef::Surface(surface.id.clone()),
                    self.entry(NameKind::Surface, id),
                );
            }
        }
        for edge in &ir.geometry.edges {
            if let Some(id) = numbered(edge.id.as_str(), EDGE) {
                ctx.charge_entities(1, "project Creo v11 name")?;
                entries.insert(
                    EntityRef::Edge(edge.id.clone()),
                    self.entry(NameKind::Edge, id),
                );
            }
        }
        Ok(NameTable::from(entries))
    }
}

#[cfg(test)]
mod tests;
