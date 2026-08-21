// SPDX-License-Identifier: Apache-2.0
//! Segment-index, stream-link, and body-lineage extractors and record types.

#[allow(clippy::wildcard_imports)]
use super::*;
use crate::native::features::{
    FeatureBodyDataBlockUse, FeatureBodyReference, FeatureBooleanOperation, FeatureInputBlock,
    FeatureOperationBodyOperand, FeatureOperationLabel,
};
use crate::native::om::{DataBlock, DataBlockRole, OmSchemaRole};

/// Classify the semantic role of one linked OM registry.
///
/// `UGS::OM::SaveAuditTrail` is a common class declaration carried by the
/// specialized model registries. It identifies an audit-only registry only
/// when no specialized marker is present. Multiple specialized markers are
/// unresolved because no role-specific extractor can select one safely.
fn classify_om_schema_role(section: &crate::om::Section<'_>) -> OmSchemaRole {
    let has = |name| {
        section
            .types
            .iter()
            .any(|definition| definition.name == name)
    };
    let specialized_roles = [
        ("UGS::FEATURE_RECORD", OmSchemaRole::FeatureHistory),
        ("UGS::EXP_expression", OmSchemaRole::Expressions),
        ("UGS::Solid::Topol", OmSchemaRole::Model),
    ]
    .into_iter()
    .filter_map(|(name, role)| has(name).then_some(role))
    .collect::<Vec<_>>();
    match specialized_roles.as_slice() {
        [role] => *role,
        [] if has("UGS::OM::SaveAuditTrail") => OmSchemaRole::AuditTrail,
        [] => OmSchemaRole::Other,
        _ => OmSchemaRole::Ambiguous,
    }
}

/// One row retained from the canonical `UG_PART` segment index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SegmentIndexRow {
    /// Globally unique row identity.
    pub id: String,
    /// Zero-based row ordinal.
    pub ordinal: u32,
    /// First little-endian row word.
    pub type_code: u32,
    /// Second little-endian row word.
    pub subtype_code: u32,
    /// Third little-endian row word.
    pub value: u32,
    /// Directory entry containing the index.
    pub source_entry: String,
    /// Absolute file offset of the row.
    pub source_offset: u64,
}

/// Decode the canonical `UG_PART` segment-index rows.
pub fn segment_index_rows(container: &Container) -> Vec<SegmentIndexRow> {
    let Some((entry, index)) = container.segment_index() else {
        return Vec::new();
    };
    let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
    index
        .rows
        .into_iter()
        .enumerate()
        .map(|(ordinal, row)| SegmentIndexRow {
            id: format!("nx:segment-index:row#{ordinal}"),
            ordinal: ordinal as u32,
            type_code: row.type_code,
            subtype_code: row.subtype_code,
            value: row.value,
            source_entry: entry.name.clone(),
            source_offset: entry_offset + (ordinal * 12) as u64,
        })
        .collect()
}

/// Word position within one segment-index row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SegmentIndexSlot {
    /// First row word.
    TypeCode,
    /// Second row word.
    SubtypeCode,
    /// Third row word.
    Value,
}

/// Validated link from a segment-index word to a compressed stream wrapper.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SegmentStreamLink {
    /// Globally unique link identity.
    pub id: String,
    /// Owning segment-index row.
    pub row: String,
    /// Row word containing the wrapper offset.
    pub slot: SegmentIndexSlot,
    /// Zero-based stream ordinal in first segment-wrapper order.
    pub stream_ordinal: u32,
    /// Decoded stream classification.
    pub stream_kind: String,
    /// Bytes from the wrapper start to its zlib header.
    pub wrapper_byte_len: u32,
    /// Absolute file offset of the wrapper.
    pub source_offset: u64,
}

/// Body-image identity carried beside one validated Parasolid stream wrapper.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SegmentBodyBinding {
    /// Globally unique binding identity.
    pub id: String,
    /// Validated stream-wrapper link owning the metadata tuple.
    pub stream_link: String,
    /// Zero-based stream ordinal in first segment-wrapper order.
    pub stream_ordinal: u32,
    /// Partition or plain cached-body stream classification.
    pub stream_kind: String,
    /// Object index used by feature-history body operands.
    pub body_object_index: u32,
    /// Second object index naming the same body image in feature history.
    pub body_alias_object_index: u32,
    /// Serialized role word completing the five-word segment tuple.
    pub stream_role: u32,
    /// Absolute file offset of the object-index word in the segment index.
    pub source_offset: u64,
}

/// Return the one segment body binding named by an object index.
///
/// A primary-body or operand relation is valid only when the index matches
/// exactly one alias pair. Zero matches and alias collisions are unresolved.
pub(crate) fn unique_segment_body_binding(
    object_index: u32,
    bindings: &[SegmentBodyBinding],
) -> Option<&SegmentBodyBinding> {
    let mut matches = bindings.iter().filter(|binding| {
        binding.body_object_index == object_index || binding.body_alias_object_index == object_index
    });
    let binding = matches.next()?;
    matches.next().is_none().then_some(binding)
}

/// Return one segment body binding whose alias identity is unique.
///
/// The alias lane is a distinct identity from the stream body's primary
/// object index. Callers use this function only when another native field
/// carries the alias identity and must not silently accept a primary-index
/// collision from a different body.
pub(crate) fn unique_segment_body_alias_binding(
    object_index: u32,
    bindings: &[SegmentBodyBinding],
) -> Option<&SegmentBodyBinding> {
    if bindings
        .iter()
        .any(|binding| binding.body_object_index == object_index)
    {
        return None;
    }
    let mut matches = bindings
        .iter()
        .filter(|binding| binding.body_alias_object_index == object_index);
    let binding = matches.next()?;
    matches.next().is_none().then_some(binding)
}

/// Unambiguous terminal status of one segment-bound body image.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SegmentBodyLineageStatus {
    /// Globally unique status identity.
    pub id: String,
    /// Segment binding whose alias pair names the body image.
    pub segment_body_binding: String,
    /// First serialized body identity.
    pub body_object_index: u32,
    /// Alias identity naming the same body image.
    pub body_alias_object_index: u32,
    /// Whether the image remains terminal after retained history.
    pub terminal: bool,
    /// Absolute source offset of the segment binding.
    pub source_offset: u64,
}

/// Validated link from a segment-index word to a framed OM section.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SegmentOmLink {
    /// Globally unique link identity.
    pub id: String,
    /// Owning segment-index row.
    pub row: String,
    /// Row word containing the section offset.
    pub slot: SegmentIndexSlot,
    /// Role established by exact class declarations in the pointed registry.
    pub schema_role: OmSchemaRole,
    /// Bytes from the pointed offset to the OM section signature.
    pub separator_byte_len: u32,
    /// Absolute file offset of the pointed location.
    pub source_offset: u64,
    /// Absolute file offset of the `ff ff ff ff` OM signature.
    pub section_offset: u64,
}

