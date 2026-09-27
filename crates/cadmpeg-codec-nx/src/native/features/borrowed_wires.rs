// SPDX-License-Identifier: Apache-2.0
//! Borrowed native wires for feature references and derived membership columns.

use serde::ser::SerializeMap;
use serde::Serialize;

use super::{
    ColumnIndexRowKind, FeatureBlockConstruction, FeatureBlockDimensions, FeatureBodyReference,
    FeatureBooleanOperation, FeatureDatumCsysConstruction, FeatureDatumCsysDescriptor,
    FeatureExtrudeConstructionProfile, FeatureExtrudePayloadHeader, FeatureInputBlockIdentityGroup,
    FeatureInputColumnTarget, FeatureInputColumnTargetRow, FeatureOperationBodyMember,
    FeatureOperationBodyOperand, FeatureOperationObjectReference, FeatureParameterUse,
    FeaturePayloadScalar, FeatureScalarPayload, FeatureSketchConstructionInputs,
    FeatureSketchPayloadScalarLane, FeatureSketchPointUse, OffsetStoreNamedPoint,
};
use crate::iter_wire::IterWire;

impl Serialize for FeatureOperationObjectReference {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut wire = serializer.serialize_map(None)?;
        wire.serialize_entry("id", &self.id)?;
        wire.serialize_entry("operation_label", &self.operation_label)?;
        wire.serialize_entry("operation_record", &self.operation_record)?;
        wire.serialize_entry("ordinal", &self.ordinal)?;
        if let Some(tag) = self.frame.kind().tag() {
            wire.serialize_entry("tag", &tag)?;
        }
        wire.serialize_entry("object_index", &self.frame.object().value())?;
        wire.serialize_entry("raw_object_index", self.frame.object().raw())?;
        if let Some(data_block) = &self.data_block {
            wire.serialize_entry("data_block", data_block)?;
        }
        wire.serialize_entry("object_index_source_offset", &self.frame.object_offset())?;
        wire.serialize_entry("byte_len", &u64::from(self.frame.byte_len()))?;
        wire.serialize_entry("source_offset", &self.frame.offset())?;
        wire.end()
    }
}

impl Serialize for FeatureBodyReference {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut wire = serializer.serialize_map(None)?;
        wire.serialize_entry("id", &self.id)?;
        wire.serialize_entry("operation_label", &self.operation_label)?;
        if let Some(ordinal) = self.ordinal {
            wire.serialize_entry("ordinal", &ordinal)?;
        }
        wire.serialize_entry("body_object_index", &self.body.value())?;
        wire.serialize_entry("raw_body_object_index", self.body.raw())?;
        wire.serialize_entry("source_offset", &self.source_offset)?;
        wire.end()
    }
}

impl Serialize for FeatureInputBlockIdentityGroup {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut wire = serializer.serialize_map(None)?;
        wire.serialize_entry("id", &self.id)?;
        wire.serialize_entry("data_block", &self.data_block)?;
        wire.serialize_entry(
            "input_blocks",
            &IterWire(
                self.members
                    .iter()
                    .map(|member| member.input_block.as_str()),
            ),
        )?;
        wire.serialize_entry(
            "operation_labels",
            &IterWire(
                self.members
                    .iter()
                    .map(|member| member.operation_label.as_str()),
            ),
        )?;
        wire.serialize_entry(
            "input_slots",
            &IterWire(self.members.iter().map(|member| member.input_slot)),
        )?;
        wire.serialize_entry(
            "source_offsets",
            &IterWire(self.members.iter().map(|member| member.source_offset)),
        )?;
        wire.end()
    }
}

