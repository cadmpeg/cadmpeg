// SPDX-License-Identifier: Apache-2.0
//! Entity-table bodies and their resolved native productions.

use super::{
    entity_suffix_framing, entity_suffix_value, CatiaConstraintRange, CatiaDefinitionChainValue,
    CatiaDefinitionSchemaSelection, CatiaDefinitionValue, CatiaEntitySuffixFraming,
    CatiaEntitySuffixSchemaSelection, CatiaEntitySuffixValue, CatiaEntityValueSchemaSelection,
    CatiaFormulaRelation, CatiaParameterValue, CatiaRangeInterval, CatiaReferenceSignature,
    CatiaRelationExpression, CatiaRelationProgramInstance, CatiaSchemaConfigurationRecord,
    CatiaSchemaConfigurationRowLink,
};
use crate::{entity_table, value_block};
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Complete production selected by the entity value and suffix frames.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CatiaEntityValueProduction {
    RelationExpression(CatiaRelationExpression),
    ParameterValue(CatiaParameterValue),
    ConstraintRange(CatiaConstraintRange),
    DefinitionValue(CatiaDefinitionValue),
    DefinitionChainValue(CatiaDefinitionChainValue),
}

/// Complete production of the paired object payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CatiaEntityObjectProduction {
    RelationProgramInstance(CatiaRelationProgramInstance),
    SchemaConfigurationRecord(CatiaSchemaConfigurationRecord),
    SchemaConfigurationRowLink(CatiaSchemaConfigurationRowLink),
    FormulaRelation(CatiaFormulaRelation),
}

/// Inline or nested body of one `7C05` entity-table record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CatiaEntityRecordBody {
    /// Complete alternate inline body, including its lead byte.
    Inline(Vec<u8>),
    /// Nested `7C06` definition and `7C07` value frames.
    Nested {
        /// Stored nested `7C06` length.
        definition_len: u32,
        /// Exact definition prefix before the `0xEA` identity delimiter.
        definition_prefix: Vec<u8>,
        /// Exact definition bytes after the identity.
        definition_suffix: Vec<u8>,
        /// Stored nested `7C07` total length.
        value_len: u32,
        /// Exact nested `7C07` payload.
        value_payload: Vec<u8>,
        /// Exact bytes after the nested `7C07` frame.
        record_suffix: Vec<u8>,
    },
}

/// Exclusive occupant of an entity-record suffix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CatiaEntityRecordSuffix {
    /// Complete typed value production.
    Value(CatiaEntitySuffixValue),
    /// Complete non-value framing.
    Framing(CatiaEntitySuffixFraming),
}

/// One `7C05` entity-table record paired with a `7C09` object record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "CatiaEntityRecordWire", into = "CatiaEntityRecordWire")]
pub struct CatiaEntityRecord {
    /// Globally unique entity-record identity.
    pub id: String,
    /// Object graph whose record occupies the same table position.
    pub object_graph: String,
    /// Positionally paired `7C09` object record.
    pub object_record: String,
    /// Stable serialized order within the table run.
    pub ordinal: u64,
    /// Byte offset of the `7C05` marker.
    pub byte_offset: u64,
    /// Total framed byte length.
    pub byte_len: u64,
    /// Byte between the `7C05` length and nested `7C06` marker.
    pub lead: u8,
    /// Inline body or nested definition/value frames.
    pub body: CatiaEntityRecordBody,
    /// Definition selectors resolved against the containing graph's source schema.
    pub definition_schema_selections: Vec<CatiaDefinitionSchemaSelection>,
    /// Stored identity used by object-record owner and payload references.
    pub entity_id: u32,
    /// Value selectors resolved against the containing graph's source schema.
    pub value_schema_selections: Vec<CatiaEntityValueSchemaSelection>,
    /// Mutually exclusive production of the paired object payload.
    pub object_production: Option<CatiaEntityObjectProduction>,
    /// Exclusive value/suffix production; the independent Range interval remains separate.
    pub value_production: Option<CatiaEntityValueProduction>,
    /// Complete source-schema `Range` interval production.
    pub range_interval: Option<CatiaRangeInterval>,
    /// Complete reference signature when the entire `7C07` payload has that production.
    pub reference_signature: Option<CatiaReferenceSignature>,
    /// Exclusive suffix occupant.
    pub suffix: Option<CatiaEntityRecordSuffix>,
    /// Fixed-width suffix selector resolved through the containing graph's catalog.
    pub suffix_schema_selection: Option<CatiaEntitySuffixSchemaSelection>,
}

