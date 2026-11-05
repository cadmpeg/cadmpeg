// SPDX-License-Identifier: Apache-2.0
//! Resolve polygon carriers through current typed topology uses.

use crate::brep::{
    Polygon3dRecord, PolygonOnTriangulationRecord, ShapePayloadRecord, Tables, TextTShape,
};
use crate::native::source_use::{SourceUse, SourceUseSelector};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::CadIr;

pub(super) struct PolygonSources {
    uses: Vec<SourceUseSelector>,
    payloads: Vec<ShapePayloadRecord>,
    polygons3d: Vec<Polygon3dRecord>,
    indexed: Vec<PolygonOnTriangulationRecord>,
}

impl PolygonSources {
    pub(super) fn new(ctx: &DecodeContext<'_>, ir: &CadIr) -> Result<Option<Self>, CodecError> {
        let Some(namespace) = ir.native.namespace("fcstd") else {
            return Ok(None);
        };
        let mut uses = namespace.arena_as_for_decode::<SourceUseSelector>(ctx, "topology_uses")?;
        uses.sort_unstable_by(|left, right| left.id.cmp(&right.id));
        let mut payloads =
            namespace.arena_as_for_decode::<ShapePayloadRecord>(ctx, "shape_payloads")?;
        payloads.sort_unstable_by(|left, right| left.id.cmp(&right.id));
        Ok(Some(Self {
            uses,
            payloads,
            polygons3d: namespace.arena_as_for_decode(ctx, "polygons3d")?,
            indexed: namespace.arena_as_for_decode(ctx, "polygons_on_triangulations")?,
        }))
    }

    pub(super) fn tables<'a>(&'a self, payload: &'a ShapePayloadRecord) -> Option<Tables<'a>> {
        Tables::from_payload(payload, &self.polygons3d, &self.indexed).ok()?
    }

    pub(super) fn shape(
        &self,
        ctx: &DecodeContext<'_>,
        native: &str,
    ) -> Result<Option<(&ShapePayloadRecord, &TextTShape)>, CodecError> {
        let find_use = |id: &str| {
            self.uses
                .binary_search_by(|record| record.id.as_str().cmp(id))
                .ok()
                .map(|index| &self.uses[index])
        };
        let Some(mut current) = find_use(native) else {
            return Ok(None);
        };
        let owner = current.owner.as_str();
        let mut slots = Vec::new();
        let mut steps = 0_usize;
        let root = loop {
            let Some(next) = steps.checked_add(1) else {
                return Ok(None);
            };
            steps = next;
            if steps > self.uses.len() || current.owner != owner {
                return Ok(None);
            }
            match &current.source_use {
                SourceUse::Root { root } => break *root,
                SourceUse::Child { parent, slot } => {
                    ctx.reserve_working(&mut slots, 1, "FreeCAD polygon source path")?;
                    slots.push(*slot);
                    let Some(parent) = find_use(parent) else {
                        return Ok(None);
                    };
                    current = parent;
                }
            }
        };
        let Some(payload) = self
            .payloads
            .binary_search_by(|record| record.id.as_str().cmp(owner))
            .ok()
            .map(|index| &self.payloads[index])
        else {
            return Ok(None);
        };
        let Some(tables) = self.tables(payload) else {
            return Ok(None);
        };
        let Some(mut index) = root
            .checked_sub(1)
            .and_then(|index| tables.roots.get(index))
            .map(|root| root.shape)
        else {
            return Ok(None);
        };
        for slot in slots.into_iter().rev() {
            let Some(child) = tables.tshapes.resolve(index).ok().and_then(|shape| {
                slot.checked_sub(1)
                    .and_then(|slot| shape.children.get(slot))
            }) else {
                return Ok(None);
            };
            index = child.shape;
        }
        Ok(tables
            .tshapes
            .resolve(index)
            .ok()
            .map(|shape| (payload, shape)))
    }
}
