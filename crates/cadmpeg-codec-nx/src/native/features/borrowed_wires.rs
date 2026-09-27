// SPDX-License-Identifier: Apache-2.0
//! Borrowed native wires for feature references and derived membership columns.

use serde::ser::SerializeMap;
use serde::Serialize;

use super::{
    ColumnIndexRowKind, FeatureBodyReference, FeatureDatumCsysConstruction,
    FeatureDatumCsysDescriptor, FeatureInputBlockIdentityGroup, FeatureInputColumnTarget,
    FeatureInputColumnTargetRow, FeatureOperationObjectReference, FeatureParameterUse,
    FeaturePayloadScalar, FeatureScalarPayload, FeatureSketchConstructionInputs,
    FeatureSketchPayloadScalarLane,
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
                wire.serialize_entry("datum_csys_payload", datum_csys_payload)?
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

#[cfg(test)]
mod tests {
    use super::super::{
        FeatureBodyReferenceWire, FeatureDatumCsysConstructionWire, FeatureDatumCsysDescriptorWire,
        FeatureInputBlockIdentityGroupWire, FeatureInputColumnTargetWire,
        FeatureOperationObjectReferenceWire, FeatureParameterUseWire, FeaturePayloadScalarWire,
        FeatureSketchConstructionInputsWire, FeatureSketchPayloadScalarLaneWire,
    };
    use super::{
        FeatureBodyReference, FeatureDatumCsysConstruction, FeatureDatumCsysDescriptor,
        FeatureInputBlockIdentityGroup, FeatureInputColumnTarget, FeatureOperationObjectReference,
        FeatureParameterUse, FeaturePayloadScalar, FeatureSketchConstructionInputs,
        FeatureSketchPayloadScalarLane,
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

    #[test]
    fn input_column_target_borrowed_wire_omits_linked_fields_for_target_rows() {
        let json = r#"{"id":"nx:feature:column-target#0","input_block":"input","operation_label":"operation","input_slot":0,"column_row":"row","row_kind":"target_index","field_indices":[2,3,4],"field_data_blocks":["a","b","c"],"field_source_offsets":[101,102,103],"mode":7,"column_table":"table","data_block":"block","source_offset":100}"#;
        let record: FeatureInputColumnTarget = serde_json::from_str(json).unwrap();
        assert_eq!(serde_json::to_vec(&record).unwrap(), json.as_bytes());
    }
}