impl CatiaEntityRecordBody {
    #[cfg(test)]
    pub fn empty_nested() -> Self {
        Self::Nested {
            definition_len: 0,
            definition_prefix: Vec::new(),
            definition_suffix: Vec::new(),
            value_len: 0,
            value_payload: Vec::new(),
            record_suffix: Vec::new(),
        }
    }
}

impl CatiaEntityRecord {
    pub fn value_packets(&self) -> Vec<entity_table::EntityValuePacket> {
        entity_table::value_packets(self.value_payload(), &self.value_fields())
    }

    pub fn numeric_pair(&self) -> Option<entity_table::NumericPair> {
        entity_table::parse_numeric_pair(self.value_payload())
    }

    pub fn relation_expression(&self) -> Option<&CatiaRelationExpression> {
        match &self.value_production {
            Some(CatiaEntityValueProduction::RelationExpression(value)) => Some(value),
            _ => None,
        }
    }

    #[cfg(test)]
    pub fn relation_expression_mut(&mut self) -> Option<&mut CatiaRelationExpression> {
        match &mut self.value_production {
            Some(CatiaEntityValueProduction::RelationExpression(value)) => Some(value),
            _ => None,
        }
    }

    pub fn parameter_value(&self) -> Option<&CatiaParameterValue> {
        match &self.value_production {
            Some(CatiaEntityValueProduction::ParameterValue(value)) => Some(value),
            _ => None,
        }
    }

    #[cfg(test)]
    pub fn parameter_value_mut(&mut self) -> Option<&mut CatiaParameterValue> {
        match &mut self.value_production {
            Some(CatiaEntityValueProduction::ParameterValue(value)) => Some(value),
            _ => None,
        }
    }

    pub fn constraint_range(&self) -> Option<&CatiaConstraintRange> {
        match &self.value_production {
            Some(CatiaEntityValueProduction::ConstraintRange(value)) => Some(value),
            _ => None,
        }
    }

    #[cfg(test)]
    pub fn constraint_range_mut(&mut self) -> Option<&mut CatiaConstraintRange> {
        match &mut self.value_production {
            Some(CatiaEntityValueProduction::ConstraintRange(value)) => Some(value),
            _ => None,
        }
    }

    pub fn definition_value(&self) -> Option<&CatiaDefinitionValue> {
        match &self.value_production {
            Some(CatiaEntityValueProduction::DefinitionValue(value)) => Some(value),
            _ => None,
        }
    }

    #[cfg(test)]
    pub fn definition_value_mut(&mut self) -> Option<&mut CatiaDefinitionValue> {
        match &mut self.value_production {
            Some(CatiaEntityValueProduction::DefinitionValue(value)) => Some(value),
            _ => None,
        }
    }

    pub fn definition_chain_value(&self) -> Option<&CatiaDefinitionChainValue> {
        match &self.value_production {
            Some(CatiaEntityValueProduction::DefinitionChainValue(value)) => Some(value),
            _ => None,
        }
    }

    #[cfg(test)]
    pub fn definition_chain_value_mut(&mut self) -> Option<&mut CatiaDefinitionChainValue> {
        match &mut self.value_production {
            Some(CatiaEntityValueProduction::DefinitionChainValue(value)) => Some(value),
            _ => None,
        }
    }

    pub fn relation_program_instance(&self) -> Option<&CatiaRelationProgramInstance> {
        match &self.object_production {
            Some(CatiaEntityObjectProduction::RelationProgramInstance(value)) => Some(value),
            _ => None,
        }
    }

    #[cfg(test)]
    pub fn relation_program_instance_mut(&mut self) -> Option<&mut CatiaRelationProgramInstance> {
        match &mut self.object_production {
            Some(CatiaEntityObjectProduction::RelationProgramInstance(value)) => Some(value),
            _ => None,
        }
    }

    pub fn schema_configuration_record(&self) -> Option<&CatiaSchemaConfigurationRecord> {
        match &self.object_production {
            Some(CatiaEntityObjectProduction::SchemaConfigurationRecord(value)) => Some(value),
            _ => None,
        }
    }

