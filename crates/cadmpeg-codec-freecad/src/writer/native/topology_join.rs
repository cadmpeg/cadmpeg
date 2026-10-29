// SPDX-License-Identifier: Apache-2.0
//! Current `native_ref` inverses checked against actual serialized shape uses.

use super::super::brep::EmittedTopologyUse;
use super::super::xml::Object;
use crate::native::element_map::ElementMapRecord;
use crate::topology_transfer::CurrentTopologyBindings;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::{NativeDisposition, NativeDispositions};
use cadmpeg_ir::CadIr;
use std::collections::BTreeMap;

pub(crate) struct TopologyJoin<'a> {
    current: CurrentTopologyBindings,
    ir: &'a CadIr,
    emitted: Vec<(&'a str, Vec<(&'a str, &'a EmittedTopologyUse)>)>,
    valid: Vec<&'a str>,
}

impl<'a> TopologyJoin<'a> {
    pub(in crate::writer) fn from_current(
        ctx: &DecodeContext<'_>,
        ir: &'a CadIr,
        current: CurrentTopologyBindings,
        emitted: &'a [(String, Vec<EmittedTopologyUse>)],
    ) -> Result<Self, CodecError> {
        if let Some(refusal) = ctx.resource_refusal() {
            return Err(refusal.into());
        }
        let mut owners = Vec::new();
        ctx.reserve_working(
            &mut owners,
            emitted.len(),
            "FreeCAD emitted topology owners",
        )?;
        for (owner, uses) in emitted {
            let mut records = Vec::new();
            ctx.reserve_working(&mut records, uses.len(), "FreeCAD emitted topology records")?;
            records.extend(uses.iter().map(|usage| (usage.neutral_id.as_str(), usage)));
            records.sort_unstable_by(|a, b| a.0.cmp(b.0));
            owners.push((owner.as_str(), records));
        }
        owners.sort_unstable_by(|a, b| a.0.cmp(b.0));
        Ok(Self {
            current,
            ir,
            emitted: owners,
            valid: current_roles(ctx, ir)?,
        })
    }

    fn owner(&self, owner: &str) -> Option<&[(&'a str, &'a EmittedTopologyUse)]> {
        let index = self
            .emitted
            .binary_search_by(|entry| entry.0.cmp(owner))
            .ok()?;
        if index
            .checked_sub(1)
            .and_then(|index| self.emitted.get(index))
            .is_some_and(|entry| entry.0 == owner)
            || self
                .emitted
                .get(index + 1)
                .is_some_and(|entry| entry.0 == owner)
        {
            return None;
        }
        Some(&self.emitted[index].1)
    }

    pub(in crate::writer) fn shape_matches(
        &self,
        ctx: &DecodeContext<'_>,
        object: &Object,
        property: &str,
        entries: &BTreeMap<String, Vec<u8>>,
    ) -> Result<bool, CodecError> {
        if let Some(refusal) = ctx.resource_refusal() {
            return Err(refusal.into());
        }
        if property != "Shape" || self.owner(&object.name).is_none() {
            return Ok(false);
        }
        let mut file = String::new();
        ctx.reserve_text_working(&mut file, object.name.len(), "FreeCAD emitted shape owner")?;
        file.push_str(&object.name);
        ctx.reserve_text_working(&mut file, ".brp".len(), "FreeCAD emitted shape member")?;
        file.push_str(".brp");
        if !entries.contains_key(&file) {
            return Ok(false);
        }
        let Some(text) = object.properties.get(property) else {
            return Ok(false);
        };
        // A BOM scalar may overwrite Shape after BRep creation. Reject its
        // different root type before parsing its potentially large value.
        let Some((opening, _)) = text.split_once('>') else {
            return Ok(false);
        };
        if !opening.contains("type=\"Part::PropertyPartShape\"") {
            return Ok(false);
        }
        let Ok(document) = roxmltree::Document::parse(text) else {
            return Ok(false);
        };
        let root = document.root_element();
        if !root.has_tag_name("Property")
            || root.attribute("name") != Some(property)
            || root.attribute("type") != Some("Part::PropertyPartShape")
        {
            return Ok(false);
        }
        let mut parts = root.children().filter(|node| node.has_tag_name("Part"));
        Ok(parts
            .next()
            .is_some_and(|part| part.attribute("file") == Some(file.as_str()))
            && parts.next().is_none())
    }

