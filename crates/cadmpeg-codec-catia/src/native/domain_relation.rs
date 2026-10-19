// SPDX-License-Identifier: Apache-2.0
//! Source-local scalar parameter relations retained before operand resolution.

use cadmpeg_core::decode::u64_from_index;
use cadmpeg_core::CodecError;
use serde::{Deserialize, Serialize};

use crate::families::consolidated::records::{
    consolidated_edge_definition_data, ConsolidatedEdgeDefinition, ConsolidatedEdgeDefinitionClass,
    ConsolidatedEdgeDefinitionData,
};
use crate::wire::records::{
    ConsolidatedFamily, ConsolidatedFrameFlag, ConsolidatedRawFrame, ConsolidatedRecord,
};

use super::edge_definition::CatiaConsolidatedEdgeDefinition;

/// A complete scalar relation with its bounded operand namespace and source frame.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "DomainRelationWire")]
pub(crate) struct CatiaConsolidatedDomainRelation {
    pub(super) id: String,
    pub(super) source_index: u64,
    pub(super) source_offset: u64,
    #[serde(flatten)]
    definition: CatiaConsolidatedEdgeDefinition,
}

#[derive(Deserialize)]
struct DomainRelationWire {
    id: String,
    source_index: u64,
    source_offset: u64,
    #[serde(flatten)]
    definition: CatiaConsolidatedEdgeDefinition,
}

impl TryFrom<DomainRelationWire> for CatiaConsolidatedDomainRelation {
    type Error = &'static str;

    fn try_from(wire: DomainRelationWire) -> Result<Self, Self::Error> {
        if !matches!(
            (wire.definition.class(), wire.definition.data()),
            (
                ConsolidatedEdgeDefinitionClass::Class24,
                Some(ConsolidatedEdgeDefinitionData::Domain24 { .. })
            ) | (
                ConsolidatedEdgeDefinitionClass::Class25,
                Some(ConsolidatedEdgeDefinitionData::Domain25 { .. })
            )
        ) {
            return Err("native domain relation requires a scalar class-24 or class-25 definition");
        }
        Ok(Self {
            id: wire.id,
            source_index: wire.source_index,
            source_offset: wire.source_offset,
            definition: wire.definition,
        })
    }
}

pub(super) fn derive(
    bytes: &[u8],
    records: &[ConsolidatedRecord],
) -> Result<Vec<CatiaConsolidatedDomainRelation>, CodecError> {
    let mut relations = Vec::new();
    for record in records {
        if record.family() != ConsolidatedFamily::B || !matches!(record.class(), 0x24 | 0x25) {
            continue;
        }
        if record.class() == 0x25 && record.flag() != ConsolidatedFrameFlag::Flag03 {
            continue;
        }
        let Some(payload) = record.payload() else {
            continue;
        };
        if !matches!(
            consolidated_edge_definition_data(record.class(), &bytes[payload.clone()]),
            Some(
                ConsolidatedEdgeDefinitionData::Domain24 { .. }
                    | ConsolidatedEdgeDefinitionData::Domain25 { .. }
            )
        ) {
            continue;
        }
        let source = ConsolidatedEdgeDefinition {
            frame: ConsolidatedRawFrame::from_record(record, bytes[payload].to_vec())?,
            class: ConsolidatedEdgeDefinitionClass::try_from(record.class())
                .map_err(CodecError::malformed)?,
        };
        let relation = CatiaConsolidatedDomainRelation {
            id: format!("catia:consolidated:domain-relation#{:00}", relations.len()),
            source_index: u64_from_index(record.source_index()),
            source_offset: u64_from_index(record.source_range().start),
            definition: CatiaConsolidatedEdgeDefinition::from_source(source),
        };
        relations.push(relation);
    }
    Ok(relations)
}

#[cfg(test)]
mod tests;
