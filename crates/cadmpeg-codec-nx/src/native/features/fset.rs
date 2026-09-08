// SPDX-License-Identifier: Apache-2.0
//! Native FSET reference graphs and construction payloads.

use super::payload_content::FeaturePayloadContent;
use super::{
    offset_data_block_bytes, unique_offset_data_block, visit_feature_history_operation_records,
    FeatureConstructionOwner, FeatureConstructionPayload,
};
use crate::container::Container;
use crate::om::fset_references::{word_reference_bytes, FsetReferences};
use serde::{Deserialize, Serialize};

/// Exact two-group object-reference graph carried by an `FSET` payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "FeatureFsetReferenceGraphWire",
    into = "FeatureFsetReferenceGraphWire"
)]
pub struct FeatureFsetReferenceGraph {
    /// Globally unique graph identity.
    pub id: String,
    /// Owning `FSET` operation label.
    pub operation_label: String,
    pub references: FsetReferences<Option<String>>,
}

#[derive(Serialize, Deserialize)]
struct FeatureFsetReferenceGraphWire {
    /// Globally unique graph identity.
    id: String,
    /// Owning `FSET` operation label.
    operation_label: String,
    /// Exact nonempty printable selector preceding the first group.
    selector: String,
    /// Serialized object indices in the bounded first group.
    first_object_indices: [u32; 2],
    /// Exact three-byte word tokens in the first group.
    raw_first_object_indices: [Vec<u8>; 2],
    /// Unique native data-block targets for the first group.
    first_data_blocks: [Option<String>; 2],
    /// Serialized object indices in the trailing second group.
    second_object_indices: [u32; 3],
    /// Exact three-byte word tokens in the second group.
    raw_second_object_indices: [Vec<u8>; 3],
    /// Unique native data-block targets for the second group.
    second_data_blocks: [Option<String>; 3],
    /// Absolute source offset of the graph's `01` marker.
    source_offset: u64,
    /// Absolute source offsets of the first-group width markers.
    first_source_offsets: [u64; 2],
    /// Absolute source offsets of the second-group width markers.
    second_source_offsets: [u64; 3],
}

impl From<FeatureFsetReferenceGraph> for FeatureFsetReferenceGraphWire {
    fn from(value: FeatureFsetReferenceGraph) -> Self {
        Self {
            id: value.id,
            operation_label: value.operation_label,
            selector: value.references.selector().to_owned(),
            first_object_indices: value
                .references
                .first()
                .each_ref()
                .map(|(index, _)| u32::from(*index)),
            raw_first_object_indices: value
                .references
                .first()
                .each_ref()
                .map(|(index, _)| word_reference_bytes(*index).to_vec()),
            first_data_blocks: value
                .references
                .first()
                .each_ref()
                .map(|(_, target)| target.clone()),
            second_object_indices: value
                .references
                .second()
                .each_ref()
                .map(|(index, _)| u32::from(*index)),
            raw_second_object_indices: value
                .references
                .second()
                .each_ref()
                .map(|(index, _)| word_reference_bytes(*index).to_vec()),
            second_data_blocks: value
                .references
                .second()
                .each_ref()
                .map(|(_, target)| target.clone()),
            source_offset: value.references.offset(),
            first_source_offsets: value.references.first_offsets(),
            second_source_offsets: value.references.second_offsets(),
        }
    }
}

impl TryFrom<FeatureFsetReferenceGraphWire> for FeatureFsetReferenceGraph {
    type Error = String;

    fn try_from(wire: FeatureFsetReferenceGraphWire) -> Result<Self, Self::Error> {
        let read_index = |value, raw: &[u8], field| {
            let index =
                u16::try_from(value).map_err(|_| format!("{field}: index exceeds word range"))?;
            if raw != word_reference_bytes(index) {
                return Err(format!("{field}: requires the matching 90 word token"));
            }
            Ok(index)
        };
        let [first_head, first_tail] = [0, 1].map(|slot| {
            read_index(
                wire.first_object_indices[slot],
                &wire.raw_first_object_indices[slot],
                "first_object_indices/raw_first_object_indices",
            )
            .map(|index| (index, wire.first_data_blocks[slot].clone()))
        });
        let [second_head, second_middle, second_tail] = [0, 1, 2].map(|slot| {
            read_index(
                wire.second_object_indices[slot],
                &wire.raw_second_object_indices[slot],
                "second_object_indices/raw_second_object_indices",
            )
            .map(|index| (index, wire.second_data_blocks[slot].clone()))
        });
        let first = [first_head?, first_tail?];
        let second = [second_head?, second_middle?, second_tail?];
        let references = FsetReferences::new(wire.source_offset, wire.selector, first, second)?;
        if references.first_offsets() != wire.first_source_offsets {
            return Err("first_source_offsets: must follow the selector and FSET framing".into());
        }
        if references.second_offsets() != wire.second_source_offsets {
            return Err("second_source_offsets: must follow the first group and separator".into());
        }
        Ok(Self {
            id: wire.id,
            operation_label: wire.operation_label,
            references,
        })
    }
}

