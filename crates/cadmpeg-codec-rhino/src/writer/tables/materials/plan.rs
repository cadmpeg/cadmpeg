// SPDX-License-Identifier: Apache-2.0
//! Current material records have one shared authoring identity allocation.

use super::{
    bound_appearance, encode_material, native_complete, own_material, records, wire_index, CadIr,
    CodecError, Value,
};
use crate::loss::RhinoLossCode;
use cadmpeg_ir::report::loss::LossNote;
use std::collections::{BTreeMap, BTreeSet};

struct Entry {
    identity: String,
    appearance: Option<usize>,
    native: Option<String>,
    uuid: Option<[u8; 16]>,
}

pub(in crate::writer) struct MaterialPlan {
    entries: Vec<Entry>,
    appearances: BTreeMap<String, i32>,
    native_targets: BTreeMap<String, Vec<i32>>,
    losses: Vec<LossNote>,
}

fn parsed(value: Option<&Value>) -> Option<[u8; 16]> {
    value
        .and_then(Value::as_str)
        .and_then(crate::presentation::parse_uuid_text)
        .filter(|uuid| !uuid.is_nil())
        .map(crate::wire::Uuid::to_wire)
}

fn increment(uuid: &mut [u8; 16]) {
    for byte in uuid.iter_mut().rev() {
        let (value, carry) = byte.overflowing_add(1);
        *byte = value;
        if !carry {
            break;
        }
    }
}

impl MaterialPlan {
    pub(in crate::writer) fn new(ir: &CadIr) -> Result<Self, CodecError> {
        let mut plan = Self {
            entries: Vec::new(),
            appearances: BTreeMap::new(),
            native_targets: BTreeMap::new(),
            losses: Vec::new(),
        };
        let mut reserved = BTreeSet::from([[0; 16]]);
        let unlinked = records(ir, "materials")
            .iter()
            .filter(|record| {
                native_complete(record)
                    && !ir
                        .model
                        .appearances
                        .iter()
                        .any(|appearance| appearance.native_ref.as_deref() == Some(record.id()))
            })
            .collect::<Vec<_>>();
        for record in &unlinked {
            if let Some(uuid) = parsed(record.field("source_uuid").as_ref()) {
                reserved.insert(uuid);
            }
        }
        let mut candidates = Vec::new();
        let mut counts = BTreeMap::new();
        for appearance in &ir.model.appearances {
            let native = own_material(ir, appearance);
            let uuid = appearance
                .asset_guid
                .as_deref()
                .and_then(crate::presentation::parse_uuid_text)
                .filter(|uuid| !uuid.is_nil())
                .map(crate::wire::Uuid::to_wire)
                .or_else(|| native.and_then(|record| parsed(record.field("source_uuid").as_ref())))
                .unwrap_or_else(|| crate::writer::neutral_object_uuid(appearance.id.as_str()));
            *counts.entry(uuid).or_insert(0_usize) += 1;
            candidates.push(uuid);
        }
        let mut allocated = BTreeMap::new();
        let mut owners = BTreeMap::new();
        let mut native_uuids = BTreeMap::new();
        for appearance in &ir.model.appearances {
            if let Some(record) = own_material(ir, appearance) {
                *owners.entry(record.id()).or_insert(0_usize) += 1;
                if let Some(uuid) = parsed(record.field("source_uuid").as_ref()) {
                    *native_uuids.entry(uuid).or_insert(0_usize) += 1;
                }
            }
        }
        for (appearance, uuid) in ir.model.appearances.iter().zip(&candidates) {
            if let Some(record) = own_material(ir, appearance) {
                if owners[record.id()] == 1
                    && native_uuids.get(uuid) == Some(&1)
                    && parsed(record.field("source_uuid").as_ref()) == Some(*uuid)
                    && reserved.insert(*uuid)
                {
                    allocated.insert(appearance.id.as_str(), *uuid);
                }
            }
        }
        for (appearance, uuid) in ir.model.appearances.iter().zip(&candidates) {
            if !allocated.contains_key(appearance.id.as_str())
                && counts[uuid] == 1
                && reserved.insert(*uuid)
            {
                allocated.insert(appearance.id.as_str(), *uuid);
            }
        }
        let mut pending = ir
            .model
            .appearances
            .iter()
            .filter(|appearance| !allocated.contains_key(appearance.id.as_str()))
            .collect::<Vec<_>>();
        pending.sort_by(|left, right| left.id.cmp(&right.id));
        for appearance in pending {
            let mut uuid = crate::writer::neutral_object_uuid(appearance.id.as_str());
            while !reserved.insert(uuid) {
                increment(&mut uuid);
            }
            allocated.insert(appearance.id.as_str(), uuid);
        }
        for (index, appearance) in ir.model.appearances.iter().enumerate() {
            let index_wire = wire_index(index)?;
            let uuid = allocated[appearance.id.as_str()];
            let stored = crate::wire::Uuid::from_wire(uuid).to_string();
            let stored_value = Some(stored.as_str());
            if appearance.asset_guid.as_deref() != stored_value {
                plan.losses.push(RhinoLossCode::WriterAttributesOmitted.note(format!("appearance {} /asset_guid {:?} -> {:?}; distinct Rhino material components require unique physical UUIDs",appearance.id,appearance.asset_guid,stored_value)));
            }
            let native = own_material(ir, appearance);
            if let Some(record) = native {
                let previous = record.field("source_uuid").unwrap_or(Value::Null);
                if previous != Value::String(stored.clone()) {
                    plan.losses.push(RhinoLossCode::WriterAttributesOmitted.note(format!("appearance {} linked native {} /source_uuid {} -> {:?}; distinct Rhino material components require unique physical UUIDs",appearance.id,record.id(),previous,stored)));
                }
                plan.native_targets
                    .entry(record.id().to_owned())
                    .or_default()
                    .push(index_wire);
            }
            plan.appearances
                .insert(appearance.id.to_string(), index_wire);
            plan.entries.push(Entry {
                identity: appearance.id.to_string(),
                appearance: Some(index),
                native: native.map(|record| record.id().to_owned()),
                uuid: Some(uuid),
            });
        }
        for record in unlinked {
            let index = wire_index(plan.entries.len())?;
            plan.native_targets
                .entry(record.id().to_owned())
                .or_default()
                .push(index);
            plan.entries.push(Entry {
                identity: record.id().to_owned(),
                appearance: None,
                native: Some(record.id().to_owned()),
                uuid: None,
            });
        }
        Ok(plan)
    }