impl Serialize for FeatureInputColumnTarget {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let (kind, leading_index, leading_offset, discriminator, flag) = match self.row {
            FeatureInputColumnTargetRow::Linked {
                leading_index,
                leading_index_source_offset,
                discriminator,
                flag,
            } => (
                ColumnIndexRowKind::LinkedIndex,
                Some(leading_index),
                Some(leading_index_source_offset),
                Some(discriminator),
                Some(flag),
            ),
            FeatureInputColumnTargetRow::Target => {
                (ColumnIndexRowKind::TargetIndex, None, None, None, None)
            }
        };
        let mut wire = serializer.serialize_map(None)?;
        wire.serialize_entry("id", &self.id)?;
        wire.serialize_entry("input_block", &self.input_block)?;
        wire.serialize_entry("operation_label", &self.operation_label)?;
        wire.serialize_entry("input_slot", &self.input_slot)?;
        wire.serialize_entry("column_row", &self.column_row)?;
        wire.serialize_entry("row_kind", &kind)?;
        if let Some(value) = leading_index {
            wire.serialize_entry("leading_index", &value)?;
        }
        if let Some(value) = leading_offset {
            wire.serialize_entry("leading_index_source_offset", &value)?;
        }
        if let Some(value) = discriminator {
            wire.serialize_entry("discriminator", &value)?;
        }
        wire.serialize_entry("field_indices", &self.field_indices)?;
        wire.serialize_entry("field_data_blocks", &self.field_data_blocks)?;
        wire.serialize_entry("field_source_offsets", &self.field_source_offsets)?;
        if let Some(value) = flag {
            wire.serialize_entry("flag", &value)?;
        }
        wire.serialize_entry("mode", &self.mode)?;
        wire.serialize_entry("column_table", &self.column_table)?;
        wire.serialize_entry("data_block", &self.data_block)?;
        wire.serialize_entry("source_offset", &self.source_offset)?;
        wire.end()
    }
}

impl Serialize for FeatureParameterUse {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut wire = serializer.serialize_map(None)?;
        wire.serialize_entry("id", &self.id)?;
        wire.serialize_entry("operation_label", &self.operation_label)?;
        wire.serialize_entry("expression", &self.expression)?;
        wire.serialize_entry(
            "bindings",
            &IterWire(self.bindings.iter().map(|binding| binding.binding.as_str())),
        )?;
        wire.serialize_entry(
            "source_offsets",
            &IterWire(self.bindings.iter().map(|binding| binding.source_offset)),
        )?;
        wire.end()
    }
}

impl Serialize for FeatureDatumCsysConstruction {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut wire = serializer.serialize_map(None)?;
        wire.serialize_entry("id", &self.id)?;
        wire.serialize_entry("operation_label", &self.operation_label)?;
        wire.serialize_entry("control", &self.frame.control())?;
        wire.serialize_entry(
            "object_indices",
            &self
                .frame
                .members()
                .each_ref()
                .map(|(token, _)| token.value()),
        )?;
        wire.serialize_entry(
            "raw_object_indices",
            &self
                .frame
                .members()
                .each_ref()
                .map(|(token, _)| token.raw()),
        )?;
        wire.serialize_entry(
            "data_blocks",
            &self.frame.members().each_ref().map(|(_, binding)| binding),
        )?;
        wire.serialize_entry("source_offsets", &self.frame.offsets())?;
        wire.end()
    }
}

impl Serialize for FeatureDatumCsysDescriptor {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let descriptor = self.descriptor.descriptor();
        let mut wire = serializer.serialize_map(None)?;
        wire.serialize_entry("id", &self.id)?;
        wire.serialize_entry("operation_label", &self.operation_label)?;
        wire.serialize_entry("construction", &self.construction)?;
        wire.serialize_entry("reference_ordinal", &u8::from(self.reference_ordinal))?;
        wire.serialize_entry("data_block", &self.data_block)?;
        wire.serialize_entry("prefix", descriptor.prefix())?;
        wire.serialize_entry("identity", descriptor.identity().as_str())?;
        wire.serialize_entry("suffix", descriptor.suffix())?;
        wire.serialize_entry("source_offset", &self.descriptor.source_offset())?;
        wire.serialize_entry(
            "identity_source_offset",
            &self.descriptor.identity_source_offset(),
        )?;
        wire.end()
    }
}