    #[cfg(test)]
    pub fn schema_configuration_record_mut(
        &mut self,
    ) -> Option<&mut CatiaSchemaConfigurationRecord> {
        match &mut self.object_production {
            Some(CatiaEntityObjectProduction::SchemaConfigurationRecord(value)) => Some(value),
            _ => None,
        }
    }

    pub fn schema_configuration_row_link(&self) -> Option<&CatiaSchemaConfigurationRowLink> {
        match &self.object_production {
            Some(CatiaEntityObjectProduction::SchemaConfigurationRowLink(value)) => Some(value),
            _ => None,
        }
    }

    #[cfg(test)]
    pub fn schema_configuration_row_link_mut(
        &mut self,
    ) -> Option<&mut CatiaSchemaConfigurationRowLink> {
        match &mut self.object_production {
            Some(CatiaEntityObjectProduction::SchemaConfigurationRowLink(value)) => Some(value),
            _ => None,
        }
    }

    pub fn formula_relation(&self) -> Option<&CatiaFormulaRelation> {
        match &self.object_production {
            Some(CatiaEntityObjectProduction::FormulaRelation(value)) => Some(value),
            _ => None,
        }
    }

    #[cfg(test)]
    pub fn formula_relation_mut(&mut self) -> Option<&mut CatiaFormulaRelation> {
        match &mut self.object_production {
            Some(CatiaEntityObjectProduction::FormulaRelation(value)) => Some(value),
            _ => None,
        }
    }

    #[cfg(test)]
    pub fn inline_body(&self) -> Option<&[u8]> {
        match &self.body {
            CatiaEntityRecordBody::Inline(bytes) => Some(bytes),
            CatiaEntityRecordBody::Nested { .. } => None,
        }
    }

    #[cfg(test)]
    pub fn definition_len(&self) -> u32 {
        match self.body {
            CatiaEntityRecordBody::Inline(_) => 0,
            CatiaEntityRecordBody::Nested { definition_len, .. } => definition_len,
        }
    }

    pub fn definition_prefix(&self) -> &[u8] {
        match &self.body {
            CatiaEntityRecordBody::Inline(_) => &[],
            CatiaEntityRecordBody::Nested {
                definition_prefix, ..
            } => definition_prefix,
        }
    }

    #[cfg(test)]
    pub fn definition_suffix(&self) -> &[u8] {
        match &self.body {
            CatiaEntityRecordBody::Inline(_) => &[],
            CatiaEntityRecordBody::Nested {
                definition_suffix, ..
            } => definition_suffix,
        }
    }

    #[cfg(test)]
    pub fn value_len(&self) -> u32 {
        match self.body {
            CatiaEntityRecordBody::Inline(_) => 0,
            CatiaEntityRecordBody::Nested { value_len, .. } => value_len,
        }
    }

    pub fn value_payload(&self) -> &[u8] {
        match &self.body {
            CatiaEntityRecordBody::Inline(_) => &[],
            CatiaEntityRecordBody::Nested { value_payload, .. } => value_payload,
        }
    }

    pub fn value_fields(&self) -> Vec<value_block::ValueField> {
        value_block::tokenize(self.value_payload())
    }

    pub fn record_suffix(&self) -> &[u8] {
        match &self.body {
            CatiaEntityRecordBody::Inline(_) => &[],
            CatiaEntityRecordBody::Nested { record_suffix, .. } => record_suffix,
        }
    }

    pub fn suffix_value(&self) -> Option<&CatiaEntitySuffixValue> {
        match &self.suffix {
            Some(CatiaEntityRecordSuffix::Value(value)) => Some(value),
            _ => None,
        }
    }

    #[cfg(test)]
    pub fn suffix_value_mut(&mut self) -> Option<&mut CatiaEntitySuffixValue> {
        match &mut self.suffix {
            Some(CatiaEntityRecordSuffix::Value(value)) => Some(value),
            _ => None,
        }
    }

    pub fn suffix_framing(&self) -> Option<&CatiaEntitySuffixFraming> {
        match &self.suffix {
            Some(CatiaEntityRecordSuffix::Framing(framing)) => Some(framing),
            _ => None,
        }
    }