/// Serialized reference group selecting one logical `FSET` construction payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeatureFsetReferenceGroup {
    /// Two-reference group inside the byte-counted angle-bracket frame.
    First,
    /// Three-reference group following the angle-bracket frame.
    Second,
}

/// Decode and resolve exact `FSET` reference graphs without assigning semantic
/// roles to either reference group.
pub fn feature_fset_reference_graphs(container: &Container) -> Vec<FeatureFsetReferenceGraph> {
    let indexed = container.indexed_om_sections();
    let mut graphs = Vec::new();
    visit_feature_history_operation_records(
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            let Some(graph) = FsetReferences::read(record.payload_view()) else {
                return;
            };
            let Ok(references) = graph.resolve(entry_offset, |index| {
                unique_offset_data_block(&indexed, u32::from(index))
            }) else {
                return;
            };
            let operation_label =
                format!("nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}");
            graphs.push(FeatureFsetReferenceGraph {
                id: format!(
                    "nx:feature-history:fset-reference-graph#{section_key}-{operation_ordinal:010}"
                ),
                operation_label,
                references,
            });
        },
    );
    graphs
}

/// Reconstruct the two ordered logical payloads selected by each complete
/// same-store `FSET` reference graph.
pub fn feature_fset_construction_payloads(
    container: &Container,
    graphs: &[FeatureFsetReferenceGraph],
) -> Vec<FeatureConstructionPayload> {
    let blocks = offset_data_block_bytes(container);
    graphs
        .iter()
        .flat_map(|graph| {
            let blocks = &blocks;
            [
                (
                    FeatureFsetReferenceGroup::First,
                    graph.references.first().iter(),
                ),
                (
                    FeatureFsetReferenceGroup::Second,
                    graph.references.second().iter(),
                ),
            ]
            .into_iter()
            .filter_map(move |(group, source_blocks)| {
                let data_blocks = source_blocks
                    .map(|(_, target)| target.clone())
                    .collect::<Option<Vec<_>>>()?;
                let store = data_blocks.first()?.rsplit_once(":block#")?.0;
                if data_blocks.iter().any(|block| {
                    block
                        .rsplit_once(":block#")
                        .is_none_or(|(prefix, _)| prefix != store)
                }) {
                    return None;
                }
                let (_, content) = FeaturePayloadContent::from_source(data_blocks, blocks)?;
                let group_name = match group {
                    FeatureFsetReferenceGroup::First => "first",
                    FeatureFsetReferenceGroup::Second => "second",
                };
                let operation_key = graph
                    .operation_label
                    .strip_prefix("nx:feature-history:operation-label#")?;
                Some(FeatureConstructionPayload {
                    id: format!(
                        "nx:feature-history:fset-construction-payload#{operation_key}-{group_name}"
                    ),
                    operation_label: graph.operation_label.clone(),
                    owner: FeatureConstructionOwner::Fset {
                        reference_graph: graph.id.clone(),
                        group,
                    },
                    content,
                })
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::FeatureFsetReferenceGraph;

    #[test]
    fn fset_wire_requires_fixed_words_and_selector_framed_positions(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let json = r#"{"id":"g","operation_label":"o","selector":"s","first_object_indices":[1,2],"raw_first_object_indices":[[144,0,1],[144,0,2]],"first_data_blocks":["a",null],"second_object_indices":[3,4,5],"raw_second_object_indices":[[144,0,3],[144,0,4],[144,0,5]],"second_data_blocks":[null,"d","e"],"source_offset":10,"first_source_offsets":[14,17],"second_source_offsets":[21,24,27]}"#;
        let graph: FeatureFsetReferenceGraph = serde_json::from_str(json)?;
        assert_eq!(serde_json::to_string(&graph)?, json);
        for (field, value) in [
            ("selector", serde_json::json!("")),
            ("selector", serde_json::json!("a b")),
            ("selector", serde_json::json!(">")),
            ("selector", serde_json::json!("é")),
            ("selector", serde_json::json!("s".repeat(248))),
            (
                "raw_first_object_indices",
                serde_json::json!([[240, 1], [144, 0, 2]]),
            ),
            ("first_object_indices", serde_json::json!([65536, 2])),
            ("first_source_offsets", serde_json::json!([11, 14])),
            ("second_source_offsets", serde_json::json!([20, 23, 26])),
            ("source_offset", serde_json::json!(u64::MAX)),
        ] {
            let mut wire: serde_json::Value = serde_json::from_str(json)?;
            wire[field] = value;
            let error = serde_json::from_value::<FeatureFsetReferenceGraph>(wire)
                .err()
                .ok_or("malformed FSET graph was accepted")?
                .to_string();
            assert!(error.contains(field), "{error}");
        }
        Ok(())
    }
}