    pub(in crate::writer) fn encode(&self, ir: &CadIr) -> Result<Vec<Vec<u8>>, CodecError> {
        self.entries
            .iter()
            .enumerate()
            .map(|(index, entry)| {
                let appearance = entry.appearance.map(|index| &ir.model.appearances[index]);
                let record = entry.native.as_ref().and_then(|id| {
                    records(ir, "materials")
                        .iter()
                        .find(|record| record.id() == id)
                });
                let fields = record.map(cadmpeg_ir::NativeRecord::fields).transpose()?;
                encode_material(
                    &entry.identity,
                    wire_index(index)?,
                    appearance,
                    fields.as_ref(),
                    entry.uuid,
                )
            })
            .collect()
    }

    pub(in crate::writer) fn material_index(&self, ir: &CadIr, identity: &str) -> Option<i32> {
        let appearance = bound_appearance(ir, identity)?;
        self.appearance_index(appearance.id.as_str())
    }
    pub(in crate::writer) fn native_targets(&self, id: &str) -> &[i32] {
        self.native_targets.get(id).map_or(&[], Vec::as_slice)
    }
    pub(in crate::writer) fn appearance_index(&self, id: &str) -> Option<i32> {
        self.appearances.get(id).copied()
    }
    pub(in crate::writer) fn target_names(ir: &CadIr, id: &str) -> Vec<String> {
        let mut names = ir
            .model
            .appearances
            .iter()
            .filter(|appearance| appearance.native_ref.as_deref() == Some(id))
            .map(|appearance| appearance.id.to_string())
            .collect::<Vec<_>>();
        names.sort();
        names
    }
    pub(in crate::writer) fn losses(&self) -> &[LossNote] {
        &self.losses
    }
}