    pub fn set_suffix_from_bytes(&mut self, suffix: &[u8]) {
        self.suffix = match (entity_suffix_value(suffix), entity_suffix_framing(suffix)) {
            (Some(value), _) => Some(CatiaEntityRecordSuffix::Value(value)),
            (None, Some(framing)) => Some(CatiaEntityRecordSuffix::Framing(framing)),
            (None, None) => None,
        };
    }
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub(super) struct CatiaEntityRecordWire {
    id: String,
    object_graph: String,
    object_record: String,
    ordinal: u64,
    byte_offset: u64,
    byte_len: u64,
    lead: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schema", schemars(with = "Option<String>"))]
    inline_body: Option<Vec<u8>>,
    definition_len: u32,
    #[serde(with = "cadmpeg_ir::bytes")]
    #[cfg_attr(feature = "schema", schemars(with = "String"))]
    definition_prefix: Vec<u8>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    definition_schema_selections: Vec<CatiaDefinitionSchemaSelection>,
    entity_id: u32,
    #[serde(with = "cadmpeg_ir::bytes")]
    #[cfg_attr(feature = "schema", schemars(with = "String"))]
    definition_suffix: Vec<u8>,
    value_len: u32,
    #[serde(with = "cadmpeg_ir::bytes")]
    #[cfg_attr(feature = "schema", schemars(with = "String"))]
    value_payload: Vec<u8>,
    #[serde(default)]
    value_fields: Vec<value_block::ValueField>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    value_schema_selections: Vec<CatiaEntityValueSchemaSelection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    relation_expression: Option<CatiaRelationExpression>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    parameter_value: Option<CatiaParameterValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    range_interval: Option<CatiaRangeInterval>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    constraint_range: Option<CatiaConstraintRange>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    definition_value: Option<CatiaDefinitionValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    definition_chain_value: Option<CatiaDefinitionChainValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    relation_program_instance: Option<CatiaRelationProgramInstance>,
    #[serde(
        default,
        alias = "configuration_record",
        skip_serializing_if = "Option::is_none"
    )]
    schema_configuration_record: Option<CatiaSchemaConfigurationRecord>,
    #[serde(
        default,
        alias = "configuration_row_link",
        skip_serializing_if = "Option::is_none"
    )]
    schema_configuration_row_link: Option<CatiaSchemaConfigurationRowLink>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    formula_relation: Option<CatiaFormulaRelation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    value_packets: Vec<entity_table::EntityValuePacket>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    numeric_pair: Option<entity_table::NumericPair>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    reference_signature: Option<CatiaReferenceSignature>,
    #[serde(with = "cadmpeg_ir::bytes")]
    #[cfg_attr(feature = "schema", schemars(with = "String"))]
    record_suffix: Vec<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    suffix_value: Option<CatiaEntitySuffixValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    suffix_framing: Option<CatiaEntitySuffixFraming>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    suffix_schema_selection: Option<CatiaEntitySuffixSchemaSelection>,
}

