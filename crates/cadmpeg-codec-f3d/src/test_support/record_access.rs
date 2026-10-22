// SPDX-License-Identifier: Apache-2.0
//! Test access to admitted native records: wire round trips, content edits
//! and reads of fields that production code does not expose.

use crate::records::mesh::DesignRelaxedGuidText;
use crate::records::parameters::{DesignParameterOwner, DesignParameterOwnerWire};
use crate::records::sketch_relations::{SketchRelation, SketchRelationContent};
use crate::records::topology::edge_identity::DesignEdgeIdentityOperand;

impl TryFrom<DesignParameterOwner> for DesignParameterOwnerWire {
    type Error = String;
    fn try_from(owner: DesignParameterOwner) -> Result<Self, Self::Error> {
        Ok(Self {
            id: owner.id().clone(),
            class_tag: owner.class_tag().clone(),
            byte_offset: owner
                .byte_offset()
                .ok_or("source owner offset is unavailable")?,
            frame_length: owner
                .frame_length()
                .ok_or("source owner frame is unavailable")?,
            record_index: owner.record_index(),
            scope_record_index: owner.scope_record_index().unwrap_or(0),
            local_ordinal: owner.local_ordinal().unwrap_or(0),
            evaluated_value: owner.evaluated_value().get(),
            evaluated_value_offset: owner
                .evaluated_value_offset()
                .ok_or("source owner value offset is unavailable")?,
            parameter_record_index: owner.parameter_record_index(),
            owned_ordinal: owner.owned_ordinal(),
            variant: owner.variant(),
            companion_record_index: owner.companion_record_index(),
        })
    }
}

/// Edit a source-less sketch-relation fixture through its typed semantic
/// constructor.
pub(crate) trait SketchRelationEdit {
    fn edit_content(&mut self, edit: impl FnOnce(&mut SketchRelationContent));
}

impl SketchRelationEdit for SketchRelation {
    fn edit_content(&mut self, edit: impl FnOnce(&mut SketchRelationContent)) {
        let mut content = SketchRelationContent {
            id: self.id.clone(),
            record_index: self.record_index,
            class_tag: self.class_tag.clone(),
            owner_reference: self.owner_reference,
            owner_entity_id: self.owner_entity_id.clone(),
            auxiliary_references: self.auxiliary_references().clone(),
            rectangular_counted_reference_count: self.rectangular_counted_reference_count,
            members: self.members().clone(),
            definition: self.definition.clone(),
            entity_genesis: self.entity_genesis,
            return_members: self.return_members().clone(),
        };
        edit(&mut content);
        *self = Self::authored(content);
    }
}

/// Read the GUID fields of an edge identity through its serialized form.
pub(crate) trait EdgeIdentityGuids {
    fn asset_id(&self) -> DesignRelaxedGuidText;
    fn context_id(&self) -> DesignRelaxedGuidText;
}

fn serialized_guid(identity: &DesignEdgeIdentityOperand, key: &str) -> DesignRelaxedGuidText {
    let value = serde_json::to_value(identity).expect("serialize edge identity");
    serde_json::from_value(value[key].clone()).expect("edge identity GUID")
}

impl EdgeIdentityGuids for DesignEdgeIdentityOperand {
    fn asset_id(&self) -> DesignRelaxedGuidText {
        serialized_guid(self, "asset_id")
    }
    fn context_id(&self) -> DesignRelaxedGuidText {
        serialized_guid(self, "context_id")
    }
}