impl Serialize for FeaturePayloadScalar {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut wire = serializer.serialize_map(None)?;
        wire.serialize_entry("id", &self.id)?;
        wire.serialize_entry("operation_label", &self.operation_label)?;
        match &self.payload {
            FeatureScalarPayload::DatumCsys { datum_csys_payload } => {
                wire.serialize_entry("datum_csys_payload", datum_csys_payload)?;
            }
            FeatureScalarPayload::Construction {
                construction_payload,
            } => wire.serialize_entry("construction_payload", construction_payload)?,
        }
        wire.serialize_entry("ordinal", &self.ordinal)?;
        wire.serialize_entry("field_code", &self.field_code)?;
        wire.serialize_entry("value", &self.scalar.value().get())?;
        wire.serialize_entry("raw_value", &self.scalar.raw())?;
        wire.serialize_entry("payload_offset", &self.payload_offset)?;
        wire.serialize_entry("source_offset", &self.source_offset)?;
        wire.end()
    }
}

impl Serialize for FeatureSketchConstructionInputs {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut wire = serializer.serialize_map(None)?;
        wire.serialize_entry("id", &self.id)?;
        wire.serialize_entry("operation_label", &self.operation_label)?;
        wire.serialize_entry("sketch_record", &self.sketch_record)?;
        wire.serialize_entry(
            "member_references",
            &IterWire(self.members.iter().map(|member| member.reference.as_str())),
        )?;
        wire.serialize_entry(
            "member_data_blocks",
            &IterWire(self.members.iter().map(|member| member.data_block.as_str())),
        )?;
        wire.serialize_entry("terminal_reference", &self.terminal_reference)?;
        wire.serialize_entry("terminal_data_block", &self.terminal_data_block)?;
        wire.end()
    }
}

impl Serialize for FeatureSketchPayloadScalarLane {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut wire = serializer.serialize_map(None)?;
        wire.serialize_entry("id", &self.id)?;
        wire.serialize_entry("operation_label", &self.operation_label)?;
        wire.serialize_entry("construction_payload", &self.construction_payload)?;
        wire.serialize_entry("ordinal", &self.ordinal)?;
        wire.serialize_entry("discriminator", self.lane.form().discriminator())?;
        wire.serialize_entry(
            "values",
            &IterWire(self.lane.iter().map(|(_, scalar, _)| scalar.value().get())),
        )?;
        wire.serialize_entry(
            "raw_values",
            &IterWire(self.lane.iter().map(|(_, scalar, _)| scalar.raw())),
        )?;
        wire.serialize_entry(
            "value_payload_offsets",
            &IterWire(self.lane.iter().map(|(offset, _, _)| offset)),
        )?;
        wire.serialize_entry("terminator_payload_offset", &self.lane.end())?;
        wire.serialize_entry("source_offset", &self.source_offset)?;
        wire.serialize_entry(
            "value_source_offsets",
            &IterWire(self.lane.iter().map(|(_, _, source)| *source)),
        )?;
        wire.serialize_entry("terminator_source_offset", &self.terminator_source_offset)?;
        wire.end()
    }
}

impl Serialize for OffsetStoreNamedPoint {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut wire = serializer.serialize_map(None)?;
        wire.serialize_entry("id", &self.id)?;
        wire.serialize_entry("name", &self.name)?;
        wire.serialize_entry("data_blocks", &self.data_blocks)?;
        wire.serialize_entry(
            "values",
            &self.values.map(|token| token.scalar.value().get()),
        )?;
        wire.serialize_entry("raw_values", &self.values.map(|token| token.scalar.raw()))?;
        wire.serialize_entry(
            "value_source_offsets",
            &self.values.map(|token| token.source_offset),
        )?;
        wire.serialize_entry("source_offset", &self.source_offset)?;
        wire.end()
    }
}