impl From<CatiaEntityRecord> for CatiaEntityRecordWire {
    fn from(value: CatiaEntityRecord) -> Self {
        let (
            inline_body,
            definition_len,
            definition_prefix,
            definition_suffix,
            value_len,
            value_payload,
            record_suffix,
        ) = match value.body {
            CatiaEntityRecordBody::Inline(bytes) => (
                Some(bytes),
                0,
                Vec::new(),
                Vec::new(),
                0,
                Vec::new(),
                Vec::new(),
            ),
            CatiaEntityRecordBody::Nested {
                definition_len,
                definition_prefix,
                definition_suffix,
                value_len,
                value_payload,
                record_suffix,
            } => (
                None,
                definition_len,
                definition_prefix,
                definition_suffix,
                value_len,
                value_payload,
                record_suffix,
            ),
        };
        let value_fields = value_block::tokenize(&value_payload);
        let value_packets = entity_table::value_packets(&value_payload, &value_fields);
        let numeric_pair = entity_table::parse_numeric_pair(&value_payload);
        let (suffix_value, suffix_framing) = match value.suffix {
            Some(CatiaEntityRecordSuffix::Value(suffix)) => (Some(suffix), None),
            Some(CatiaEntityRecordSuffix::Framing(framing)) => (None, Some(framing)),
            None => (None, None),
        };
        let (
            relation_program_instance,
            schema_configuration_record,
            schema_configuration_row_link,
            formula_relation,
        ) = match value.object_production {
            Some(CatiaEntityObjectProduction::RelationProgramInstance(value)) => {
                (Some(value), None, None, None)
            }
            Some(CatiaEntityObjectProduction::SchemaConfigurationRecord(value)) => {
                (None, Some(value), None, None)
            }
            Some(CatiaEntityObjectProduction::SchemaConfigurationRowLink(value)) => {
                (None, None, Some(value), None)
            }
            Some(CatiaEntityObjectProduction::FormulaRelation(value)) => {
                (None, None, None, Some(value))
            }
            None => (None, None, None, None),
        };
        let (
            relation_expression,
            parameter_value,
            constraint_range,
            definition_value,
            definition_chain_value,
        ) = match value.value_production {
            Some(CatiaEntityValueProduction::RelationExpression(value)) => {
                (Some(value), None, None, None, None)
            }
            Some(CatiaEntityValueProduction::ParameterValue(value)) => {
                (None, Some(value), None, None, None)
            }
            Some(CatiaEntityValueProduction::ConstraintRange(value)) => {
                (None, None, Some(value), None, None)
            }
            Some(CatiaEntityValueProduction::DefinitionValue(value)) => {
                (None, None, None, Some(value), None)
            }
            Some(CatiaEntityValueProduction::DefinitionChainValue(value)) => {
                (None, None, None, None, Some(value))
            }
            None => (None, None, None, None, None),
        };
        Self {
            id: value.id,
            object_graph: value.object_graph,
            object_record: value.object_record,
            ordinal: value.ordinal,
            byte_offset: value.byte_offset,
            byte_len: value.byte_len,
            lead: value.lead,
            inline_body,
            definition_len,
            definition_prefix,
            definition_schema_selections: value.definition_schema_selections,
            entity_id: value.entity_id,
            definition_suffix,
            value_len,
            value_payload,
            value_fields,
            value_schema_selections: value.value_schema_selections,
            relation_expression,
            parameter_value,
            range_interval: value.range_interval,
            constraint_range,
            definition_value,
            definition_chain_value,
            relation_program_instance,
            schema_configuration_record,
            schema_configuration_row_link,
            formula_relation,
            value_packets,
            numeric_pair,
            reference_signature: value.reference_signature,
            record_suffix,
            suffix_value,
            suffix_framing,
            suffix_schema_selection: value.suffix_schema_selection,
        }
    }
}

impl TryFrom<CatiaEntityRecordWire> for CatiaEntityRecord {
    type Error = String;