/// Return body objects whose latest decoded writer is not consumed by a later
/// Boolean, sewing, or trimming operation. Segment-bound bodies exist before
/// the retained history area unless a decoded operation writes them. Primary
/// references from operations with resolved offset-store inputs do not
/// participate in object-identity lineage, including missing or ambiguous
/// body ordinals or duplicate primary-body fields. The label arena is
/// source/newest-first; all history positions below use oldest-first order
/// within each section.
#[allow(clippy::too_many_arguments)]
pub fn terminal_feature_body_indices(
    labels: &[FeatureOperationLabel],
    references: &[FeatureBodyReference],
    data_block_uses: &[FeatureBodyDataBlockUse],
    data_blocks: &[DataBlock],
    booleans: &[FeatureBooleanOperation],
    operands: &[FeatureOperationBodyOperand],
    bindings: &[SegmentBodyBinding],
    inputs: &[FeatureInputBlock],
) -> Option<BTreeSet<u32>> {
    let offset_store_references = data_block_uses
        .iter()
        .map(|use_| use_.feature_body_reference.as_str())
        .collect::<BTreeSet<_>>();
    let offset_store_operations =
        crate::native::features::feature_input_store_operations(inputs, data_blocks);
    let unique_references = crate::native::features::unique_feature_body_references(references);
    let object_references = unique_references
        .into_iter()
        .filter(|(_, reference)| {
            !offset_store_references.contains(reference.id.as_str())
                && !offset_store_operations.contains(reference.operation_label.as_str())
        })
        .map(|(_, reference)| reference)
        .collect::<Vec<_>>();
    if object_references.is_empty() && bindings.is_empty() {
        return None;
    }
    let chronological_labels =
        crate::native::features::feature_operation_chronological_labels(labels);
    let positions = chronological_labels
        .iter()
        .enumerate()
        .map(|(position, label)| (label.id.as_str(), position))
        .collect::<BTreeMap<_, _>>();
    let aliases = body_alias_roots(bindings)?;
    let canonical = |identity: u32| aliases.get(&identity).copied().unwrap_or(identity);
    let segment_boolean_operations = segment_boolean_operation_labels(booleans, data_blocks);
    let operation_kinds = chronological_labels
        .iter()
        .map(|label| (label.id.as_str(), label.value.as_str()))
        .collect::<BTreeMap<_, _>>();
    let mut last_writers = bindings
        .iter()
        .flat_map(|binding| [binding.body_object_index, binding.body_alias_object_index])
        .map(|identity| (canonical(identity), None))
        .collect::<BTreeMap<u32, Option<usize>>>();
    {
        let mut record_writer = |body, position| {
            let writer = last_writers.entry(body).or_default();
            if writer.is_none_or(|writer| writer < position) {
                *writer = Some(position);
            }
        };
        for reference in &object_references {
            let position = *positions.get(reference.operation_label.as_str())?;
            if operation_kinds.get(reference.operation_label.as_str()) == Some(&"DELETE") {
                continue;
            }
            record_writer(canonical(reference.body_object_index), position);
        }
        for operation in booleans
            .iter()
            .filter(|operation| segment_boolean_operations.contains(&operation.operation_label))
        {
            let position = *positions.get(operation.operation_label.as_str())?;
            record_writer(canonical(operation.target_object_index), position);
        }
    }
    let mut consumed = BTreeSet::new();
    for operation in booleans
        .iter()
        .filter(|operation| segment_boolean_operations.contains(&operation.operation_label))
    {
        let position = *positions.get(operation.operation_label.as_str())?;
        for tool in &operation.tool_object_indices {
            let tool = canonical(*tool);
            if last_writers
                .get(&tool)
                .is_some_and(|writer| writer.is_none_or(|writer| writer < position))
            {
                consumed.insert(tool);
            }
        }
    }
    for reference in &object_references {
        if operation_kinds.get(reference.operation_label.as_str()) == Some(&"DELETE") {
            let position = *positions.get(reference.operation_label.as_str())?;
            let body = canonical(reference.body_object_index);
            if last_writers
                .get(&body)
                .is_some_and(|writer| writer.is_none_or(|writer| writer < position))
            {
                consumed.insert(body);
            }
        }
    }
    for operand in operands {
        // Offset-store operands use the operation-body namespace, even when
        // their serialized index happens to equal a segment-body identity.
        // Only the resolved segment-binding lane can consume a segment image.
        if operand.segment_body_bindings.is_empty()
            || !matches!(
                operation_kinds.get(operand.operation_label.as_str()),
                Some(&("SEW" | "TRIM BODY"))
            )
        {
            continue;
        }
        let position = *positions.get(operand.operation_label.as_str())?;
        let body = canonical(operand.operand_object_index);
        if last_writers
            .get(&body)
            .is_some_and(|writer| writer.is_none_or(|writer| writer < position))
        {
            consumed.insert(body);
        }
    }
    let terminal_roots = last_writers
        .into_keys()
        .filter(|body| !consumed.contains(body))
        .collect::<BTreeSet<_>>();
    Some(
        object_references
            .iter()
            .map(|reference| reference.body_object_index)
            .chain(
                bindings.iter().flat_map(|binding| {
                    [binding.body_object_index, binding.body_alias_object_index]
                }),
            )
            .filter(|identity| terminal_roots.contains(&canonical(*identity)))
            .collect(),
    )
}

/// Resolve one atomic terminal status for every segment-bound body image.
#[allow(clippy::too_many_arguments)]
pub fn segment_body_lineage_statuses(
    labels: &[FeatureOperationLabel],
    references: &[FeatureBodyReference],
    data_block_uses: &[FeatureBodyDataBlockUse],
    data_blocks: &[DataBlock],
    booleans: &[FeatureBooleanOperation],
    operands: &[FeatureOperationBodyOperand],
    bindings: &[SegmentBodyBinding],
    inputs: &[FeatureInputBlock],
) -> Option<Vec<SegmentBodyLineageStatus>> {
    let terminal = terminal_feature_body_indices(
        labels,
        references,
        data_block_uses,
        data_blocks,
        booleans,
        operands,
        bindings,
        inputs,
    )?;
    bindings
        .iter()
        .map(|binding| {
            let statuses = [binding.body_object_index, binding.body_alias_object_index]
                .map(|identity| terminal.contains(&identity));
            if statuses[0] != statuses[1] {
                return None;
            }
            let key = binding
                .id
                .rsplit_once('#')
                .map_or(binding.id.as_str(), |(_, key)| key);
            Some(SegmentBodyLineageStatus {
                id: format!("nx:segment-body-lineage:status#{key}"),
                segment_body_binding: binding.id.clone(),
                body_object_index: binding.body_object_index,
                body_alias_object_index: binding.body_alias_object_index,
                terminal: statuses[0],
                source_offset: binding.source_offset,
            })
        })
        .collect()
}

/// Namespace proof for one Boolean's target and ordered tool participants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BooleanOffsetStoreResolution {
    /// No participant ordinal occurs in an offset-only data block.
    None,
    /// Every participant resolves to one block in one offset store.
    Complete(BTreeMap<u32, String>),
    /// At least one participant has offset-store evidence, but the complete
    /// one-store relation is not proven.
    Unresolved,
}

/// Classify one Boolean participant set before applying any integer identity.
/// A partial, duplicate, or cross-store offset-store relation is unresolved;
/// it must not fall back to a segment-body alias with the same integer.
pub(crate) fn boolean_offset_store_resolution(
    operation: &FeatureBooleanOperation,
    data_blocks: &[DataBlock],
) -> BooleanOffsetStoreResolution {
    let participants = std::iter::once(operation.target_object_index)
        .chain(operation.tool_object_indices.iter().copied())
        .collect::<Vec<_>>();
    let mut blocks_by_ordinal = BTreeMap::<u32, Vec<&DataBlock>>::new();
    for block in data_blocks {
        if block.role != DataBlockRole::Column {
            continue;
        }
        blocks_by_ordinal
            .entry(block.block_ordinal)
            .or_default()
            .push(block);
    }
    let mut has_offset_store_evidence = false;
    let mut resolved = Vec::with_capacity(participants.len());
    for object_index in &participants {
        let Some(matches) = blocks_by_ordinal.get(object_index) else {
            continue;
        };
        has_offset_store_evidence = true;
        let [block] = matches.as_slice() else {
            return BooleanOffsetStoreResolution::Unresolved;
        };
        resolved.push((*object_index, *block));
    }
    if !has_offset_store_evidence {
        return BooleanOffsetStoreResolution::None;
    }
    if resolved.len() != participants.len() {
        return BooleanOffsetStoreResolution::Unresolved;
    }
    let Some(section_ordinal) = resolved.first().map(|(_, block)| block.section_ordinal) else {
        return BooleanOffsetStoreResolution::Unresolved;
    };
    if resolved
        .iter()
        .any(|(_, block)| block.section_ordinal != section_ordinal)
    {
        return BooleanOffsetStoreResolution::Unresolved;
    }
    BooleanOffsetStoreResolution::Complete(
        resolved
            .into_iter()
            .map(|(object_index, block)| (object_index, block.id.clone()))
            .collect(),
    )
}