impl Serialize for FeatureSketchPointUse {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut wire = serializer.serialize_map(None)?;
        wire.serialize_entry("id", &self.id)?;
        wire.serialize_entry("operation_label", &self.operation_label)?;
        wire.serialize_entry(
            "sketch_references",
            &IterWire(
                self.references
                    .iter()
                    .map(|reference| reference.sketch_reference.as_str()),
            ),
        )?;
        wire.serialize_entry(
            "block_uses",
            &IterWire(
                self.references
                    .iter()
                    .map(|reference| reference.block_use.as_str()),
            ),
        )?;
        wire.serialize_entry("sketch_point_group", &self.sketch_point_group)?;
        wire.serialize_entry("named_point", &self.named_point)?;
        wire.serialize_entry(
            "source_offsets",
            &IterWire(
                self.references
                    .iter()
                    .map(|reference| reference.source_offset),
            ),
        )?;
        wire.end()
    }
}

impl Serialize for FeatureExtrudePayloadHeader {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut wire = serializer.serialize_map(None)?;
        wire.serialize_entry("id", &self.id)?;
        wire.serialize_entry("operation_label", &self.operation_label)?;
        wire.serialize_entry("scalars", &self.scalars.map(|scalar| scalar.value().get()))?;
        wire.serialize_entry(
            "raw_scalars",
            &self.scalars.map(crate::om::scalar::ShiftedBinary64::raw),
        )?;
        wire.serialize_entry("source_offset", &self.source_offset)?;
        wire.end()
    }
}

impl Serialize for FeatureOperationBodyMember {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut wire = serializer.serialize_map(None)?;
        wire.serialize_entry("id", &self.id)?;
        wire.serialize_entry("operation_label", &self.operation_label)?;
        wire.serialize_entry("body_reference_ordinal", &self.body_reference_ordinal)?;
        wire.serialize_entry("body_object_index", &self.body_object_index)?;
        wire.serialize_entry("ordinal", &self.ordinal)?;
        wire.serialize_entry("member_index", &self.member.atom.value())?;
        wire.serialize_entry("raw_member_index", self.member.atom.raw())?;
        wire.serialize_entry("source_offset", &self.member.offset)?;
        wire.end()
    }
}

impl Serialize for FeatureOperationBodyOperand {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut wire = serializer.serialize_map(None)?;
        wire.serialize_entry("id", &self.id)?;
        wire.serialize_entry("operation_label", &self.operation_label)?;
        wire.serialize_entry("body_object_index", &self.body_object_index)?;
        wire.serialize_entry("body_reference_ordinal", &self.body_reference_ordinal)?;
        wire.serialize_entry("ordinal", &self.ordinal)?;
        wire.serialize_entry("operand_object_index", &self.operand.atom.value())?;
        wire.serialize_entry("raw_operand_object_index", self.operand.atom.raw())?;
        if let Some(value) = &self.operand_data_block {
            wire.serialize_entry("operand_data_block", value)?;
        }
        if !self.segment_body_bindings.is_empty() {
            wire.serialize_entry("segment_body_bindings", &self.segment_body_bindings)?;
        }
        wire.serialize_entry("source_offset", &self.operand.offset)?;
        wire.end()
    }
}

impl Serialize for FeatureExtrudeConstructionProfile {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut wire = serializer.serialize_map(None)?;
        wire.serialize_entry("id", &self.id)?;
        wire.serialize_entry("operation_label", &self.operation_label)?;
        wire.serialize_entry(
            "object_indices",
            &IterWire(self.references.iter().map(|item| item.object_index)),
        )?;
        wire.serialize_entry(
            "data_blocks",
            &IterWire(self.references.iter().map(|item| item.data_block.as_str())),
        )?;
        wire.serialize_entry(
            "profile_source_offsets",
            &IterWire(
                self.references
                    .iter()
                    .map(|item| item.profile_source_offset),
            ),
        )?;
        wire.serialize_entry(
            "witness_source_offsets",
            &IterWire(
                self.references
                    .iter()
                    .map(|item| item.witness_source_offset),
            ),
        )?;
        wire.end()
    }
}