    pub(in crate::writer) fn map_resolves(&self, map: &ElementMapRecord, owner: &str) -> bool {
        for group in &map.maps.root().groups {
            for (index, names) in group.names.iter().enumerate() {
                if names.is_empty() {
                    continue;
                }
                let (needs_inverse, records) =
                    self.current
                        .selected_records(&map.property, &group.indexed_name, index);
                // Native tombstone names are identified by the actual typed
                // source graph, never by their spelling or absent helper rows.
                if !needs_inverse {
                    continue;
                }
                let mut records = records.peekable();
                if records.peek().is_none() {
                    return false;
                }
                for record in records {
                    if self.valid.binary_search(&record).is_err() {
                        return false;
                    }
                    let Some(uses) = self.owner(owner) else {
                        return false;
                    };
                    let Ok(index) = uses.binary_search_by(|entry| entry.0.cmp(record)) else {
                        return false;
                    };
                    let usage = uses[index].1;
                    // A flattened Wi represents its one current Shell only if
                    // every actual current member was emitted to this owner.
                    // Sharing an id/native_ref or matching a shell count alone
                    // cannot certify a missing edge/face/vertex use.
                    if let Some(shell) = self
                        .ir
                        .model
                        .shells
                        .iter()
                        .find(|shell| shell.id.as_str() == record)
                    {
                        if shell
                            .faces()
                            .iter()
                            .map(cadmpeg_ir::ids::FaceId::as_str)
                            .chain(
                                shell
                                    .wire_edges()
                                    .iter()
                                    .map(cadmpeg_ir::ids::EdgeId::as_str),
                            )
                            .chain(
                                shell
                                    .free_vertices()
                                    .iter()
                                    .map(cadmpeg_ir::ids::VertexId::as_str),
                            )
                            .any(|member| {
                                uses.binary_search_by(|entry| entry.0.cmp(member)).is_err()
                            })
                        {
                            return false;
                        }
                    }
                    if usage.native_shape == 0
                        || crate::topology_transfer::indexed_name(usage.kind) != group.indexed_name
                    {
                        return false;
                    }
                }
            }
        }
        true
    }

    pub(in crate::writer) fn record_emitted(
        &self,
        ctx: &DecodeContext<'_>,
        objects: &[Object],
        entries: &BTreeMap<String, Vec<u8>>,
        outcomes: &mut NativeDispositions,
    ) -> Result<(), CodecError> {
        let mut actual_objects = Vec::new();
        ctx.reserve_working(
            &mut actual_objects,
            objects.len(),
            "FreeCAD emitted shape references",
        )?;
        actual_objects.extend(objects.iter().map(|object| (object.name.as_str(), object)));
        actual_objects.sort_unstable_by(|a, b| a.0.cmp(b.0));
        for (owner, uses) in &self.emitted {
            let Ok(index) = actual_objects.binary_search_by(|entry| entry.0.cmp(owner)) else {
                continue;
            };
            if !self.shape_matches(ctx, actual_objects[index].1, "Shape", entries)? {
                continue;
            }
            for (record, usage) in uses {
                if self.valid.binary_search(record).is_err() || usage.native_shape == 0 {
                    continue;
                }
                let Some(native) = self.current.native_for_record(record) else {
                    continue;
                };
                if self.current.source_kind(native) == Some(usage.kind) {
                    super::super::dispositions::record(
                        outcomes,
                        "topology_uses",
                        native,
                        NativeDisposition::Regenerated,
                    );
                }
            }
        }
        Ok(())
    }
}