/// Return Boolean operations that are safe to treat as segment-object
/// lineage. Complete offset-store selections and unresolved offset-store
/// candidates have no segment-body effect; integer equality across the two
/// namespaces is not an identity proof. Only operations with no offset-store
/// participant evidence retain the native Boolean lineage rules.
fn segment_boolean_operation_labels(
    booleans: &[FeatureBooleanOperation],
    data_blocks: &[DataBlock],
) -> BTreeSet<String> {
    booleans
        .iter()
        .filter(|operation| {
            matches!(
                boolean_offset_store_resolution(operation, data_blocks),
                BooleanOffsetStoreResolution::None
            )
        })
        .map(|operation| operation.operation_label.clone())
        .collect()
}

/// Map each segment body identity to the smallest identity in its transitive alias component.
pub(crate) fn body_alias_roots(bindings: &[SegmentBodyBinding]) -> Option<BTreeMap<u32, u32>> {
    let mut adjacency = BTreeMap::<u32, BTreeSet<u32>>::new();
    for binding in bindings {
        adjacency
            .entry(binding.body_object_index)
            .or_default()
            .insert(binding.body_alias_object_index);
        adjacency
            .entry(binding.body_alias_object_index)
            .or_default()
            .insert(binding.body_object_index);
    }
    let mut roots = BTreeMap::new();
    for identity in adjacency.keys().copied() {
        if roots.contains_key(&identity) {
            continue;
        }
        let mut component = BTreeSet::new();
        let mut pending = vec![identity];
        while let Some(member) = pending.pop() {
            if !component.insert(member) {
                continue;
            }
            pending.extend(
                adjacency
                    .get(&member)
                    .into_iter()
                    .flatten()
                    .filter(|neighbor| !component.contains(neighbor))
                    .copied(),
            );
        }
        let root = *component.first()?;
        roots.extend(component.into_iter().map(|member| (member, root)));
    }
    Some(roots)
}

/// Resolve segment-index words that point to validated framed OM sections.
pub fn segment_om_links(container: &Container) -> Vec<SegmentOmLink> {
    let Some((entry, index)) = container.segment_index() else {
        return Vec::new();
    };
    let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
    let entry_start = usize::try_from(entry_offset).expect("in-bounds directory offset");
    let sections = container
        .om_sections()
        .into_iter()
        .filter(|(candidate, _)| candidate.name == entry.name)
        .map(|(_, section)| (section.offset, classify_om_schema_role(&section)))
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut links = Vec::new();
    for (row_ordinal, row) in index.rows.into_iter().enumerate() {
        for (slot, relative) in [
            (SegmentIndexSlot::TypeCode, row.type_code),
            (SegmentIndexSlot::SubtypeCode, row.subtype_code),
            (SegmentIndexSlot::Value, row.value),
        ] {
            let relative = relative as usize;
            let (separator_byte_len, schema_role) = if let Some(role) = sections.get(&relative) {
                (0usize, *role)
            } else if container
                .data
                .get(entry_start + relative..entry_start + relative + 4)
                == Some(&[0xc0, 0xd1, 0xf1, 0xed])
            {
                let Some(role) = sections.get(&(relative + 4)) else {
                    continue;
                };
                (4, *role)
            } else {
                continue;
            };
            links.push(SegmentOmLink {
                id: format!("nx:segment-om-links:link#{}", links.len()),
                row: format!("nx:segment-index:row#{row_ordinal}"),
                slot,
                schema_role,
                separator_byte_len: separator_byte_len as u32,
                source_offset: entry_offset + relative as u64,
                section_offset: entry_offset + relative as u64 + separator_byte_len as u64,
            });
        }
    }
    links
}

/// Resolve segment-index words that point to validated compressed wrappers.
pub fn segment_stream_links(container: &Container, streams: &[Stream]) -> Vec<SegmentStreamLink> {
    let mut links = Vec::new();
    for wrapper in container.segment_stream_wrappers() {
        let slot = match wrapper.word_ordinal {
            0 => SegmentIndexSlot::TypeCode,
            1 => SegmentIndexSlot::SubtypeCode,
            2 => SegmentIndexSlot::Value,
            _ => continue,
        };
        let Some((stream_ordinal, stream)) = streams
            .iter()
            .enumerate()
            .find(|(_, stream)| stream.file_offset == wrapper.zlib_offset)
        else {
            continue;
        };
        links.push(SegmentStreamLink {
            id: format!("nx:segment-stream-links:link#{}", links.len()),
            row: format!("nx:segment-index:row#{}", wrapper.row_ordinal),
            slot,
            stream_ordinal: stream_ordinal as u32,
            stream_kind: match stream.kind {
                StreamKind::Partition => "partition",
                StreamKind::Deltas => "deltas",
                StreamKind::Plain => "plain",
                StreamKind::Preview => "preview",
            }
            .to_string(),
            wrapper_byte_len: wrapper.wrapper_byte_len as u32,
            source_offset: wrapper.wrapper_offset as u64,
        });
    }
    links
}