impl Serialize for FeatureBlockConstruction {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut wire = serializer.serialize_map(None)?;
        wire.serialize_entry("id", &self.id)?;
        wire.serialize_entry("operation_label", &self.operation_label)?;
        wire.serialize_entry("control", &self.control)?;
        wire.serialize_entry(
            "member_references",
            &self
                .members
                .each_ref()
                .map(|member| member.reference.as_str()),
        )?;
        wire.serialize_entry(
            "member_data_blocks",
            &self
                .members
                .each_ref()
                .map(|member| member.data_block.as_str()),
        )?;
        wire.serialize_entry("terminal_reference", &self.terminal_reference)?;
        wire.serialize_entry("terminal_data_block", &self.terminal_data_block)?;
        wire.end()
    }
}

impl Serialize for FeatureBlockDimensions {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut wire = serializer.serialize_map(None)?;
        wire.serialize_entry("id", &self.id)?;
        wire.serialize_entry("operation_label", &self.operation_label)?;
        wire.serialize_entry("construction", &self.construction)?;
        wire.serialize_entry("anchor_bindings", &self.anchor_bindings)?;
        wire.serialize_entry(
            "declarations",
            &self
                .dimensions
                .each_ref()
                .map(|dimension| dimension.declaration.as_str()),
        )?;
        wire.serialize_entry(
            "expressions",
            &self
                .dimensions
                .each_ref()
                .map(|dimension| dimension.expression.as_str()),
        )?;
        wire.serialize_entry(
            "values",
            &self.dimensions.each_ref().map(|dimension| dimension.value),
        )?;
        wire.end()
    }
}

impl Serialize for FeatureBooleanOperation {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut wire = serializer.serialize_map(None)?;
        wire.serialize_entry("id", &self.id)?;
        wire.serialize_entry("operation_label", &self.operation_label)?;
        wire.serialize_entry("kind", &self.kind)?;
        wire.serialize_entry("target_object_index", &self.target.token.value())?;
        wire.serialize_entry("raw_target_object_index", self.target.token.raw())?;
        wire.serialize_entry("target_source_offset", &self.target.offset)?;
        wire.serialize_entry(
            "tool_object_indices",
            &IterWire(self.tools.iter().map(|tool| tool.token.value())),
        )?;
        wire.serialize_entry(
            "raw_tool_object_indices",
            &IterWire(self.tools.iter().map(|tool| tool.token.raw())),
        )?;
        wire.serialize_entry(
            "tool_source_offsets",
            &IterWire(self.tools.iter().map(|tool| tool.offset)),
        )?;
        wire.serialize_entry("source_offset", &self.source_offset)?;
        wire.end()
    }
}

#[cfg(test)]
mod tests {
    use super::super::{
        FeatureBlockConstructionWire, FeatureBlockDimensionsWire, FeatureBodyReferenceWire,
        FeatureBooleanOperationWire, FeatureDatumCsysConstructionWire,
        FeatureDatumCsysDescriptorWire, FeatureExtrudeConstructionProfileWire,
        FeatureExtrudePayloadHeaderWire, FeatureInputBlockIdentityGroupWire,
        FeatureInputColumnTargetWire, FeatureOperationBodyMemberWire,
        FeatureOperationBodyOperandWire, FeatureOperationObjectReferenceWire,
        FeatureParameterUseWire, FeaturePayloadScalarWire, FeatureSketchConstructionInputsWire,
        FeatureSketchPayloadScalarLaneWire, FeatureSketchPointUseWire, OffsetStoreNamedPointWire,
    };
    use super::{
        FeatureBlockConstruction, FeatureBlockDimensions, FeatureBodyReference,
        FeatureBooleanOperation, FeatureDatumCsysConstruction, FeatureDatumCsysDescriptor,
        FeatureExtrudeConstructionProfile, FeatureExtrudePayloadHeader,
        FeatureInputBlockIdentityGroup, FeatureInputColumnTarget, FeatureOperationBodyMember,
        FeatureOperationBodyOperand, FeatureOperationObjectReference, FeatureParameterUse,
        FeaturePayloadScalar, FeatureSketchConstructionInputs, FeatureSketchPayloadScalarLane,
        FeatureSketchPointUse, OffsetStoreNamedPoint,
    };

