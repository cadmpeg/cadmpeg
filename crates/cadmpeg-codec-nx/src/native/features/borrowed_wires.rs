// SPDX-License-Identifier: Apache-2.0
//! Borrowed native wires for feature references and derived membership columns.

use serde::ser::SerializeMap;
use serde::Serialize;

use super::{
    ColumnIndexRowKind, FeatureBodyReference, FeatureInputBlockIdentityGroup,
    FeatureInputColumnTarget, FeatureInputColumnTargetRow, FeatureOperationObjectReference,
    FeatureParameterUse,
};
use crate::native::iter_wire::IterWire;

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

#[cfg(test)]
mod tests {
    use super::super::{
        FeatureBodyReferenceWire, FeatureInputBlockIdentityGroupWire, FeatureInputColumnTargetWire,
        FeatureOperationObjectReferenceWire, FeatureParameterUseWire,
    };
    use super::{
        FeatureBodyReference, FeatureInputBlockIdentityGroup, FeatureInputColumnTarget,
        FeatureOperationObjectReference, FeatureParameterUse,
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

    #[test]
    fn input_column_target_borrowed_wire_omits_linked_fields_for_target_rows() {
        let json = r#"{"id":"nx:feature:column-target#0","input_block":"input","operation_label":"operation","input_slot":0,"column_row":"row","row_kind":"target_index","field_indices":[2,3,4],"field_data_blocks":["a","b","c"],"field_source_offsets":[101,102,103],"mode":7,"column_table":"table","data_block":"block","source_offset":100}"#;
        let record: FeatureInputColumnTarget = serde_json::from_str(json).unwrap();
        assert_eq!(serde_json::to_vec(&record).unwrap(), json.as_bytes());
    }
}