    fn try_from(wire: CatiaEntityRecordWire) -> Result<Self, Self::Error> {
        let nested_occupied = wire.definition_len != 0
            || !wire.definition_prefix.is_empty()
            || !wire.definition_suffix.is_empty()
            || wire.value_len != 0
            || !wire.value_payload.is_empty()
            || !wire.record_suffix.is_empty();
        let body = match (wire.inline_body, nested_occupied) {
            (Some(_), true) => {
                return Err(
                    "entity record cannot carry both an inline body and nested frames".to_owned(),
                );
            }
            (Some(bytes), false) => CatiaEntityRecordBody::Inline(bytes),
            (None, _) => CatiaEntityRecordBody::Nested {
                definition_len: wire.definition_len,
                definition_prefix: wire.definition_prefix,
                definition_suffix: wire.definition_suffix,
                value_len: wire.value_len,
                value_payload: wire.value_payload,
                record_suffix: wire.record_suffix,
            },
        };
        let payload = match &body {
            CatiaEntityRecordBody::Inline(_) => &[][..],
            CatiaEntityRecordBody::Nested { value_payload, .. } => value_payload.as_slice(),
        };
        let value_fields = value_block::tokenize(payload);
        if wire.value_fields != value_fields
            || wire.value_packets != entity_table::value_packets(payload, &value_fields)
            || wire.numeric_pair != entity_table::parse_numeric_pair(payload)
        {
            return Err("entity record value views disagree with payload".to_owned());
        }
        let suffix = match (wire.suffix_value, wire.suffix_framing) {
            (Some(_), Some(_)) => {
                return Err("entity record suffix cannot be both a value and a framing".to_owned());
            }
            (Some(value), None) => Some(CatiaEntityRecordSuffix::Value(value)),
            (None, Some(framing)) => Some(CatiaEntityRecordSuffix::Framing(framing)),
            (None, None) => None,
        };
        let object_production = match (
            wire.relation_program_instance,
            wire.schema_configuration_record,
            wire.schema_configuration_row_link,
            wire.formula_relation,
        ) {
            (Some(value), None, None, None) => {
                Some(CatiaEntityObjectProduction::RelationProgramInstance(value))
            }
            (None, Some(value), None, None) => Some(
                CatiaEntityObjectProduction::SchemaConfigurationRecord(value),
            ),
            (None, None, Some(value), None) => Some(
                CatiaEntityObjectProduction::SchemaConfigurationRowLink(value),
            ),
            (None, None, None, Some(value)) => {
                Some(CatiaEntityObjectProduction::FormulaRelation(value))
            }
            (None, None, None, None) => None,
            _ => return Err("entity record has incompatible object payload productions".to_owned()),
        };
        let value_production = match (
            wire.relation_expression,
            wire.parameter_value,
            wire.constraint_range,
            wire.definition_value,
            wire.definition_chain_value,
        ) {
            (Some(value), None, None, None, None) => {
                Some(CatiaEntityValueProduction::RelationExpression(value))
            }
            (None, Some(value), None, None, None) => {
                Some(CatiaEntityValueProduction::ParameterValue(value))
            }
            (None, None, Some(value), None, None) => {
                Some(CatiaEntityValueProduction::ConstraintRange(value))
            }
            (None, None, None, Some(value), None) => {
                Some(CatiaEntityValueProduction::DefinitionValue(value))
            }
            (None, None, None, None, Some(value)) => {
                Some(CatiaEntityValueProduction::DefinitionChainValue(value))
            }
            (None, None, None, None, None) => None,
            _ => return Err("entity record has incompatible value/suffix productions".to_owned()),
        };
        Ok(Self {
            id: wire.id,
            object_graph: wire.object_graph,
            object_record: wire.object_record,
            ordinal: wire.ordinal,
            byte_offset: wire.byte_offset,
            byte_len: wire.byte_len,
            lead: wire.lead,
            body,
            definition_schema_selections: wire.definition_schema_selections,
            entity_id: wire.entity_id,
            value_schema_selections: wire.value_schema_selections,
            object_production,
            value_production,
            range_interval: wire.range_interval,
            reference_signature: wire.reference_signature,
            suffix,
            suffix_schema_selection: wire.suffix_schema_selection,
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::{CatiaEntityReference, CatiaNative};
    use crate::test_support::standard_catpart_with_formula_relation;

    #[test]
    fn wire_rejects_competing_object_productions() {
        let native = CatiaNative::decode(&standard_catpart_with_formula_relation(0x63, false));
        let entity = &native.entity_records[0];
        assert!(entity.formula_relation().is_some());
        let mut wire = serde_json::to_value(entity).expect("serialize formula entity");
        wire["schema_configuration_row_link"] =
            serde_json::to_value(CatiaSchemaConfigurationRowLink {
                class_reference: CatiaEntityReference::Unresolved { entity_id: 1 },
                successor_payload_offset: 0,
                successor: CatiaEntityReference::Unresolved { entity_id: 2 },
            })
            .expect("serialize row-link production");
        let error = serde_json::from_value::<CatiaEntityRecord>(wire)
            .expect_err("formula and row-link productions require distinct object payloads");
        assert!(error
            .to_string()
            .contains("incompatible object payload productions"));
    }

    #[test]
    fn wire_rejects_competing_value_productions() {
        use crate::native::{CatiaEntitySchemaValue, CatiaEntitySuffixPayload};

        let native = CatiaNative::decode(&standard_catpart_with_formula_relation(0x63, false));
        let entity = &native.entity_records[2];
        assert!(entity.parameter_value().is_some());
        let mut wire = serde_json::to_value(entity).expect("serialize parameter entity");
        wire["definition_value"] = serde_json::to_value(CatiaDefinitionValue {
            definition: CatiaEntitySchemaValue {
                offset: 0,
                ordinal: 1,
                entry: "definition-entry".to_owned(),
                value: "definition".to_owned(),
            },
            payload: CatiaEntitySuffixPayload::Atom { value: 1 },
            schema_selection: None,
        })
        .expect("serialize definition production");
        let error = serde_json::from_value::<CatiaEntityRecord>(wire)
            .expect_err("a named parameter and a definition value require distinct value frames");
        assert!(error
            .to_string()
            .contains("incompatible value/suffix productions"));
    }
}