    macro_rules! route_tests {
        ($bytes:ident, $limit:ident, $record:ty, $wire:ty, $json:expr) => {
            #[test]
            fn $bytes() {
                let json = $json;
                let record: $record = serde_json::from_str(json).unwrap();
                assert_eq!(serde_json::to_vec(&record).unwrap(), json.as_bytes());
                assert_eq!(
                    serde_json::to_vec(&record).unwrap(),
                    serde_json::to_vec(&<$wire>::from(record.clone())).unwrap()
                );
            }

            #[test]
            fn $limit() {
                let json = $json;
                let record: $record = serde_json::from_str(json).unwrap();
                cadmpeg_test_support::native_serialization::assert_native_limit(
                    &record,
                    serde_json::from_str::<serde_json::Value>(json).unwrap(),
                );
            }
        };
    }

    route_tests!(
        operation_object_reference_borrowed_wire_preserves_bytes,
        operation_object_reference_native_limit_refuses_before_clone,
        FeatureOperationObjectReference,
        FeatureOperationObjectReferenceWire,
        r#"{"id":"nx:feature:object-reference#0","operation_label":"operation","operation_record":"record","ordinal":0,"tag":23,"object_index":1,"raw_object_index":[1],"data_block":"block","object_index_source_offset":103,"byte_len":9,"source_offset":100}"#
    );
    route_tests!(
        body_reference_borrowed_wire_preserves_bytes,
        body_reference_native_limit_refuses_before_clone,
        FeatureBodyReference,
        FeatureBodyReferenceWire,
        r#"{"id":"nx:feature:body-reference#0","operation_label":"operation","ordinal":0,"body_object_index":1,"raw_body_object_index":[1],"source_offset":100}"#
    );
    route_tests!(
        input_identity_group_borrowed_wire_preserves_bytes,
        input_identity_group_native_limit_refuses_before_clone,
        FeatureInputBlockIdentityGroup,
        FeatureInputBlockIdentityGroupWire,
        r#"{"id":"nx:feature:input-group#0","data_block":"block","input_blocks":["a","b"],"operation_labels":["one","two"],"input_slots":[0,1],"source_offsets":[100,200]}"#
    );
    route_tests!(
        input_column_target_borrowed_wire_preserves_bytes,
        input_column_target_native_limit_refuses_before_clone,
        FeatureInputColumnTarget,
        FeatureInputColumnTargetWire,
        r#"{"id":"nx:feature:column-target#0","input_block":"input","operation_label":"operation","input_slot":0,"column_row":"row","row_kind":"linked_index","leading_index":1,"leading_index_source_offset":90,"discriminator":22,"field_indices":[2,3,4],"field_data_blocks":["a","b","c"],"field_source_offsets":[101,102,103],"flag":3,"mode":7,"column_table":"table","data_block":"block","source_offset":100}"#
    );
    route_tests!(
        parameter_use_borrowed_wire_preserves_bytes,
        parameter_use_native_limit_refuses_before_clone,
        FeatureParameterUse,
        FeatureParameterUseWire,
        r#"{"id":"nx:feature:parameter-use#0","operation_label":"operation","expression":"expression","bindings":["a","b"],"source_offsets":[100,200]}"#
    );
    route_tests!(
        datum_csys_construction_borrowed_wire_preserves_bytes,
        datum_csys_construction_native_limit_refuses_before_clone,
        FeatureDatumCsysConstruction,
        FeatureDatumCsysConstructionWire,
        r#"{"id":"nx:feature:datum-csys-construction#0","operation_label":"operation","control":19,"object_indices":[0,1,2,3,4,5,6,7],"raw_object_indices":[[240,0],[240,1],[240,2],[240,3],[240,4],[240,5],[240,6],[240,7]],"data_blocks":["a","b","c","d","e","f","g","h"],"source_offsets":[14,16,18,20,22,24,26,28]}"#
    );
    route_tests!(
        datum_csys_descriptor_borrowed_wire_preserves_bytes,
        datum_csys_descriptor_native_limit_refuses_before_clone,
        FeatureDatumCsysDescriptor,
        FeatureDatumCsysDescriptorWire,
        r#"{"id":"nx:feature:datum-csys-descriptor#0","operation_label":"operation","construction":"construction","reference_ordinal":7,"data_block":"block","prefix":[2,1],"identity":"012345678901234567890123456789","suffix":[63,65],"source_offset":10,"identity_source_offset":12}"#
    );
    route_tests!(
        payload_scalar_borrowed_wire_preserves_bytes,
        payload_scalar_native_limit_refuses_before_clone,
        FeaturePayloadScalar,
        FeaturePayloadScalarWire,
        r#"{"id":"nx:feature:payload-scalar#0","operation_label":"operation","datum_csys_payload":"payload","ordinal":0,"field_code":100,"value":2.0,"raw_value":[48,0,0,0,0,0,0,0],"payload_offset":10,"source_offset":20}"#
    );
    route_tests!(
        sketch_construction_inputs_borrowed_wire_preserves_bytes,
        sketch_construction_inputs_native_limit_refuses_before_clone,
        FeatureSketchConstructionInputs,
        FeatureSketchConstructionInputsWire,
        r#"{"id":"nx:feature:sketch-inputs#0","operation_label":"operation","sketch_record":"sketch","member_references":["reference"],"member_data_blocks":["block"],"terminal_reference":"terminal","terminal_data_block":"last"}"#
    );
    route_tests!(
        sketch_scalar_lane_borrowed_wire_preserves_bytes,
        sketch_scalar_lane_native_limit_refuses_before_clone,
        FeatureSketchPayloadScalarLane,
        FeatureSketchPayloadScalarLaneWire,
        r#"{"id":"nx:feature:sketch-scalar-lane#0","operation_label":"operation","construction_payload":"payload","ordinal":0,"discriminator":[37,37,65,0,4,1,7,1,192,69,16,0,128,134,2,0,1,0],"values":[2.5,4.0],"raw_values":[[80,32,0,0],[80,128,0,0]],"value_payload_offsets":[18,22],"terminator_payload_offset":26,"source_offset":100,"value_source_offsets":[118,122],"terminator_source_offset":126}"#
    );
    route_tests!(
        named_point_borrowed_wire_preserves_bytes,
        named_point_native_limit_refuses_before_clone,
        OffsetStoreNamedPoint,
        OffsetStoreNamedPointWire,
        r#"{"id":"nx:feature:named-point#0","name":"Point1","data_blocks":["first","second"],"values":[1.0,2.0],"raw_values":[[47,240,0,0,0,0,0,0],[48,0,0,0,0,0,0,0]],"value_source_offsets":[10,20],"source_offset":5}"#
    );
    route_tests!(
        sketch_point_use_borrowed_wire_preserves_bytes,
        sketch_point_use_native_limit_refuses_before_clone,
        FeatureSketchPointUse,
        FeatureSketchPointUseWire,
        r#"{"id":"nx:feature:sketch-point-use#0","operation_label":"operation","sketch_references":["ref"],"block_uses":["use"],"sketch_point_group":"group","named_point":"point","source_offsets":[10]}"#
    );
    route_tests!(
        extrude_payload_header_borrowed_wire_preserves_bytes,
        extrude_payload_header_native_limit_refuses_before_clone,
        FeatureExtrudePayloadHeader,
        FeatureExtrudePayloadHeaderWire,
        r#"{"id":"nx:feature:extrude-header#0","operation_label":"operation","scalars":[1.0,2.0],"raw_scalars":[[47,240,0,0,0,0,0,0],[48,0,0,0,0,0,0,0]],"source_offset":100}"#
    );
    route_tests!(
        operation_body_member_borrowed_wire_preserves_bytes,
        operation_body_member_native_limit_refuses_before_clone,
        FeatureOperationBodyMember,
        FeatureOperationBodyMemberWire,
        r#"{"id":"nx:feature:body-member#0","operation_label":"operation","body_reference_ordinal":0,"body_object_index":66,"ordinal":0,"member_index":4097,"raw_member_index":[144,1],"source_offset":122}"#
    );
    route_tests!(
        operation_body_operand_borrowed_wire_preserves_bytes,
        operation_body_operand_native_limit_refuses_before_clone,
        FeatureOperationBodyOperand,
        FeatureOperationBodyOperandWire,
        r#"{"id":"nx:feature:body-operand#0","operation_label":"operation","body_object_index":66,"body_reference_ordinal":0,"ordinal":0,"operand_object_index":4097,"raw_operand_object_index":[144,1],"operand_data_block":"block","segment_body_bindings":["binding"],"source_offset":122}"#
    );

