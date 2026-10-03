// SPDX-License-Identifier: Apache-2.0
//! Native FSET reference graphs and construction payloads.

use super::payload_content::FeaturePayloadContent;
use super::{
    charged_unique_offset_data_block, format_feature_history_id, offset_data_block_bytes,
    visit_feature_history_operation_records, FeatureConstructionOwner, FeatureConstructionPayload,
};
use crate::container::Container;
use crate::om::fset_references::{word_reference_bytes, FsetReferences};
use serde::ser::SerializeMap;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt::Write;

/// Exact two-group object-reference graph carried by an `FSET` payload.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "FeatureFsetReferenceGraphWire")]
pub(in crate::native) struct FeatureFsetReferenceGraph {
    /// Globally unique graph identity.
    pub(in crate::native) id: String,
    /// Owning `FSET` operation label.
    pub(in crate::native) operation_label: String,
    references: FsetReferences<Option<String>>,
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

impl Serialize for FeatureFsetReferenceGraph {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let first = self.references.first();
        let second = self.references.second();
        let mut wire = serializer.serialize_map(None)?;
        wire.serialize_entry("id", &self.id)?;
        wire.serialize_entry("operation_label", &self.operation_label)?;
        wire.serialize_entry("selector", self.references.selector())?;
        wire.serialize_entry(
            "first_object_indices",
            &first.each_ref().map(|(index, _)| u32::from(*index)),
        )?;
        wire.serialize_entry(
            "raw_first_object_indices",
            &first
                .each_ref()
                .map(|(index, _)| word_reference_bytes(*index)),
        )?;
        wire.serialize_entry(
            "first_data_blocks",
            &first.each_ref().map(|(_, target)| target.as_deref()),
        )?;
        wire.serialize_entry(
            "second_object_indices",
            &second.each_ref().map(|(index, _)| u32::from(*index)),
        )?;
        wire.serialize_entry(
            "raw_second_object_indices",
            &second
                .each_ref()
                .map(|(index, _)| word_reference_bytes(*index)),
        )?;
        wire.serialize_entry(
            "second_data_blocks",
            &second.each_ref().map(|(_, target)| target.as_deref()),
        )?;
        wire.serialize_entry("source_offset", &self.references.offset())?;
        wire.serialize_entry("first_source_offsets", &self.references.first_offsets())?;
        wire.serialize_entry("second_source_offsets", &self.references.second_offsets())?;
        wire.end()
    }
}

#[cfg(test)]
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
pub(in crate::native) enum FeatureFsetReferenceGroup {
    /// Two-reference group inside the byte-counted angle-bracket frame.
    First,
    /// Three-reference group following the angle-bracket frame.
    Second,
}

/// Decode and resolve exact `FSET` reference graphs without assigning semantic
/// roles to either reference group.
pub(in crate::native) fn feature_fset_reference_graphs(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    container: &Container,
) -> Result<Vec<FeatureFsetReferenceGraph>, cadmpeg_core::CodecError> {
    let indexed = container.indexed_om_sections(ctx)?;
    let mut graphs = Vec::new();
    let mut failure = None;
    visit_feature_history_operation_records(
        ctx,
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            if failure.is_some() {
                return;
            }
            let projected =
                (|| -> Result<Option<FeatureFsetReferenceGraph>, cadmpeg_core::CodecError> {
                    let Some(graph) = FsetReferences::read(record.payload_view()) else {
                        return Ok(None);
                    };
                    let Some(references) = graph.resolve(entry_offset, |index| {
                        charged_unique_offset_data_block(ctx, &indexed, u32::from(index))
                    })?
                    else {
                        return Ok(None);
                    };
                    let id = format_feature_history_id(
                        ctx,
                        "fset-reference-graph",
                        section_key,
                        operation_ordinal,
                        None,
                    )?;
                    let operation_label = format_feature_history_id(
                        ctx,
                        "operation-label",
                        section_key,
                        operation_ordinal,
                        None,
                    )?;
                    ctx.reserve_vec(&mut graphs, 1, "NX FSET reference graphs")?;
                    Ok(Some(FeatureFsetReferenceGraph {
                        id,
                        operation_label,
                        references,
                    }))
                })();
            match projected {
                Ok(Some(graph)) => graphs.push(graph),
                Ok(None) => {}
                Err(error) => failure = Some(error),
            }
        },
    )?;
    if let Some(error) = failure {
        return Err(error);
    }
    Ok(graphs)
}