/// Current typed roles only: no comparison to a historical native projection.
fn current_roles<'a>(ctx: &DecodeContext<'_>, ir: &'a CadIr) -> Result<Vec<&'a str>, CodecError> {
    let mut valid = Vec::new();
    let mut ownership = Vec::new();
    ctx.reserve_working(
        &mut valid,
        ir.model.bodies.len(),
        "FreeCAD current body roles",
    )?;
    for body in &ir.model.bodies {
        valid.push(body.id.as_str());
        ctx.reserve_working(
            &mut ownership,
            body.regions.len(),
            "FreeCAD current region roles",
        )?;
        ownership.extend(
            body.regions
                .iter()
                .map(|id| (body.id.as_str(), id.as_str())),
        );
    }
    valid.sort_unstable();
    ownership.sort_unstable();
    ctx.reserve_working(
        &mut valid,
        ir.model.regions.len(),
        "FreeCAD current region roles",
    )?;
    let parent_count = valid.len();
    let link_count = ownership.len();
    for region in &ir.model.regions {
        if valid[..parent_count]
            .binary_search(&region.body.as_str())
            .is_ok()
            && ownership[..link_count]
                .binary_search(&(region.body.as_str(), region.id.as_str()))
                .is_ok()
        {
            valid.push(region.id.as_str());
        }
        ctx.reserve_working(
            &mut ownership,
            region.shells.len(),
            "FreeCAD current shell roles",
        )?;
        ownership.extend(
            region
                .shells
                .iter()
                .map(|id| (region.id.as_str(), id.as_str())),
        );
    }
    valid.sort_unstable();
    ownership.sort_unstable();
    ctx.reserve_working(
        &mut valid,
        ir.model.shells.len(),
        "FreeCAD current shell roles",
    )?;
    let parent_count = valid.len();
    let link_count = ownership.len();
    for shell in &ir.model.shells {
        if valid[..parent_count]
            .binary_search(&shell.region.as_str())
            .is_ok()
            && ownership[..link_count]
                .binary_search(&(shell.region.as_str(), shell.id.as_str()))
                .is_ok()
        {
            valid.push(shell.id.as_str());
        }
        ctx.reserve_working(
            &mut ownership,
            shell.faces().len(),
            "FreeCAD current face roles",
        )?;
        ownership.extend(
            shell
                .faces()
                .iter()
                .map(|id| (shell.id.as_str(), id.as_str())),
        );
    }
    valid.sort_unstable();
    ownership.sort_unstable();
    ctx.reserve_working(
        &mut valid,
        ir.model.faces.len(),
        "FreeCAD current face roles",
    )?;
    let parent_count = valid.len();
    let link_count = ownership.len();
    for face in &ir.model.faces {
        if valid[..parent_count]
            .binary_search(&face.shell.as_str())
            .is_ok()
            && ownership[..link_count]
                .binary_search(&(face.shell.as_str(), face.id.as_str()))
                .is_ok()
        {
            valid.push(face.id.as_str());
        }
        ctx.reserve_working(
            &mut ownership,
            face.loops.iter().count(),
            "FreeCAD current loop roles",
        )?;
        ownership.extend(face.loops.iter().map(|id| (face.id.as_str(), id.as_str())));
    }
    valid.sort_unstable();
    ownership.sort_unstable();
    let mut coedges = Vec::new();
    ctx.reserve_working(
        &mut coedges,
        ir.model.coedges.len(),
        "FreeCAD current coedge roles",
    )?;
    coedges.extend(
        ir.model
            .coedges
            .iter()
            .map(|coedge| (coedge.id.as_str(), coedge)),
    );
    coedges.sort_unstable_by(|a, b| a.0.cmp(b.0));
    ctx.reserve_working(
        &mut valid,
        ir.model.loops.len(),
        "FreeCAD current loop roles",
    )?;
    let parent_count = valid.len();
    let link_count = ownership.len();
    for wire in &ir.model.loops {
        if valid[..parent_count]
            .binary_search(&wire.face.as_str())
            .is_ok()
            && ownership[..link_count]
                .binary_search(&(wire.face.as_str(), wire.id.as_str()))
                .is_ok()
            && wire.coedges().iter().all(|id| {
                coedges
                    .binary_search_by(|entry| entry.0.cmp(id.as_str()))
                    .ok()
                    .is_some_and(|index| coedges[index].1.owner_loop == wire.id)
            })
        {
            valid.push(wire.id.as_str());
        }
    }
    ctx.reserve_working(
        &mut valid,
        ir.model.edges.len(),
        "FreeCAD current edge roles",
    )?;
    valid.extend(ir.model.edges.iter().map(|edge| edge.id.as_str()));
    ctx.reserve_working(
        &mut valid,
        ir.model.vertices.len(),
        "FreeCAD current vertex roles",
    )?;
    valid.extend(ir.model.vertices.iter().map(|vertex| vertex.id.as_str()));
    valid.sort_unstable();
    valid.dedup();
    Ok(valid)
}

#[cfg(test)]
mod tests;