    route_tests!(
        extrude_profile_borrowed_wire_preserves_bytes,
        extrude_profile_native_limit_refuses_before_clone,
        FeatureExtrudeConstructionProfile,
        FeatureExtrudeConstructionProfileWire,
        r#"{"id":"nx:feature:extrude-profile#0","operation_label":"operation","object_indices":[1],"data_blocks":["block"],"profile_source_offsets":[10],"witness_source_offsets":[20]}"#
    );
    route_tests!(
        block_dimensions_borrowed_wire_preserves_bytes,
        block_dimensions_native_limit_refuses_before_clone,
        FeatureBlockDimensions,
        FeatureBlockDimensionsWire,
        r#"{"id":"nx:feature:block-dimensions#0","operation_label":"operation","construction":"construction","anchor_bindings":["binding"],"declarations":["d1","d2","d3"],"expressions":["e1","e2","e3"],"values":[1.0,2.0,3.0]}"#
    );
    route_tests!(
        boolean_operation_borrowed_wire_preserves_bytes,
        boolean_operation_native_limit_refuses_before_clone,
        FeatureBooleanOperation,
        FeatureBooleanOperationWire,
        r#"{"id":"nx:feature:boolean#0","operation_label":"operation","kind":"subtract","target_object_index":10,"raw_target_object_index":[10],"target_source_offset":100,"tool_object_indices":[20,30],"raw_tool_object_indices":[[20],[30]],"tool_source_offsets":[110,120],"source_offset":90}"#
    );