/// Reconstruct the two ordered logical payloads selected by each complete
/// same-store `FSET` reference graph.
pub(in crate::native) fn feature_fset_construction_payloads(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    container: &Container,
    graphs: &[FeatureFsetReferenceGraph],
) -> Result<Vec<FeatureConstructionPayload>, cadmpeg_core::CodecError> {
    let blocks = offset_data_block_bytes(ctx, container)?;
    let mut output = Vec::new();
    for graph in graphs {
        for (group, source_blocks) in [
            (
                FeatureFsetReferenceGroup::First,
                graph.references.first().as_slice(),
            ),
            (
                FeatureFsetReferenceGroup::Second,
                graph.references.second().as_slice(),
            ),
        ] {
            let Some(payload) =
                fset_construction_payload_from_group(ctx, graph, group, source_blocks, &blocks)?
            else {
                continue;
            };
            ctx.reserve_vec(&mut output, 1, "NX FSET construction payloads")?;
            output.push(payload);
        }
    }
    Ok(output)
}

fn fset_construction_payload_from_group(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    graph: &FeatureFsetReferenceGraph,
    group: FeatureFsetReferenceGroup,
    source_blocks: &[(u16, Option<String>)],
    blocks: &BTreeMap<String, (&[u8], u64)>,
) -> Result<Option<FeatureConstructionPayload>, cadmpeg_core::CodecError> {
    if source_blocks.iter().any(|(_, target)| target.is_none()) {
        return Ok(None);
    }

    ctx.charge_collection_items(
        cadmpeg_core::decode::u64_from_index(source_blocks.len()),
        "NX FSET source block references",
    )?;
    let mut source_reservation = ctx.reserve_scoped(0, "NX FSET source block references")?;
    let mut data_blocks = Vec::new();
    source_reservation.with_storage(|| {
        ctx.reserve_capacity(
            &mut data_blocks,
            source_blocks.len(),
            "allocate NX FSET source block references",
        )
    })?;
    for (_, target) in source_blocks {
        let Some(block) = target else {
            return Ok(None);
        };
        let mut id = String::new();
        ctx.try_reserve_retained_text(
            &mut id,
            block.len(),
            "allocate NX FSET source block reference",
        )?;
        id.push_str(block);
        data_blocks.push(id);
    }
    let Some(store) = data_blocks
        .first()
        .and_then(|id| id.rsplit_once(":block#").map(|(store, _)| store))
    else {
        return Ok(None);
    };
    if data_blocks.iter().any(|block| {
        block
            .rsplit_once(":block#")
            .is_none_or(|(prefix, _)| prefix != store)
    }) {
        return Ok(None);
    }
    let Some(content) = FeaturePayloadContent::from_source(ctx, data_blocks, blocks)? else {
        return Ok(None);
    };
    let group_name = match group {
        FeatureFsetReferenceGroup::First => "first",
        FeatureFsetReferenceGroup::Second => "second",
    };
    let Some(operation_key) = graph
        .operation_label
        .strip_prefix("nx:feature-history:operation-label#")
    else {
        return Ok(None);
    };
    let prefix = "nx:feature-history:fset-construction-payload#";
    let id_len = prefix
        .len()
        .checked_add(operation_key.len())
        .and_then(|length| length.checked_add(1 + group_name.len()))
        .ok_or_else(|| ctx.refuse_codec_limit("NX FSET construction identity", 0, 1))?;
    let mut id = ctx.retained_string(id_len, "NX FSET construction identity")?;
    write!(&mut id, "{prefix}{operation_key}-{group_name}")
        .map_err(|_| ctx.refuse_codec_limit("write NX FSET construction identity", 0, 1))?;
    Ok(Some(FeatureConstructionPayload {
        id,
        operation_label: ctx
            .copy_retained_text(&graph.operation_label, "NX FSET construction operation")?,
        owner: FeatureConstructionOwner::Fset {
            reference_graph: ctx.copy_retained_text(&graph.id, "NX FSET construction reference")?,
            group,
        },
        content,
    }))
}

#[cfg(test)]
mod tests {
    use super::{FeatureFsetReferenceGraph, FeatureFsetReferenceGraphWire};
    use cadmpeg_core::CodecError;
    use std::collections::BTreeMap;

