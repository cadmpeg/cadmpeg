// SPDX-License-Identifier: Apache-2.0
//! One checked attribute-field position and the legacy derived wire fields.

use serde::{Deserialize, Serialize};

use crate::parasolid::attribute_field::AttributeField;
use crate::parasolid::entity_references::FieldPosition;

use super::{ParasolidAttributeFieldUse, ParasolidAttributeFieldValueKind};

#[derive(Serialize, Deserialize)]
pub(super) struct FieldUseWire {
    /// Globally unique relation identity.
    id: String,
    /// Zero-based inflated Parasolid stream ordinal.
    stream_ordinal: u32,
    /// Resolved class relation for the attribute instance.
    attribute_class_use: String,
    /// Type-81 attribute-instance record.
    entity_51_record: String,
    /// Uniquely matched attribute definition.
    attribute_definition: String,
    /// Zero-based position in the type-80 field declaration.
    field_ordinal: u32,
    /// Declared type-80 field code.
    field_code: AttributeField,
    /// Zero-based position in the complete type-81 reference lane.
    reference_ordinal: u32,
    /// Resolved value-record family.
    value_kind: ParasolidAttributeFieldValueKind,
    /// Type-81-to-value relation carrying this field.
    value_use: String,
    /// Uniquely resolved value record.
    value_record: String,
    /// Offset of the owning type-81 record in the inflated stream.
    inflated_offset: u64,
}

impl From<ParasolidAttributeFieldUse> for FieldUseWire {
    fn from(value: ParasolidAttributeFieldUse) -> Self {
        Self {
            field_ordinal: value.position.field_ordinal(),
            reference_ordinal: value.position.reference_ordinal(),
            field_code: value.value_kind.field_code(),
            id: value.id,
            stream_ordinal: value.stream_ordinal,
            attribute_class_use: value.attribute_class_use,
            entity_51_record: value.entity_51_record,
            attribute_definition: value.attribute_definition,
            value_kind: value.value_kind,
            value_use: value.value_use,
            value_record: value.value_record,
            inflated_offset: value.inflated_offset,
        }
    }
}

impl TryFrom<FieldUseWire> for ParasolidAttributeFieldUse {
    type Error = &'static str;

    fn try_from(wire: FieldUseWire) -> Result<Self, Self::Error> {
        let position = FieldPosition::try_from(wire.reference_ordinal)?;
        if position.field_ordinal() != wire.field_ordinal {
            return Err("field_ordinal disagrees with reference_ordinal");
        }
        if wire.field_code != wire.value_kind.field_code() {
            return Err("field_code disagrees with value_kind");
        }
        Ok(Self {
            position,
            id: wire.id,
            stream_ordinal: wire.stream_ordinal,
            attribute_class_use: wire.attribute_class_use,
            entity_51_record: wire.entity_51_record,
            attribute_definition: wire.attribute_definition,
            value_kind: wire.value_kind,
            value_use: wire.value_use,
            value_record: wire.value_record,
            inflated_offset: wire.inflated_offset,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::ParasolidAttributeFieldUse;

    #[test]
    fn field_use_wire_preserves_and_checks_derived_fields() {
        let wire = r#"{"id":"field","stream_ordinal":0,"attribute_class_use":"class","entity_51_record":"entity","attribute_definition":"definition","field_ordinal":1,"field_code":2,"reference_ordinal":6,"value_kind":"doubles","value_use":"use","value_record":"value","inflated_offset":8}"#;
        let value: ParasolidAttributeFieldUse = serde_json::from_str(wire).unwrap();
        assert_eq!(serde_json::to_string(&value).unwrap(), wire);
        for invalid in [
            wire.replace("\"field_ordinal\":1", "\"field_ordinal\":2"),
            wire.replace("\"field_code\":2", "\"field_code\":1"),
            wire.replace("\"reference_ordinal\":6", "\"reference_ordinal\":4"),
        ] {
            assert!(serde_json::from_str::<ParasolidAttributeFieldUse>(&invalid).is_err());
        }
    }
}