    #[test]
    fn block_construction_borrowed_wire_preserves_bytes_and_retained_limit() {
        let references = (0..18).map(|n| format!("reference{n}")).collect::<Vec<_>>();
        let blocks = (0..18).map(|n| format!("block{n}")).collect::<Vec<_>>();
        let json = format!(
            r#"{{"id":"nx:feature:block-construction#0","operation_label":"operation","control":38,"member_references":{},"member_data_blocks":{},"terminal_reference":"terminal","terminal_data_block":"last"}}"#,
            serde_json::to_string(&references).unwrap(),
            serde_json::to_string(&blocks).unwrap()
        );
        let record: FeatureBlockConstruction = serde_json::from_str(&json).unwrap();
        assert_eq!(serde_json::to_vec(&record).unwrap(), json.as_bytes());
        assert_eq!(
            serde_json::to_vec(&record).unwrap(),
            serde_json::to_vec(&FeatureBlockConstructionWire::from(record.clone())).unwrap()
        );
        cadmpeg_test_support::native_serialization::assert_native_limit(
            &record,
            serde_json::from_str::<serde_json::Value>(&json).unwrap(),
        );
    }

    #[test]
    fn input_column_target_borrowed_wire_omits_linked_fields_for_target_rows() {
        let json = r#"{"id":"nx:feature:column-target#0","input_block":"input","operation_label":"operation","input_slot":0,"column_row":"row","row_kind":"target_index","field_indices":[2,3,4],"field_data_blocks":["a","b","c"],"field_source_offsets":[101,102,103],"mode":7,"column_table":"table","data_block":"block","source_offset":100}"#;
        let record: FeatureInputColumnTarget = serde_json::from_str(json).unwrap();
        assert_eq!(serde_json::to_vec(&record).unwrap(), json.as_bytes());
    }
}
