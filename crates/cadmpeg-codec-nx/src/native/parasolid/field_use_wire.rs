// SPDX-License-Identifier: Apache-2.0
//! One checked attribute-field position and the legacy derived wire fields.

use serde::{Deserialize, Serialize};

use crate::parasolid::attribute_field::AttributeField;
use crate::parasolid::entity_references::FieldPosition;

use super::{ParasolidAttributeFieldUse, ParasolidAttributeFieldValueKind};

#[derive(Deserialize)]
#[cfg_attr(test, derive(Serialize))]
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

#[derive(Serialize)]
struct FieldUseRef<'a> {
    id: &'a str,
    stream_ordinal: u32,
    attribute_class_use: &'a str,
    entity_51_record: &'a str,
    attribute_definition: &'a str,
    field_ordinal: u32,
    field_code: AttributeField,
    reference_ordinal: u32,
    value_kind: ParasolidAttributeFieldValueKind,
    value_use: &'a str,
    value_record: &'a str,
    inflated_offset: u64,
}

impl Serialize for ParasolidAttributeFieldUse {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        FieldUseRef {
            id: &self.id,
            stream_ordinal: self.stream_ordinal,
            attribute_class_use: &self.attribute_class_use,
            entity_51_record: &self.entity_51_record,
            attribute_definition: &self.attribute_definition,
            field_ordinal: self.position.field_ordinal(),
            field_code: self.value_kind.field_code(),
            reference_ordinal: self.position.reference_ordinal(),
            value_kind: self.value_kind,
            value_use: &self.value_use,
            value_record: &self.value_record,
            inflated_offset: self.inflated_offset,
        }
        .serialize(serializer)
    }
}

#[cfg(test)]
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
    use super::super::ATTRIBUTE_FIELD_USE_CLONE_COUNT;
    use super::{FieldUseWire, ParasolidAttributeFieldUse};

    #[test]
    fn field_use_wire_preserves_and_checks_derived_fields() {
        let wire = r#"{"id":"field","stream_ordinal":0,"attribute_class_use":"class","entity_51_record":"entity","attribute_definition":"definition","field_ordinal":1,"field_code":2,"reference_ordinal":6,"value_kind":"doubles","value_use":"use","value_record":"value","inflated_offset":8}"#;
        let value: ParasolidAttributeFieldUse = serde_json::from_str(wire).unwrap();
        assert_eq!(serde_json::to_string(&value).unwrap(), wire);
        assert_eq!(
            serde_json::to_vec(&value).unwrap(),
            serde_json::to_vec(&FieldUseWire::from(value.clone())).unwrap()
        );
        for invalid in [
            wire.replace("\"field_ordinal\":1", "\"field_ordinal\":2"),
            wire.replace("\"field_code\":2", "\"field_code\":1"),
            wire.replace("\"reference_ordinal\":6", "\"reference_ordinal\":4"),
        ] {
            assert!(serde_json::from_str::<ParasolidAttributeFieldUse>(&invalid).is_err());
        }
    }

    #[test]
    fn attribute_field_use_retained_limit_refuses_borrowed_serialization() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_ir::NativeNamespace;

        let wire = r#"{"id":"nx:s3:attribute-field-use#2-8","stream_ordinal":3,"attribute_class_use":"class","entity_51_record":"entity","attribute_definition":"definition","field_ordinal":1,"field_code":2,"reference_ordinal":6,"value_kind":"doubles","value_use":"use","value_record":"value","inflated_offset":8}"#;
        let record: ParasolidAttributeFieldUse = serde_json::from_str(wire).unwrap();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = u64::try_from(wire.len()).unwrap() - 1;
        let (limited, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        ATTRIBUTE_FIELD_USE_CLONE_COUNT.with(|count| count.set(0));
        let error = NativeNamespace::default()
            .set_arena(&limited, "a", std::slice::from_ref(&record))
            .unwrap_err();
        ATTRIBUTE_FIELD_USE_CLONE_COUNT.with(|count| assert_eq!(count.get(), 0));
        assert!(matches!(cadmpeg_core::CodecError::from(error),
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.operation == "serialize native record"));

        let (service, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
        NativeNamespace::default()
            .set_arena(&service, "a", &[record])
            .unwrap();
        ATTRIBUTE_FIELD_USE_CLONE_COUNT.with(|count| assert_eq!(count.get(), 0));
    }
}