/// Bind partition and cached-body streams to feature-history body object indices.
pub fn segment_body_bindings(container: &Container, streams: &[Stream]) -> Vec<SegmentBodyBinding> {
    let Some((entry, index)) = container.segment_index() else {
        return Vec::new();
    };
    let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
    let words = index
        .rows
        .iter()
        .flat_map(|row| [row.type_code, row.subtype_code, row.value])
        .collect::<Vec<_>>();
    segment_stream_links(container, streams)
        .into_iter()
        .filter(|link| matches!(link.stream_kind.as_str(), "partition" | "plain"))
        .filter_map(|link| {
            let row = link.row.rsplit_once('#')?.1.parse::<usize>().ok()?;
            let slot = match link.slot {
                SegmentIndexSlot::TypeCode => 0,
                SegmentIndexSlot::SubtypeCode => 1,
                SegmentIndexSlot::Value => 2,
            };
            let pointer_word = row.checked_mul(3)?.checked_add(slot)?;
            (words.get(pointer_word + 1) == Some(&0)).then_some(())?;
            let body_object_index = *words.get(pointer_word + 2)?;
            let body_alias_object_index = *words.get(pointer_word + 3)?;
            let stream_role = *words.get(pointer_word + 4)?;
            (body_object_index != 0 && body_alias_object_index != 0).then_some(())?;
            Some(SegmentBodyBinding {
                id: format!("nx:segment-body-bindings:binding#{}", link.stream_ordinal),
                stream_link: link.id,
                stream_ordinal: link.stream_ordinal,
                stream_kind: link.stream_kind,
                body_object_index,
                body_alias_object_index,
                stream_role,
                source_offset: entry_offset + ((pointer_word + 2) * 4) as u64,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use cadmpeg_ir::codec::{Codec, DecodeOptions};

    use crate::test_support::*;
    use crate::NxCodec;

    use super::*;

    #[test]
    fn decode_retains_ordered_ug_part_segment_index_rows() {
        let file = prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", segment_index_payload())]);
        let result = NxCodec
            .decode(&mut Cursor::new(file), &DecodeOptions::default())
            .expect("required invariant");
        let namespace = result.ir().native.namespace("nx").expect("NX namespace");
        assert_eq!(namespace.version, 189);
        let rows = namespace
            .arena_as::<super::SegmentIndexRow>("segment_index_rows")
            .expect("required invariant");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].ordinal, 0);
        assert_eq!(rows[1].value, 28);
        assert_eq!(rows[1].source_entry, "/Root/UG_PART/UG_PART");
        assert_eq!(rows[1].source_offset, rows[0].source_offset + 12);
    }

    #[test]
    fn decode_links_segment_index_word_to_validated_stream_wrapper() {
        let file = prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", segment_stream_payload())]);
        let result = NxCodec
            .decode(&mut Cursor::new(file), &DecodeOptions::default())
            .expect("required invariant");
        let links = result
            .ir()
            .native
            .namespace("nx")
            .expect("required invariant")
            .arena_as::<super::SegmentStreamLink>("segment_stream_links")
            .expect("required invariant");
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].row, "nx:segment-index:row#0");
        assert_eq!(links[0].slot, super::SegmentIndexSlot::TypeCode);
        assert_eq!(links[0].stream_ordinal, 0);
        assert_eq!(links[0].stream_kind, "deltas");
        assert_eq!(links[0].wrapper_byte_len, 8);
    }

    #[test]
    fn decode_binds_segment_body_object_index_to_partition_stream() {
        let file = prt_with_named_payloads(&[(
            "/Root/UG_PART/UG_PART",
            segment_body_binding_payload("partition"),
        )]);
        let result = NxCodec
            .decode(&mut Cursor::new(file), &DecodeOptions::default())
            .expect("required invariant");
        let bindings = result
            .ir()
            .native
            .namespace("nx")
            .expect("required invariant")
            .arena_as::<super::SegmentBodyBinding>("segment_body_bindings")
            .expect("required invariant");
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].stream_ordinal, 0);
        assert_eq!(bindings[0].stream_kind, "partition");
        assert_eq!(bindings[0].body_object_index, 94);
        assert_eq!(bindings[0].body_alias_object_index, 150);
        assert_eq!(bindings[0].stream_role, 19);
        assert_eq!(bindings[0].source_offset, 108);
    }

    #[test]
    fn decode_binds_segment_body_object_index_to_plain_cached_body_stream() {
        let file = prt_with_named_payloads(&[(
            "/Root/UG_PART/UG_PART",
            segment_body_binding_payload("plain"),
        )]);
        let result = NxCodec
            .decode(&mut Cursor::new(file), &DecodeOptions::default())
            .expect("required invariant");
        let bindings = result
            .ir()
            .native
            .namespace("nx")
            .expect("required invariant")
            .arena_as::<super::SegmentBodyBinding>("segment_body_bindings")
            .expect("required invariant");
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].stream_ordinal, 0);
        assert_eq!(bindings[0].stream_kind, "plain");
        assert_eq!(bindings[0].body_object_index, 94);
        assert_eq!(bindings[0].body_alias_object_index, 150);
        assert_eq!(bindings[0].stream_role, 19);
    }

    #[test]
    fn decode_links_extended_partition_wrapper_and_body_identity() {
        let file = prt_with_named_payloads(&[(
            "/Root/UG_PART/UG_PART",
            segment_extended_wrapper_payload(),
        )]);
        let result = NxCodec
            .decode(&mut Cursor::new(file), &DecodeOptions::default())
            .expect("required invariant");
        let namespace = result
            .ir()
            .native
            .namespace("nx")
            .expect("required invariant");
        let links = namespace
            .arena_as::<super::SegmentStreamLink>("segment_stream_links")
            .expect("required invariant");
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].wrapper_byte_len, 38);
        let bindings = namespace
            .arena_as::<super::SegmentBodyBinding>("segment_body_bindings")
            .expect("required invariant");
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].body_object_index, 94);
        assert_eq!(bindings[0].body_alias_object_index, 150);
        assert_eq!(bindings[0].stream_role, 19);
    }

    #[test]
    fn decode_links_segment_index_words_to_direct_and_separated_om_sections() {
        for (separated, expected_separator) in [(false, 0), (true, 4)] {
            let file = prt_with_named_payloads(&[(
                "/Root/UG_PART/UG_PART",
                segment_om_payload(separated),
            )]);
            let result = NxCodec
                .decode(&mut Cursor::new(file), &DecodeOptions::default())
                .expect("required invariant");
            let links = result
                .ir()
                .native
                .namespace("nx")
                .expect("required invariant")
                .arena_as::<super::SegmentOmLink>("segment_om_links")
                .expect("required invariant");
            assert_eq!(links.len(), 1);
            assert_eq!(links[0].row, "nx:segment-index:row#0");
            assert_eq!(links[0].slot, super::SegmentIndexSlot::TypeCode);
            assert_eq!(
                links[0].schema_role,
                crate::native::om::OmSchemaRole::FeatureHistory
            );
            assert_eq!(links[0].separator_byte_len, expected_separator);
            assert_eq!(
                links[0].section_offset,
                links[0].source_offset + u64::from(expected_separator)
            );
        }
    }

    #[test]
    fn decode_marks_multi_role_om_registry_ambiguous() {
        let mut section = size_framed_om_section();
        let insertion = section
            .windows(b"m_target".len())
            .position(|window| window == b"m_target")
            .expect("field declaration");
        let role = b"UGS::EXP_expression";
        let mut declaration = Vec::with_capacity(role.len() + 2);
        declaration.push((role.len() + 1) as u8);
        declaration.extend_from_slice(role);
        declaration.push(0xa1);
        section.splice(insertion..insertion, declaration);
        let payload_len = u32::try_from(section.len() - 16).expect("synthetic OM section length");
        section[8..12].copy_from_slice(&payload_len.to_be_bytes());

        let mut payload = Vec::new();
        for word in [32u32, 9, 11, 1, 1, 24] {
            payload.extend_from_slice(&word.to_le_bytes());
        }
        payload.resize(32, 0);
        payload.extend_from_slice(&section);
        let file = prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", payload)]);
        let result = NxCodec
            .decode(&mut Cursor::new(file), &DecodeOptions::default())
            .expect("required invariant");
        let links = result
            .ir()
            .native
            .namespace("nx")
            .expect("NX namespace")
            .arena_as::<super::SegmentOmLink>("segment_om_links")
            .expect("required invariant");
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].schema_role, OmSchemaRole::Ambiguous);
    }

    #[test]
    fn decode_uses_specialized_role_when_registry_also_declares_audit_class() {
        let mut section = size_framed_om_section();
        let insertion = section
            .windows(b"m_target".len())
            .position(|window| window == b"m_target")
            .expect("field declaration");
        let audit = b"UGS::OM::SaveAuditTrail";
        let mut declaration = Vec::with_capacity(audit.len() + 2);
        declaration.push((audit.len() + 1) as u8);
        declaration.extend_from_slice(audit);
        declaration.push(0xa1);
        section.splice(insertion..insertion, declaration);
        let payload_len = u32::try_from(section.len() - 16).expect("synthetic OM section length");
        section[8..12].copy_from_slice(&payload_len.to_be_bytes());

        let mut payload = Vec::new();
        for word in [32u32, 9, 11, 1, 1, 24] {
            payload.extend_from_slice(&word.to_le_bytes());
        }
        payload.resize(32, 0);
        payload.extend_from_slice(&section);
        let file = prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", payload)]);
        let result = NxCodec
            .decode(&mut Cursor::new(file), &DecodeOptions::default())
            .expect("required invariant");
        let links = result
            .ir()
            .native
            .namespace("nx")
            .expect("NX namespace")
            .arena_as::<super::SegmentOmLink>("segment_om_links")
            .expect("required invariant");
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].schema_role, OmSchemaRole::FeatureHistory);
    }

    #[test]
    fn feature_body_lineage_excludes_tools_consumed_after_their_latest_writer() {
        use crate::native::features::{
            FeatureBodyReference, FeatureBooleanKind, FeatureBooleanOperation,
            FeatureOperationLabel,
        };

        let label = |ordinal: u32, value: &str| FeatureOperationLabel {
            id: format!("operation#{ordinal}"),
            section_link: "history#0".to_string(),
            ordinal,
            value: value.to_string(),
            object_indices: [None; 4],
            raw_object_indices: std::array::from_fn(|_| vec![0xff]),
            source_offset: 2 - u64::from(ordinal),
        };
        let labels = [label(2, "UNITE"), label(1, "EXTRUDE"), label(0, "EXTRUDE")];
        let reference = |operation: &str, body_object_index| FeatureBodyReference {
            id: format!("reference#{body_object_index}"),
            operation_label: operation.to_string(),
            body_object_index,
            raw_body_object_index: vec![body_object_index as u8],
            source_offset: 0,
        };
        let references = [reference("operation#0", 10), reference("operation#1", 20)];
        let booleans = [FeatureBooleanOperation {
            id: "boolean#0".to_string(),
            operation_label: "operation#2".to_string(),
            kind: FeatureBooleanKind::Unite,
            target_object_index: 10,
            raw_target_object_index: vec![10],
            target_source_offset: 0,
            tool_object_indices: vec![20],
            raw_tool_object_indices: vec![vec![20]],
            tool_source_offsets: vec![0],
            source_offset: 0,
        }];

        assert_eq!(
            super::terminal_feature_body_indices(
                &labels,
                &references,
                &[],
                &[],
                &booleans,
                &[],
                &[],
                &[],
            ),
            Some([10].into_iter().collect())
        );
    }

    #[test]
    fn later_boolean_target_write_supersedes_earlier_consumption() {
        use super::SegmentBodyBinding;
        use crate::native::features::{
            FeatureBooleanKind, FeatureBooleanOperation, FeatureOperationLabel,
        };

        let label = |ordinal: u32| FeatureOperationLabel {
            id: format!("operation#{ordinal}"),
            section_link: "history#0".to_string(),
            ordinal,
            value: "UNITE".to_string(),
            object_indices: [None; 4],
            raw_object_indices: std::array::from_fn(|_| vec![0xff]),
            source_offset: 1 - u64::from(ordinal),
        };
        let labels = [label(1), label(0)];
        let boolean = |ordinal: usize, target: u32, tools: Vec<u32>| FeatureBooleanOperation {
            id: format!("boolean#{ordinal}"),
            operation_label: format!("operation#{ordinal}"),
            kind: FeatureBooleanKind::Unite,
            target_object_index: target,
            raw_target_object_index: vec![target as u8],
            target_source_offset: ordinal as u64,
            tool_object_indices: tools,
            raw_tool_object_indices: Vec::new(),
            tool_source_offsets: Vec::new(),
            source_offset: ordinal as u64,
        };
        let booleans = [boolean(0, 20, vec![10]), boolean(1, 10, vec![20])];
        let bindings = [
            SegmentBodyBinding {
                id: "binding#0".to_string(),
                stream_link: "stream#0".to_string(),
                stream_ordinal: 0,
                stream_kind: "partition".to_string(),
                body_object_index: 10,
                body_alias_object_index: 11,
                stream_role: 19,
                source_offset: 0,
            },
            SegmentBodyBinding {
                id: "binding#1".to_string(),
                stream_link: "stream#1".to_string(),
                stream_ordinal: 1,
                stream_kind: "partition".to_string(),
                body_object_index: 20,
                body_alias_object_index: 21,
                stream_role: 19,
                source_offset: 1,
            },
        ];

        assert_eq!(
            super::terminal_feature_body_indices(
                &labels,
                &[],
                &[],
                &[],
                &booleans,
                &[],
                &bindings,
                &[],
            ),
            Some([10, 11].into_iter().collect())
        );
    }

    #[test]
    fn latest_writer_is_selected_across_primary_and_boolean_sources() {
        use super::SegmentBodyBinding;
        use crate::native::features::{
            FeatureBodyReference, FeatureBooleanKind, FeatureBooleanOperation,
            FeatureOperationLabel,
        };

        let label = |ordinal: u32, value: &str| FeatureOperationLabel {
            id: format!("operation#{ordinal}"),
            section_link: "history#0".to_string(),
            ordinal,
            value: value.to_string(),
            object_indices: [None; 4],
            raw_object_indices: std::array::from_fn(|_| vec![0xff]),
            source_offset: u64::from(ordinal),
        };
        let labels = [label(2, "EXTRUDE"), label(1, "UNITE"), label(0, "UNITE")];
        let references = [FeatureBodyReference {
            id: "reference#10".to_string(),
            operation_label: "operation#2".to_string(),
            body_object_index: 10,
            raw_body_object_index: vec![10],
            source_offset: 2,
        }];
        let boolean = |ordinal: usize, target: u32, tools: Vec<u32>| FeatureBooleanOperation {
            id: format!("boolean#{ordinal}"),
            operation_label: format!("operation#{ordinal}"),
            kind: FeatureBooleanKind::Unite,
            target_object_index: target,
            raw_target_object_index: vec![target as u8],
            target_source_offset: ordinal as u64,
            tool_object_indices: tools,
            raw_tool_object_indices: Vec::new(),
            tool_source_offsets: Vec::new(),
            source_offset: ordinal as u64,
        };
        let booleans = [boolean(0, 10, Vec::new()), boolean(1, 20, vec![10])];
        let bindings = [
            SegmentBodyBinding {
                id: "binding#0".to_string(),
                stream_link: "stream#0".to_string(),
                stream_ordinal: 0,
                stream_kind: "partition".to_string(),
                body_object_index: 10,
                body_alias_object_index: 11,
                stream_role: 19,
                source_offset: 0,
            },
            SegmentBodyBinding {
                id: "binding#1".to_string(),
                stream_link: "stream#1".to_string(),
                stream_ordinal: 1,
                stream_kind: "partition".to_string(),
                body_object_index: 20,
                body_alias_object_index: 21,
                stream_role: 19,
                source_offset: 1,
            },
        ];

        assert_eq!(
            super::terminal_feature_body_indices(
                &labels,
                &references,
                &[],
                &[],
                &booleans,
                &[],
                &bindings,
                &[],
            ),
            Some([10, 11, 20, 21].into_iter().collect())
        );
    }

    #[test]
    fn feature_body_lineage_consumes_delete_body_references() {
        use super::SegmentBodyBinding;
        use crate::native::features::{FeatureBodyReference, FeatureOperationLabel};

        let labels = [FeatureOperationLabel {
            id: "operation#delete".to_string(),
            section_link: "history#0".to_string(),
            ordinal: 0,
            value: "DELETE".to_string(),
            object_indices: [None; 4],
            raw_object_indices: std::array::from_fn(|_| vec![0xff]),
            source_offset: 0,
        }];
        let references = [FeatureBodyReference {
            id: "reference#10".to_string(),
            operation_label: "operation#delete".to_string(),
            body_object_index: 10,
            raw_body_object_index: vec![10],
            source_offset: 0,
        }];
        let bindings = [SegmentBodyBinding {
            id: "binding#0".to_string(),
            stream_link: "stream#0".to_string(),
            stream_ordinal: 0,
            stream_kind: "partition".to_string(),
            body_object_index: 10,
            body_alias_object_index: 11,
            stream_role: 19,
            source_offset: 0,
        }];

        assert_eq!(
            super::terminal_feature_body_indices(
                &labels,
                &references,
                &[],
                &[],
                &[],
                &[],
                &bindings,
                &[],
            ),
            Some(std::collections::BTreeSet::new())
        );
    }

    #[test]
    fn feature_body_lineage_ignores_ambiguous_primary_body_fields() {
        use super::SegmentBodyBinding;
        use crate::native::features::{FeatureBodyReference, FeatureOperationLabel};

        let labels = [FeatureOperationLabel {
            id: "operation#delete".to_string(),
            section_link: "history#0".to_string(),
            ordinal: 0,
            value: "DELETE".to_string(),
            object_indices: [None; 4],
            raw_object_indices: std::array::from_fn(|_| vec![0xff]),
            source_offset: 0,
        }];
        let references = [
            FeatureBodyReference {
                id: "reference#10".to_string(),
                operation_label: labels[0].id.clone(),
                body_object_index: 10,
                raw_body_object_index: vec![10],
                source_offset: 0,
            },
            FeatureBodyReference {
                id: "reference#20".to_string(),
                operation_label: labels[0].id.clone(),
                body_object_index: 20,
                raw_body_object_index: vec![20],
                source_offset: 1,
            },
        ];
        let bindings = [
            SegmentBodyBinding {
                id: "binding#0".to_string(),
                stream_link: "stream#0".to_string(),
                stream_ordinal: 0,
                stream_kind: "partition".to_string(),
                body_object_index: 10,
                body_alias_object_index: 11,
                stream_role: 19,
                source_offset: 0,
            },
            SegmentBodyBinding {
                id: "binding#1".to_string(),
                stream_link: "stream#1".to_string(),
                stream_ordinal: 1,
                stream_kind: "partition".to_string(),
                body_object_index: 20,
                body_alias_object_index: 21,
                stream_role: 19,
                source_offset: 1,
            },
        ];

        assert_eq!(
            super::terminal_feature_body_indices(
                &labels,
                &references,
                &[],
                &[],
                &[],
                &[],
                &bindings,
                &[],
            ),
            Some([10, 11, 20, 21].into_iter().collect())
        );
    }

    #[test]
    fn delete_only_history_distinguishes_consumed_and_terminal_images() {
        use super::SegmentBodyBinding;
        use crate::native::features::{FeatureBodyReference, FeatureOperationLabel};

        let labels = [FeatureOperationLabel {
            id: "operation#delete".to_string(),
            section_link: "history#0".to_string(),
            ordinal: 0,
            value: "DELETE".to_string(),
            object_indices: [None; 4],
            raw_object_indices: std::array::from_fn(|_| vec![0xff]),
            source_offset: 0,
        }];
        let references = [FeatureBodyReference {
            id: "reference#10".to_string(),
            operation_label: labels[0].id.clone(),
            body_object_index: 10,
            raw_body_object_index: vec![10],
            source_offset: 0,
        }];
        let binding = |ordinal, body_object_index, body_alias_object_index| SegmentBodyBinding {
            id: format!("binding#{ordinal}"),
            stream_link: format!("stream#{ordinal}"),
            stream_ordinal: ordinal,
            stream_kind: "partition".to_string(),
            body_object_index,
            body_alias_object_index,
            stream_role: 19,
            source_offset: u64::from(ordinal),
        };
        let bindings = [binding(0, 10, 11), binding(1, 20, 21)];

        let statuses = super::segment_body_lineage_statuses(
            &labels,
            &references,
            &[],
            &[],
            &[],
            &[],
            &bindings,
            &[],
        )
        .expect("complete delete-only lineage");
        assert_eq!(statuses.len(), 2);
        assert!(!statuses[0].terminal);
        assert!(statuses[1].terminal);
    }

    #[test]
    fn feature_body_lineage_excludes_offset_store_reference_collisions() {
        use super::SegmentBodyBinding;
        use crate::native::features::{
            FeatureBodyDataBlockUse, FeatureBodyReference, FeatureOperationLabel,
        };

        let labels = [FeatureOperationLabel {
            id: "operation#delete".to_string(),
            section_link: "history#0".to_string(),
            ordinal: 0,
            value: "DELETE".to_string(),
            object_indices: [None; 4],
            raw_object_indices: std::array::from_fn(|_| vec![0xff]),
            source_offset: 0,
        }];
        let references = [FeatureBodyReference {
            id: "reference#11".to_string(),
            operation_label: "operation#delete".to_string(),
            body_object_index: 11,
            raw_body_object_index: vec![11],
            source_offset: 0,
        }];
        let data_block_uses = [FeatureBodyDataBlockUse {
            id: "data-block-use#11".to_string(),
            feature_body_reference: references[0].id.clone(),
            data_block: "block#11".to_string(),
        }];
        let bindings = [SegmentBodyBinding {
            id: "binding#0".to_string(),
            stream_link: "stream#0".to_string(),
            stream_ordinal: 0,
            stream_kind: "partition".to_string(),
            body_object_index: 10,
            body_alias_object_index: 11,
            stream_role: 19,
            source_offset: 0,
        }];

        let statuses = super::segment_body_lineage_statuses(
            &labels,
            &references,
            &data_block_uses,
            &[],
            &[],
            &[],
            &bindings,
            &[],
        )
        .expect("segment binding establishes lineage");
        assert_eq!(statuses.len(), 1);
        assert!(statuses[0].terminal);
    }

    #[test]
    fn feature_body_lineage_excludes_missing_offset_store_ordinal_collisions() {
        use super::SegmentBodyBinding;
        use crate::native::features::{
            FeatureBodyReference, FeatureInputBlock, FeatureOperationLabel,
        };
        use crate::native::om::{DataBlock, DataBlockRole};

        let labels = [FeatureOperationLabel {
            id: "operation#delete".to_string(),
            section_link: "history#0".to_string(),
            ordinal: 0,
            value: "DELETE".to_string(),
            object_indices: [None; 4],
            raw_object_indices: std::array::from_fn(|_| vec![0xff]),
            source_offset: 0,
        }];
        let references = [FeatureBodyReference {
            id: "reference#11".to_string(),
            operation_label: "operation#delete".to_string(),
            body_object_index: 11,
            raw_body_object_index: vec![11],
            source_offset: 0,
        }];
        let inputs = [FeatureInputBlock {
            id: "input#3".to_string(),
            operation_label: "operation#delete".to_string(),
            input_slot: 0,
            object_index: 3,
            raw_object_index: vec![3],
            data_block: "block#3".to_string(),
            source_offset: 0,
        }];
        let blocks = [DataBlock {
            id: "block#3".to_string(),
            section_ordinal: 3,
            block_ordinal: 3,
            role: DataBlockRole::Column,
            section_offset: 0,
            byte_len: 0,
            sha256: String::new(),
            source_entry: String::new(),
            source_offset: 0,
        }];
        let bindings = [SegmentBodyBinding {
            id: "binding#0".to_string(),
            stream_link: "stream#0".to_string(),
            stream_ordinal: 0,
            stream_kind: "partition".to_string(),
            body_object_index: 10,
            body_alias_object_index: 11,
            stream_role: 19,
            source_offset: 0,
        }];

        let statuses = super::segment_body_lineage_statuses(
            &labels,
            &references,
            &[],
            &blocks,
            &[],
            &[],
            &bindings,
            &inputs,
        )
        .expect("segment binding establishes lineage");
        assert_eq!(statuses.len(), 1);
        assert!(statuses[0].terminal);
    }

    #[test]
    fn feature_body_lineage_ignores_complete_offset_store_boolean_collisions() {
        use super::SegmentBodyBinding;
        use crate::native::features::{
            FeatureBooleanKind, FeatureBooleanOperation, FeatureOperationLabel,
        };
        use crate::native::om::{DataBlock, DataBlockRole};

        let operation_label = "nx:feature-history:operation-label#section-boolean".to_string();
        let labels = [FeatureOperationLabel {
            id: operation_label.clone(),
            section_link: "history#0".to_string(),
            ordinal: 0,
            value: "UNITE".to_string(),
            object_indices: [None; 4],
            raw_object_indices: std::array::from_fn(|_| vec![0xff]),
            source_offset: 0,
        }];
        let booleans = [FeatureBooleanOperation {
            id: "boolean#offset-store".to_string(),
            operation_label,
            kind: FeatureBooleanKind::Unite,
            target_object_index: 11,
            raw_target_object_index: vec![11],
            target_source_offset: 0,
            tool_object_indices: vec![21],
            raw_tool_object_indices: vec![vec![21]],
            tool_source_offsets: vec![0],
            source_offset: 0,
        }];
        let block = |ordinal| DataBlock {
            id: format!("nx:om-data-blocks-3:block#{ordinal}"),
            section_ordinal: 3,
            block_ordinal: ordinal,
            role: DataBlockRole::Column,
            section_offset: 0,
            byte_len: 0,
            sha256: String::new(),
            source_entry: String::new(),
            source_offset: 0,
        };
        let blocks = [block(11), block(21)];
        let binding = |ordinal, body, alias| SegmentBodyBinding {
            id: format!("binding#{ordinal}"),
            stream_link: format!("stream#{ordinal}"),
            stream_ordinal: ordinal,
            stream_kind: "partition".to_string(),
            body_object_index: body,
            body_alias_object_index: alias,
            stream_role: 19,
            source_offset: u64::from(ordinal),
        };
        let bindings = [binding(0, 10, 11), binding(1, 20, 21)];

        assert_eq!(
            super::terminal_feature_body_indices(
                &labels,
                &[],
                &[],
                &blocks,
                &booleans,
                &[],
                &bindings,
                &[],
            ),
            Some([10, 11, 20, 21].into_iter().collect())
        );
    }

    #[test]
    fn feature_body_lineage_ignores_unresolved_offset_store_boolean_collisions() {
        use super::SegmentBodyBinding;
        use crate::native::features::{
            FeatureBooleanKind, FeatureBooleanOperation, FeatureOperationLabel,
        };
        use crate::native::om::{DataBlock, DataBlockRole};

        let operation_label = "nx:feature-history:operation-label#section-boolean".to_string();
        let labels = [FeatureOperationLabel {
            id: operation_label.clone(),
            section_link: "history#0".to_string(),
            ordinal: 0,
            value: "UNITE".to_string(),
            object_indices: [None; 4],
            raw_object_indices: std::array::from_fn(|_| vec![0xff]),
            source_offset: 0,
        }];
        let booleans = [FeatureBooleanOperation {
            id: "boolean#unresolved-offset-store".to_string(),
            operation_label,
            kind: FeatureBooleanKind::Unite,
            target_object_index: 11,
            raw_target_object_index: vec![11],
            target_source_offset: 0,
            tool_object_indices: vec![21],
            raw_tool_object_indices: vec![vec![21]],
            tool_source_offsets: vec![0],
            source_offset: 0,
        }];
        let blocks = [DataBlock {
            id: "nx:om-data-blocks-3:block#11".to_string(),
            section_ordinal: 3,
            block_ordinal: 11,
            role: DataBlockRole::Column,
            section_offset: 0,
            byte_len: 0,
            sha256: String::new(),
            source_entry: String::new(),
            source_offset: 0,
        }];
        let binding = |ordinal, body, alias| SegmentBodyBinding {
            id: format!("binding#{ordinal}"),
            stream_link: format!("stream#{ordinal}"),
            stream_ordinal: ordinal,
            stream_kind: "partition".to_string(),
            body_object_index: body,
            body_alias_object_index: alias,
            stream_role: 19,
            source_offset: u64::from(ordinal),
        };
        let bindings = [binding(0, 10, 11), binding(1, 20, 21)];

        assert_eq!(
            super::terminal_feature_body_indices(
                &labels,
                &[],
                &[],
                &blocks,
                &booleans,
                &[],
                &bindings,
                &[],
            ),
            Some([10, 11, 20, 21].into_iter().collect())
        );
    }

    #[test]
    fn feature_body_lineage_allows_a_writer_after_delete() {
        use super::SegmentBodyBinding;
        use crate::native::features::{FeatureBodyReference, FeatureOperationLabel};

        let label = |ordinal: u32, value: &str| FeatureOperationLabel {
            id: format!("operation#{ordinal}"),
            section_link: "history#0".to_string(),
            ordinal,
            value: value.to_string(),
            object_indices: [None; 4],
            raw_object_indices: std::array::from_fn(|_| vec![0xff]),
            source_offset: u64::from(ordinal),
        };
        // Feature-history labels are newest-first within one section. The raw
        // order places the later writer before the earlier delete.
        let labels = [label(0, "EXTRUDE"), label(1, "DELETE")];
        let reference = |ordinal: u32| FeatureBodyReference {
            id: format!("reference#{ordinal}"),
            operation_label: format!("operation#{ordinal}"),
            body_object_index: 10,
            raw_body_object_index: vec![10],
            source_offset: u64::from(ordinal),
        };
        let references = [reference(0), reference(1)];
        let bindings = [SegmentBodyBinding {
            id: "binding#0".to_string(),
            stream_link: "stream#0".to_string(),
            stream_ordinal: 0,
            stream_kind: "partition".to_string(),
            body_object_index: 10,
            body_alias_object_index: 11,
            stream_role: 19,
            source_offset: 0,
        }];

        assert_eq!(
            super::terminal_feature_body_indices(
                &labels,
                &references,
                &[],
                &[],
                &[],
                &[],
                &bindings,
                &[],
            ),
            Some([10, 11].into_iter().collect())
        );
    }

    #[test]
    fn feature_body_lineage_consumes_a_writer_before_a_delete_in_raw_order() {
        use super::SegmentBodyBinding;
        use crate::native::features::{FeatureBodyReference, FeatureOperationLabel};

        let label = |ordinal: u32, value: &str| FeatureOperationLabel {
            id: format!("operation#{ordinal}"),
            section_link: "history#0".to_string(),
            ordinal,
            value: value.to_string(),
            object_indices: [None; 4],
            raw_object_indices: std::array::from_fn(|_| vec![0xff]),
            source_offset: u64::from(ordinal),
        };
        let reference = |ordinal: u32| FeatureBodyReference {
            id: format!("reference#{ordinal}"),
            operation_label: format!("operation#{ordinal}"),
            body_object_index: 10,
            raw_body_object_index: vec![10],
            source_offset: u64::from(ordinal),
        };
        // The raw order is newest-first, so this encodes a writer followed by
        // a delete in chronological history.
        let labels = [label(0, "DELETE"), label(1, "EXTRUDE")];
        let references = [reference(0), reference(1)];
        let bindings = [SegmentBodyBinding {
            id: "binding#0".to_string(),
            stream_link: "stream#0".to_string(),
            stream_ordinal: 0,
            stream_kind: "partition".to_string(),
            body_object_index: 10,
            body_alias_object_index: 11,
            stream_role: 19,
            source_offset: 0,
        }];

        assert_eq!(
            super::terminal_feature_body_indices(
                &labels,
                &references,
                &[],
                &[],
                &[],
                &[],
                &bindings,
                &[],
            ),
            Some(std::collections::BTreeSet::new())
        );
    }

    #[test]
    fn feature_body_lineage_continues_across_ordered_history_sections() {
        use crate::native::features::{
            FeatureBodyReference, FeatureBooleanKind, FeatureBooleanOperation,
            FeatureOperationLabel,
        };

        let label = |id: &str, section_link: &str, ordinal, value: &str| FeatureOperationLabel {
            id: id.to_string(),
            section_link: section_link.to_string(),
            ordinal,
            value: value.to_string(),
            object_indices: [None; 4],
            raw_object_indices: std::array::from_fn(|_| vec![0xff]),
            source_offset: u64::from(ordinal),
        };
        let labels = [
            label("operation#early", "history#0", 0, "EXTRUDE"),
            label("operation#late", "history#1", 0, "UNITE"),
        ];
        let references = [FeatureBodyReference {
            id: "reference#20".to_string(),
            operation_label: "operation#early".to_string(),
            body_object_index: 20,
            raw_body_object_index: vec![20],
            source_offset: 0,
        }];
        let booleans = [FeatureBooleanOperation {
            id: "boolean#0".to_string(),
            operation_label: "operation#late".to_string(),
            kind: FeatureBooleanKind::Unite,
            target_object_index: 10,
            raw_target_object_index: vec![10],
            target_source_offset: 1,
            tool_object_indices: vec![20],
            raw_tool_object_indices: vec![vec![20]],
            tool_source_offsets: vec![1],
            source_offset: 1,
        }];

        assert_eq!(
            super::terminal_feature_body_indices(
                &labels,
                &references,
                &[],
                &[],
                &booleans,
                &[],
                &[],
                &[],
            ),
            Some(std::collections::BTreeSet::new())
        );
    }

    #[test]
    fn feature_body_lineage_treats_segment_tuple_indices_as_one_identity() {
        use super::SegmentBodyBinding;
        use crate::native::features::{
            FeatureBodyReference, FeatureBooleanKind, FeatureBooleanOperation,
            FeatureOperationLabel,
        };

        let label = |ordinal: u32, value: &str| FeatureOperationLabel {
            id: format!("operation#{ordinal}"),
            section_link: "history#0".to_string(),
            ordinal,
            value: value.to_string(),
            object_indices: [None; 4],
            raw_object_indices: std::array::from_fn(|_| vec![0xff]),
            source_offset: 1 - u64::from(ordinal),
        };
        let labels = [label(1, "UNITE"), label(0, "EXTRUDE")];
        let references = [FeatureBodyReference {
            id: "reference#150".to_string(),
            operation_label: "operation#0".to_string(),
            body_object_index: 150,
            raw_body_object_index: vec![0x80, 150],
            source_offset: 0,
        }];
        let booleans = [FeatureBooleanOperation {
            id: "boolean#0".to_string(),
            operation_label: "operation#1".to_string(),
            kind: FeatureBooleanKind::Unite,
            target_object_index: 10,
            raw_target_object_index: vec![10],
            target_source_offset: 0,
            tool_object_indices: vec![94],
            raw_tool_object_indices: vec![vec![94]],
            tool_source_offsets: vec![0],
            source_offset: 0,
        }];
        let bindings = [SegmentBodyBinding {
            id: "binding#0".to_string(),
            stream_link: "stream#0".to_string(),
            stream_ordinal: 0,
            stream_kind: "partition".to_string(),
            body_object_index: 94,
            body_alias_object_index: 150,
            stream_role: 19,
            source_offset: 0,
        }];

        assert_eq!(
            super::terminal_feature_body_indices(
                &labels,
                &references,
                &[],
                &[],
                &booleans,
                &[],
                &bindings,
                &[],
            ),
            Some(std::collections::BTreeSet::new())
        );
    }

    #[test]
    fn feature_body_lineage_consumes_segment_bound_sew_operands() {
        use super::SegmentBodyBinding;
        use crate::native::features::{FeatureOperationBodyOperand, FeatureOperationLabel};
        let labels = [FeatureOperationLabel {
            id: "operation#0".to_string(),
            section_link: "history#0".to_string(),
            ordinal: 0,
            value: "SEW".to_string(),
            object_indices: [None; 4],
            raw_object_indices: std::array::from_fn(|_| vec![0xff]),
            source_offset: 0,
        }];
        let bindings = [SegmentBodyBinding {
            id: "binding#0".to_string(),
            stream_link: "stream#0".to_string(),
            stream_ordinal: 0,
            stream_kind: "partition".to_string(),
            body_object_index: 20,
            body_alias_object_index: 30,
            stream_role: 0,
            source_offset: 0,
        }];
        let operands = [FeatureOperationBodyOperand {
            id: "operand#0".to_string(),
            operation_label: "operation#0".to_string(),
            body_object_index: 10,
            body_reference_ordinal: 0,
            ordinal: 0,
            operand_object_index: 30,
            raw_operand_object_index: vec![30],
            operand_data_block: None,
            segment_body_bindings: vec!["binding#0".to_string()],
            source_offset: 0,
        }];
        assert_eq!(
            super::terminal_feature_body_indices(
                &labels,
                &[],
                &[],
                &[],
                &[],
                &operands,
                &bindings,
                &[],
            ),
            Some(std::collections::BTreeSet::new())
        );
    }

    #[test]
    fn feature_body_lineage_ignores_offset_store_operands() {
        use super::SegmentBodyBinding;
        use crate::native::features::{FeatureOperationBodyOperand, FeatureOperationLabel};
        let labels = [FeatureOperationLabel {
            id: "operation#0".to_string(),
            section_link: "history#0".to_string(),
            ordinal: 0,
            value: "TRIM BODY".to_string(),
            object_indices: [None; 4],
            raw_object_indices: std::array::from_fn(|_| vec![0xff]),
            source_offset: 0,
        }];
        let bindings = [SegmentBodyBinding {
            id: "binding#0".to_string(),
            stream_link: "stream#0".to_string(),
            stream_ordinal: 0,
            stream_kind: "partition".to_string(),
            body_object_index: 20,
            body_alias_object_index: 30,
            stream_role: 0,
            source_offset: 0,
        }];
        let operands = [FeatureOperationBodyOperand {
            id: "operand#0".to_string(),
            operation_label: "operation#0".to_string(),
            body_object_index: 10,
            body_reference_ordinal: 0,
            ordinal: 0,
            operand_object_index: 30,
            raw_operand_object_index: vec![30],
            operand_data_block: Some("data-block#0".to_string()),
            segment_body_bindings: Vec::new(),
            source_offset: 0,
        }];
        assert_eq!(
            super::terminal_feature_body_indices(
                &labels,
                &[],
                &[],
                &[],
                &[],
                &operands,
                &bindings,
                &[],
            ),
            Some([20, 30].into_iter().collect())
        );
    }
}
