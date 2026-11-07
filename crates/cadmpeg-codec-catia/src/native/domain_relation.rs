// SPDX-License-Identifier: Apache-2.0
//! Source-local scalar parameter relations retained before operand resolution.

use cadmpeg_core::decode::u64_from_index;
use cadmpeg_core::CodecError;
use serde::{Deserialize, Serialize};

use crate::families::consolidated::records::{
    consolidated_edge_definition_data, ConsolidatedEdgeDefinition, ConsolidatedEdgeDefinitionClass,
    ConsolidatedEdgeDefinitionData,
};
use crate::wire::bytes::{allocation_reference, AllocationReferenceEncoding};
use crate::wire::records::{
    records_are_contiguous, ConsolidatedFamily, ConsolidatedFrameFlag, ConsolidatedFrameWidth,
    ConsolidatedRawFrame, ConsolidatedRecord,
};

use super::edge_definition::CatiaConsolidatedEdgeDefinition;

/// A complete scalar relation with its bounded operand namespace and source frame.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "DomainRelationWire")]
pub(crate) struct CatiaConsolidatedDomainRelation {
    pub(super) id: String,
    pub(super) source_index: u64,
    pub(super) source_offset: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    adjacent_uses: Option<[Class25UseFrame; 2]>,
    #[serde(flatten)]
    definition: CatiaConsolidatedEdgeDefinition,
}

#[derive(Deserialize)]
struct DomainRelationWire {
    id: String,
    source_index: u64,
    source_offset: u64,
    #[serde(default, deserialize_with = "deserialize_adjacent_uses")]
    adjacent_uses: Option<[Class25UseFrame; 2]>,
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
        if wire
            .adjacent_uses
            .as_ref()
            .is_some_and(|uses| !valid_class25_use_pair(&wire.definition, wire.source_offset, uses))
        {
            return Err("adjacent class-25 uses do not close over the source map");
        }
        Ok(Self {
            id: wire.id,
            source_index: wire.source_index,
            source_offset: wire.source_offset,
            adjacent_uses: wire.adjacent_uses,
            definition: wire.definition,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Class25UseFrame {
    source_offset: u64,
    frame: ConsolidatedRawFrame<u64>,
}

fn class25_use_values(frame: &ConsolidatedRawFrame<u64>, terminal: u8) -> Option<[u32; 3]> {
    if frame.width() != ConsolidatedFrameWidth::One
        || frame.flag != ConsolidatedFrameFlag::Flag03
        || frame.header_token() != 5
    {
        return None;
    }
    let payload = frame.payload.as_ref();
    if payload.first() != Some(&0x83) {
        return None;
    }
    let mut at = 1;
    let first = allocation_reference(payload, &mut at)?;
    let second = allocation_reference(payload, &mut at)?;
    let third = allocation_reference(payload, &mut at)?;
    if first.encoding != AllocationReferenceEncoding::BackwardDistance
        || second.encoding != AllocationReferenceEncoding::WidthCoded
        || third.encoding != AllocationReferenceEncoding::WidthCoded
        || payload.get(at..) != Some(&[terminal])
    {
        return None;
    }
    Some([first.value, second.value, third.value])
}

fn class25_frame_end(start: u64, frame: &ConsolidatedRawFrame<u64>) -> Option<u64> {
    let payload_len = u8::try_from(frame.payload.len()).ok()?;
    start
        .checked_add(4)?
        .checked_add(u64::from(u8::from(frame.width())))?
        .checked_add(u64::from(payload_len))
}

fn valid_class25_use_pair(
    definition: &CatiaConsolidatedEdgeDefinition,
    source_offset: u64,
    uses: &[Class25UseFrame; 2],
) -> bool {
    let Some(ConsolidatedEdgeDefinitionData::Domain25 { relation }) = definition.data() else {
        return false;
    };
    if relation.source_operand.encoding != AllocationReferenceEncoding::WidthCoded
        || relation.target_operand.encoding != AllocationReferenceEncoding::WidthCoded
        || class25_frame_end(source_offset, definition.frame()) != Some(uses[0].source_offset)
        || class25_frame_end(uses[0].source_offset, &uses[0].frame) != Some(uses[1].source_offset)
    {
        return false;
    }
    let source = relation.source_operand.value;
    let target = relation.target_operand.value;
    [1, 2].into_iter().enumerate().all(|(index, increment)| {
        let Some(source) = source.checked_add(increment) else {
            return false;
        };
        let Some(target) = target.checked_add(increment) else {
            return false;
        };
        class25_use_values(&uses[index].frame, [0x8a, 0x85][index])
            == Some([increment, source, target])
    })
}

fn class25_adjacent_uses(
    bytes: &[u8],
    records: &[ConsolidatedRecord],
    definition: &CatiaConsolidatedEdgeDefinition,
) -> Result<Option<[Class25UseFrame; 2]>, CodecError> {
    let [map, first, second] = records else {
        return Ok(None);
    };
    if !matches!(
        definition.data(),
        Some(ConsolidatedEdgeDefinitionData::Domain25 { .. })
    ) || !records_are_contiguous(records)
        || [first, second]
            .into_iter()
            .any(|record| record.family() != ConsolidatedFamily::B || record.class() != 0x06)
    {
        return Ok(None);
    }
    let mut uses = Vec::new();
    for record in [first, second] {
        let Some(payload) = record.payload() else {
            return Ok(None);
        };
        uses.push(Class25UseFrame {
            source_offset: u64_from_index(record.source_range().start),
            frame: ConsolidatedRawFrame::from_record(record, bytes[payload].to_vec())?.into(),
        });
    }
    let Ok(uses) = <[Class25UseFrame; 2]>::try_from(uses) else {
        return Ok(None);
    };
    Ok(
        valid_class25_use_pair(definition, u64_from_index(map.source_range().start), &uses)
            .then_some(uses),
    )
}

pub(super) fn derive(
    bytes: &[u8],
    records: &[ConsolidatedRecord],
) -> Result<Vec<CatiaConsolidatedDomainRelation>, CodecError> {
    let mut relations = Vec::new();
    for (index, record) in records.iter().enumerate() {
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
        let definition = CatiaConsolidatedEdgeDefinition::from_source(source);
        let adjacent_uses = match records.get(index..).and_then(|tail| tail.get(..3)) {
            Some(window) => class25_adjacent_uses(bytes, window, &definition)?,
            None => None,
        };
        let relation = CatiaConsolidatedDomainRelation {
            id: format!("catia:consolidated:domain-relation#{:00}", relations.len()),
            source_index: u64_from_index(record.source_index()),
            source_offset: u64_from_index(record.source_range().start),
            adjacent_uses,
            definition,
        };
        relations.push(relation);
    }
    Ok(relations)
}

// Each optional key below names itself in whatever it refuses.
cadmpeg_core::named_optional_field!(
    deserialize_adjacent_uses,
    [Class25UseFrame; 2],
    "adjacent_uses"
);

#[cfg(test)]
mod tests;