    fn fset_construction_fixture() -> (
        FeatureFsetReferenceGraph,
        BTreeMap<String, (&'static [u8], u64)>,
    ) {
        let graph = serde_json::from_str(r#"{"id":"graph","operation_label":"nx:feature-history:operation-label#0-0000000001","selector":"s","first_object_indices":[1,2],"raw_first_object_indices":[[144,0,1],[144,0,2]],"first_data_blocks":["nx:om-data-blocks-0:block#1","nx:om-data-blocks-0:block#2"],"second_object_indices":[3,4,5],"raw_second_object_indices":[[144,0,3],[144,0,4],[144,0,5]],"second_data_blocks":["nx:om-data-blocks-0:block#3","nx:om-data-blocks-0:block#4","nx:om-data-blocks-0:block#5"],"source_offset":10,"first_source_offsets":[14,17],"second_source_offsets":[21,24,27]}"#)
            .expect("complete FSET graph");
        let blocks = (1u32..=5)
            .map(|ordinal| {
                (
                    format!("nx:om-data-blocks-0:block#{ordinal}"),
                    (b"A".as_slice(), u64::from(ordinal)),
                )
            })
            .collect();
        (graph, blocks)
    }

    fn fset_construction_limit_error(
        configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
    ) -> CodecError {
        let (graph, blocks) = fset_construction_fixture();

        crate::test_support::with_decode_context_over(
            &[],
            |policy| {
                configure(policy);
            },
            |ctx| {
                super::fset_construction_payload_from_group(
                    ctx,
                    &graph,
                    super::FeatureFsetReferenceGroup::First,
                    graph.references.first().as_slice(),
                    &blocks,
                )
                .expect_err("FSET construction limit refusal")
            },
        )
    }

    #[test]
    fn fset_construction_refuses_collection_limit() {
        let error = fset_construction_limit_error(|policy| policy.limits.max_collection_items = 0);
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems));
    }

    #[test]
    fn fset_construction_refuses_scoped_limit() {
        let error =
            fset_construction_limit_error(|policy| policy.limits.max_materialized_bytes = 0);
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes));
    }

    #[test]
    fn fset_construction_refuses_retained_limit() {
        let error = fset_construction_limit_error(|policy| policy.limits.max_retained_bytes = 0);
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes));
    }

    #[test]
    fn fset_construction_refuses_work_limit() {
        let error = fset_construction_limit_error(|policy| policy.limits.max_work_units = 0);
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits));
    }

    #[test]
    fn fset_construction_preserves_first_and_second_source_groups() {
        let (graph, blocks) = fset_construction_fixture();

        crate::test_support::with_decode_context(|ctx| {
            for (group, source_blocks, expected_count, first_id) in [
                (
                    super::FeatureFsetReferenceGroup::First,
                    graph.references.first().as_slice(),
                    2,
                    "nx:om-data-blocks-0:block#1",
                ),
                (
                    super::FeatureFsetReferenceGroup::Second,
                    graph.references.second().as_slice(),
                    3,
                    "nx:om-data-blocks-0:block#3",
                ),
            ] {
                let payload = super::fset_construction_payload_from_group(
                    ctx,
                    &graph,
                    group,
                    source_blocks,
                    &blocks,
                )
                .expect("admitted source blocks")
                .expect("complete FSET payload");
                assert_eq!(payload.content.blocks().len(), expected_count);
                assert_eq!(payload.content.blocks()[0].id, first_id);
            }
        });
    }

    #[test]
    fn fset_reference_borrowed_wire_matches_owned_bytes_and_retained_limit() {
        let json = r#"{"id":"nx:feature:fset-reference#0","operation_label":"o","selector":"s","first_object_indices":[1,2],"raw_first_object_indices":[[144,0,1],[144,0,2]],"first_data_blocks":["a",null],"second_object_indices":[3,4,5],"raw_second_object_indices":[[144,0,3],[144,0,4],[144,0,5]],"second_data_blocks":[null,"d","e"],"source_offset":10,"first_source_offsets":[14,17],"second_source_offsets":[21,24,27]}"#;
        let record: FeatureFsetReferenceGraph = serde_json::from_str(json).unwrap();
        assert_eq!(serde_json::to_vec(&record).unwrap(), json.as_bytes());
        assert_eq!(
            serde_json::to_vec(&record).unwrap(),
            serde_json::to_vec(&FeatureFsetReferenceGraphWire::from(record.clone())).unwrap()
        );
        cadmpeg_test_support::native_serialization::assert_native_limit(
            &record,
            serde_json::from_str::<serde_json::Value>(json).unwrap(),
        );
    }

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
