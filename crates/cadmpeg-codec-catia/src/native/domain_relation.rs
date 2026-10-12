// SPDX-License-Identifier: Apache-2.0
//! Source-local scalar class-24 relations retained before operand resolution.

use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use serde::{Deserialize, Serialize};

use crate::families::consolidated::records::{
    class24, ConsolidatedEdgeDefinition, ConsolidatedEdgeDefinitionClass,
    ConsolidatedEdgeDefinitionData,
};
use crate::wire::records::{ConsolidatedFamily, ConsolidatedRawFrame, ConsolidatedRecord};

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
        if wire.definition.class() != ConsolidatedEdgeDefinitionClass::Class24
            || !matches!(
                wire.definition.data(),
                Some(ConsolidatedEdgeDefinitionData::Domain24 { .. })
            )
        {
            return Err("native domain relation requires a scalar class-24 definition");
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
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &[ConsolidatedRecord],
) -> Result<Vec<CatiaConsolidatedDomainRelation>, CodecError> {
    let mut relations = Vec::new();
    for record in records {
        if record.family() != ConsolidatedFamily::B || record.class() != 0x24 {
            continue;
        }
        ctx.charge_work(
            u64_from_index(record.source_range().len()),
            "catia_domain_relation_parse",
        )?;
        let Some(payload) = record.payload() else {
            continue;
        };
        if class24::parse(&bytes[payload.clone()]).is_none() {
            continue;
        }
        let source = ConsolidatedEdgeDefinition {
            frame: ConsolidatedRawFrame::from_record(
                record,
                ctx.copy_retained(&bytes[payload], "catia_domain_relation_payload")?,
            )?,
            class: ConsolidatedEdgeDefinitionClass::Class24,
        };
        let relation = CatiaConsolidatedDomainRelation {
            id: ctx.format_retained(
                format_args!("catia:consolidated:domain-relation#{:00}", relations.len()),
                "catia_domain_relation_id",
            )?,
            source_index: u64_from_index(record.source_index()),
            source_offset: u64_from_index(record.source_range().start),
            definition: CatiaConsolidatedEdgeDefinition::from_source(ctx, source)?,
        };
        ctx.push_vec(&mut relations, relation, "catia_domain_relations")?;
    }
    Ok(relations)
}

#[cfg(test)]
mod tests;
