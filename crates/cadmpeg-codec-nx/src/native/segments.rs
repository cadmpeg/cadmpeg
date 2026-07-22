// SPDX-License-Identifier: Apache-2.0
//! Segment-index, stream-link, and body-lineage extractors and record types.

#[allow(clippy::wildcard_imports)]
use super::*;

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
    /// Zero-based stream ordinal in source-file order.
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
    /// Zero-based stream ordinal in source-file order.
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
/// the retained history area unless a decoded operation writes them.
pub fn terminal_feature_body_indices(
    labels: &[FeatureOperationLabel],
    references: &[FeatureBodyReference],
    booleans: &[FeatureBooleanOperation],
    operands: &[FeatureOperationBodyOperand],
    bindings: &[SegmentBodyBinding],
) -> Option<BTreeSet<u32>> {
    if references.is_empty() && bindings.is_empty() {
        return None;
    }
    let positions = labels
        .iter()
        .enumerate()
        .map(|(position, label)| (label.id.as_str(), position))
        .collect::<BTreeMap<_, _>>();
    let aliases = body_alias_roots(bindings)?;
    let canonical = |identity: u32| aliases.get(&identity).copied().unwrap_or(identity);
    let operation_kinds = labels
        .iter()
        .map(|label| (label.id.as_str(), label.value.as_str()))
        .collect::<BTreeMap<_, _>>();
    let mut last_writers = bindings
        .iter()
        .flat_map(|binding| [binding.body_object_index, binding.body_alias_object_index])
        .map(|identity| (canonical(identity), None))
        .collect::<BTreeMap<u32, Option<usize>>>();
    for reference in references {
        let position = *positions.get(reference.operation_label.as_str())?;
        if operation_kinds.get(reference.operation_label.as_str()) == Some(&"DELETE") {
            continue;
        }
        last_writers.insert(canonical(reference.body_object_index), Some(position));
    }
    let mut consumed = BTreeSet::new();
    for operation in booleans {
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
    for reference in references {
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
        if !matches!(
            operation_kinds.get(operand.operation_label.as_str()),
            Some(&("SEW" | "TRIM BODY"))
        ) {
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
        references
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
pub fn segment_body_lineage_statuses(
    labels: &[FeatureOperationLabel],
    references: &[FeatureBodyReference],
    booleans: &[FeatureBooleanOperation],
    operands: &[FeatureOperationBodyOperand],
    bindings: &[SegmentBodyBinding],
) -> Option<Vec<SegmentBodyLineageStatus>> {
    let terminal = terminal_feature_body_indices(labels, references, booleans, operands, bindings)?;
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
        .map(|(_, section)| {
            let has = |name| {
                section
                    .types
                    .iter()
                    .any(|definition| definition.name == name)
            };
            let role = if has("UGS::FEATURE_RECORD") {
                OmSchemaRole::FeatureHistory
            } else if has("UGS::EXP_expression") {
                OmSchemaRole::Expressions
            } else if has("UGS::Solid::Topol") {
                OmSchemaRole::Model
            } else if has("UGS::OM::SaveAuditTrail") {
                OmSchemaRole::AuditTrail
            } else {
                OmSchemaRole::Other
            };
            (section.offset, role)
        })
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
    let Some((entry, index)) = container.segment_index() else {
        return Vec::new();
    };
    let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
    let entry_start = usize::try_from(entry_offset).expect("in-bounds directory offset");
    let mut links = Vec::new();
    for (row_ordinal, row) in index.rows.into_iter().enumerate() {
        for (slot, relative) in [
            (SegmentIndexSlot::TypeCode, row.type_code),
            (SegmentIndexSlot::SubtypeCode, row.subtype_code),
            (SegmentIndexSlot::Value, row.value),
        ] {
            let relative = relative as usize;
            let Some(wrapper) = container.data.get(entry_start + relative..) else {
                continue;
            };
            let Some(wrapper_word) = cadmpeg_ir::le::u32_at(wrapper, 0) else {
                continue;
            };
            let extension = (wrapper_word & 0x3fff_ffff) as usize;
            let wrapper_byte_len = match wrapper_word & 0xc000_0000 {
                0x8000_0000 => 8usize.checked_add(extension),
                0xc000_0000 => 33usize.checked_add(extension),
                _ => continue,
            };
            let Some(wrapper_byte_len) = wrapper_byte_len else {
                continue;
            };
            let zlib_offset = entry_start + relative + wrapper_byte_len;
            let Some((stream_ordinal, stream)) = streams
                .iter()
                .enumerate()
                .find(|(_, stream)| stream.file_offset == zlib_offset)
            else {
                continue;
            };
            links.push(SegmentStreamLink {
                id: format!("nx:segment-stream-links:link#{}", links.len()),
                row: format!("nx:segment-index:row#{row_ordinal}"),
                slot,
                stream_ordinal: stream_ordinal as u32,
                stream_kind: match stream.kind {
                    StreamKind::Partition => "partition",
                    StreamKind::Deltas => "deltas",
                    StreamKind::Plain => "plain",
                    StreamKind::Preview => "preview",
                }
                .to_string(),
                wrapper_byte_len: wrapper_byte_len as u32,
                source_offset: entry_offset + relative as u64,
            });
        }
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
