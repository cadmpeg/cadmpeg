// SPDX-License-Identifier: Apache-2.0
//! Segment-index, stream-link, and body-lineage extractors and record types.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::container::{Container, SegmentStreamWrapper};
use crate::parasolid::{Stream, StreamKind};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use crate::native::features::{
    FeatureBodyDataBlockUse, FeatureBodyReference, FeatureBooleanOperation, FeatureInputBlock,
    FeatureOperationBodyOperand, FeatureOperationLabel,
};
use crate::native::om::{DataBlock, DataBlockRole, OmSchemaRole};
pub(super) mod om_location;
use om_location::OmLocation;
mod row_wire;

/// Classify the semantic role of one linked OM registry.
///
/// `UGS::OM::SaveAuditTrail` is a common class declaration carried by the
/// specialized model registries. It identifies an audit-only registry only
/// when no specialized marker is present. Multiple specialized markers are
/// unresolved because no role-specific extractor can select one safely.
fn classify_om_schema_role(
    ctx: &DecodeContext<'_>,
    section: &crate::om::Section<'_>,
) -> Result<OmSchemaRole, CodecError> {
    let (mut feature_history, mut expressions, mut model, mut audit_trail) =
        (false, false, false, false);
    for definition in ctx.admit_iter(&*section.types, "classify NX OM schema role")? {
        match definition.name {
            "UGS::FEATURE_RECORD" => feature_history = true,
            "UGS::EXP_expression" => expressions = true,
            "UGS::Solid::Topol" => model = true,
            "UGS::OM::SaveAuditTrail" => audit_trail = true,
            _ => {}
        }
    }
    let mut roles = [
        (feature_history, OmSchemaRole::FeatureHistory),
        (expressions, OmSchemaRole::Expressions),
        (model, OmSchemaRole::Model),
    ]
    .into_iter()
    .filter_map(|(present, role)| present.then_some(role));
    Ok(match (roles.next(), roles.next()) {
        (Some(role), None) => role,
        (None, _) if audit_trail => OmSchemaRole::AuditTrail,
        (None, _) => OmSchemaRole::Other,
        _ => OmSchemaRole::Ambiguous,
    })
}

/// One row retained from the canonical `UG_PART` segment index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct SegmentIndexRow {
    /// Globally unique row identity.
    pub(super) id: String,
    /// Zero-based row ordinal.
    ordinal: u32,
    /// First little-endian row word.
    type_code: u32,
    /// Second little-endian row word.
    subtype_code: u32,
    /// Third little-endian row word.
    value: u32,
    /// Directory entry containing the index.
    source_entry: String,
    /// Absolute file offset of the row.
    pub(super) source_offset: u64,
}

/// Decode the canonical `UG_PART` segment-index rows.
pub(super) fn segment_index_rows(
    ctx: &DecodeContext<'_>,
    container: &Container,
) -> Result<Vec<SegmentIndexRow>, CodecError> {
    let Some((entry, index)) = container.segment_index() else {
        return Ok(Vec::new());
    };
    let entry_offset = entry.file_span().map_or(0, |(offset, _)| offset);
    let mut rows = Vec::new();
    for (ordinal, row) in index.rows().enumerate() {
        ctx.charge_work(1, "nx segment index rows")?;
        ctx.charge_entities(1, "nx segment index rows")?;
        ctx.reserve_vec(&mut rows, 1, "nx segment index rows")?;
        let ordinal_u32 = u32::try_from(ordinal).map_err(|_| {
            ctx.refuse_codec_limit(
                "nx segment index ordinal",
                u64::from(u32::MAX),
                cadmpeg_core::decode::u64_from_index(ordinal),
            )
        })?;
        let id = ctx.format_retained(
            format_args!("nx:segment-index:row#{ordinal}"),
            "nx segment index row identity",
        )?;
        let source_entry = ctx.copy_retained_text(&entry.name, "nx segment index source entry")?;
        let byte_offset = ordinal
            .checked_mul(12)
            .and_then(|offset| u64::try_from(offset).ok())
            .and_then(|offset| entry_offset.checked_add(offset))
            .ok_or_else(|| ctx.refuse_codec_limit("nx segment index row offset", 0, u64::MAX))?;
        rows.push(SegmentIndexRow {
            id,
            ordinal: ordinal_u32,
            type_code: row.type_code,
            subtype_code: row.subtype_code,
            value: row.value,
            source_entry,
            source_offset: byte_offset,
        });
    }
    Ok(rows)
}

/// Word position within one segment-index row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum SegmentIndexSlot {
    /// First row word.
    TypeCode,
    /// Second row word.
    SubtypeCode,
    /// Third row word.
    Value,
}

/// Validated link from a segment-index word to a compressed stream wrapper.
#[derive(Debug, PartialEq, Eq, Deserialize)]
#[cfg_attr(not(test), derive(Clone))]
#[serde(try_from = "row_wire::Wire")]
pub(super) struct SegmentStreamLink {
    /// Globally unique link identity.
    pub(super) id: String,
    /// Owning segment-index row.
    row: usize,
    /// Row word containing the wrapper offset.
    slot: SegmentIndexSlot,
    /// Zero-based stream ordinal in first segment-wrapper order.
    pub(super) stream_ordinal: u32,
    /// Decoded stream classification.
    pub(super) stream_kind: crate::parasolid::StreamKind,
    /// Bytes from the wrapper start to its zlib header.
    wrapper_byte_len: u32,
    /// Absolute file offset of the wrapper.
    pub(super) source_offset: u64,
}

#[derive(Clone, Copy)]
struct SegmentStreamCandidate {
    wrapper: SegmentStreamWrapper,
    slot: SegmentIndexSlot,
    stream_ordinal: usize,
    stream_kind: StreamKind,
}

fn segment_stream_candidates<'a>(
    ctx: &'a DecodeContext<'_>,
    container: &'a Container<'_>,
    streams: &'a [Stream],
) -> impl Iterator<Item = Result<Option<SegmentStreamCandidate>, CodecError>> + 'a {
    container.segment_stream_wrappers().map(move |wrapper| {
        let slot = match wrapper.word_ordinal {
            0 => SegmentIndexSlot::TypeCode,
            1 => SegmentIndexSlot::SubtypeCode,
            2 => SegmentIndexSlot::Value,
            _ => return Ok(None),
        };
        let Some(stream_ordinal) = ctx.position_by(
            streams,
            |stream| Ok(stream.file_offset == wrapper.zlib_offset),
            "nx segment stream matching",
        )?
        else {
            return Ok(None);
        };
        Ok(streams
            .get(stream_ordinal)
            .map(|stream| SegmentStreamCandidate {
                wrapper,
                slot,
                stream_ordinal,
                stream_kind: stream.kind(),
            }))
    })
}

#[cfg(test)]
std::thread_local! {
    static STREAM_LINK_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
impl Clone for SegmentStreamLink {
    fn clone(&self) -> Self {
        STREAM_LINK_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            id: self.id.clone(),
            row: self.row,
            slot: self.slot,
            stream_ordinal: self.stream_ordinal,
            stream_kind: self.stream_kind,
            wrapper_byte_len: self.wrapper_byte_len,
            source_offset: self.source_offset,
        }
    }
}

/// Body-image identity carried beside one validated Parasolid stream wrapper.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SegmentBodyBinding {
    /// Globally unique binding identity.
    pub(super) id: String,
    /// Validated stream-wrapper link owning the metadata tuple.
    pub(super) stream_link: String,
    /// Zero-based stream ordinal in first segment-wrapper order.
    pub(super) stream_ordinal: u32,
    /// Partition or plain cached-body stream classification.
    pub(super) stream_kind: crate::parasolid::StreamKind,
    /// Object index used by feature-history body operands.
    pub(super) body_object_index: u32,
    /// Second object index naming the same body image in feature history.
    pub(super) body_alias_object_index: u32,
    /// Serialized role word completing the five-word segment tuple.
    pub(super) stream_role: u32,
    /// Absolute file offset of the object-index word in the segment index.
    pub(super) source_offset: u64,
}

/// Return the one segment body binding named by an object index.
///
/// A primary-body or operand relation is valid only when the index matches
/// exactly one alias pair. Zero matches and alias collisions are unresolved.
pub(super) fn unique_segment_body_binding<'a>(
    ctx: &DecodeContext<'_>,
    object_index: u32,
    bindings: &'a [SegmentBodyBinding],
) -> Result<Option<&'a SegmentBodyBinding>, CodecError> {
    unique_binding(ctx, bindings, |binding| {
        binding.body_object_index == object_index || binding.body_alias_object_index == object_index
    })
}

/// Return one segment body binding whose alias identity is unique.
///
/// The alias lane is a distinct identity from the stream body's primary
/// object index. Callers use this function only when another native field
/// carries the alias identity and must not silently accept a primary-index
/// collision from a different body.
pub(super) fn unique_segment_body_alias_binding<'a>(
    ctx: &DecodeContext<'_>,
    object_index: u32,
    bindings: &'a [SegmentBodyBinding],
) -> Result<Option<&'a SegmentBodyBinding>, CodecError> {
    if ctx.any_by(
        bindings,
        |binding| Ok(binding.body_object_index == object_index),
        "match NX segment body bindings",
    )? {
        return Ok(None);
    }
    unique_binding(ctx, bindings, |binding| {
        binding.body_alias_object_index == object_index
    })
}

fn unique_binding<'a>(
    ctx: &DecodeContext<'_>,
    bindings: &'a [SegmentBodyBinding],
    matches: impl Fn(&SegmentBodyBinding) -> bool,
) -> Result<Option<&'a SegmentBodyBinding>, CodecError> {
    super::unique_by(
        ctx,
        bindings,
        |binding| Ok(matches(binding)),
        "match NX segment body bindings",
    )
}

/// Return the only plain-stream binding whose alias identity is `object_index`.
pub(super) fn unique_plain_alias_binding<'a>(
    ctx: &DecodeContext<'_>,
    object_index: u32,
    bindings: &'a [SegmentBodyBinding],
) -> Result<Option<&'a SegmentBodyBinding>, CodecError> {
    unique_binding(ctx, bindings, |binding| {
        binding.stream_kind == crate::parasolid::StreamKind::Plain
            && binding.body_alias_object_index == object_index
    })
}

/// Unambiguous terminal status of one segment-bound body image.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SegmentBodyLineageStatus {
    /// Globally unique status identity.
    pub(super) id: String,
    /// Segment binding whose alias pair names the body image.
    pub(super) segment_body_binding: String,
    /// First serialized body identity.
    pub(super) body_object_index: u32,
    /// Alias identity naming the same body image.
    pub(super) body_alias_object_index: u32,
    /// Whether the image remains terminal after retained history.
    pub(super) terminal: bool,
    /// Absolute source offset of the segment binding.
    pub(super) source_offset: u64,
}

/// Validated link from a segment-index word to a framed OM section.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct SegmentOmLink {
    /// Globally unique link identity.
    pub(super) id: String,
    /// Owning segment-index row.
    pub(super) row: String,
    /// Row word containing the section offset.
    pub(super) slot: SegmentIndexSlot,
    /// Role established by exact class declarations in the pointed registry.
    pub(super) schema_role: OmSchemaRole,
    /// Checked pointed and signature offsets.
    #[serde(flatten)]
    pub(super) location: OmLocation,
}

pub(super) struct BodyLineageInputs<'inputs> {
    pub(super) labels: &'inputs [FeatureOperationLabel],
    pub(super) references: &'inputs [FeatureBodyReference],
    pub(super) data_block_uses: &'inputs [FeatureBodyDataBlockUse],
    pub(super) data_blocks: &'inputs [DataBlock],
    pub(super) booleans: &'inputs [FeatureBooleanOperation],
    pub(super) operands: &'inputs [FeatureOperationBodyOperand],
    pub(super) bindings: &'inputs [SegmentBodyBinding],
    pub(super) inputs: &'inputs [FeatureInputBlock],
}

/// Return body objects whose latest decoded writer is not consumed by a later
/// Boolean, sewing, or trimming operation. Segment-bound bodies exist before
/// the retained history area unless a decoded operation writes them. Primary
/// references from operations with resolved offset-store inputs do not
/// participate in object-identity lineage, including missing or ambiguous
/// body ordinals or duplicate primary-body fields. The label arena is
/// source/newest-first; all history positions below use oldest-first order
/// within each section.
fn terminal_feature_body_indices(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    body_lineage_inputs: &BodyLineageInputs<'_>,
) -> Result<Option<BTreeSet<u32>>, cadmpeg_core::CodecError> {
    let &BodyLineageInputs {
        labels,
        references,
        data_block_uses,
        data_blocks,
        booleans,
        operands,
        bindings,
        inputs,
    } = body_lineage_inputs;

    let mut offset_store_reservation =
        ctx.reserve_scoped(0, "NX terminal offset-store references")?;
    let mut offset_store_references = BTreeSet::new();
    for use_ in ctx.admit_iter(data_block_uses, "NX terminal offset-store references")? {
        offset_store_reservation.with_storage(|| {
            ctx.insert_btree_set(
                &mut offset_store_references,
                use_.feature_body_reference.as_str(),
                "NX terminal offset-store references",
            )
        })?;
    }
    let (offset_store_operations, _offset_store_operations_storage) = ctx
        .with_scoped_storage("NX local offset_store_operations storage", || {
            crate::native::features::feature_input_store_operations(ctx, inputs, data_blocks)
        })?;
    let (unique_references, _unique_references_storage) = ctx
        .with_scoped_storage("NX local unique_references storage", || {
            crate::native::features::unique_feature_body_references(ctx, references)
        })?;
    let mut object_reservation = ctx.reserve_scoped(0, "NX terminal body object references")?;
    let mut object_references = Vec::new();
    for &reference in ctx
        .admit_iter(&unique_references, "NX terminal body object references")?
        .map(|(_, reference)| reference)
    {
        if ctx.contains_btree_set(
            &offset_store_references,
            reference.id.as_str(),
            "NX terminal offset-store references",
        )? || ctx.contains_btree_set(
            &offset_store_operations,
            reference.operation_label.as_str(),
            "NX offset-store operations",
        )? {
            continue;
        }
        ctx.reserve_scoped_vec(
            &mut object_reservation,
            &mut object_references,
            1,
            "NX terminal body object references",
        )?;
        object_references.push(reference);
    }
    if object_references.is_empty() && bindings.is_empty() {
        return Ok(None);
    }
    let (chronological_labels, _chronological_labels_storage) = ctx
        .with_scoped_storage("NX local chronological_labels storage", || {
            crate::native::features::feature_operation_chronological_labels(ctx, labels)
        })?;
    let mut positions_reservation = ctx.reserve_scoped(0, "NX terminal label positions")?;
    let mut positions = BTreeMap::new();
    let mut kinds_reservation = ctx.reserve_scoped(0, "NX terminal operation kinds")?;
    let mut operation_kinds = BTreeMap::new();
    for (position, label) in ctx
        .admit_iter(&chronological_labels, "NX terminal label positions")?
        .enumerate()
    {
        positions_reservation.with_storage(|| {
            ctx.insert_btree_map(
                &mut positions,
                label.id.as_str(),
                position,
                "NX terminal label positions",
            )
        })?;
        kinds_reservation.with_storage(|| {
            ctx.insert_btree_map(
                &mut operation_kinds,
                label.id.as_str(),
                label.value.as_str(),
                "NX terminal operation kinds",
            )
        })?;
    }
    let (aliases, _aliases_storage) = ctx
        .with_scoped_storage("NX local aliases storage", || {
            body_alias_roots(ctx, bindings)
        })?;
    let canonical = |identity: u32| -> Result<u32, CodecError> {
        Ok(ctx
            .get_btree_map(&aliases, &identity, "NX canonical segment body")?
            .copied()
            .unwrap_or(identity))
    };
    let position_of = |label: &str| -> Result<Option<usize>, CodecError> {
        Ok(ctx
            .get_btree_map(&positions, label, "NX terminal label positions")?
            .copied())
    };
    let kind_of = |label: &str| -> Result<Option<&str>, CodecError> {
        Ok(ctx
            .get_btree_map(&operation_kinds, label, "NX terminal operation kinds")?
            .copied())
    };
    let (segment_boolean_operations, _segment_boolean_storage) = ctx
        .with_scoped_storage("NX segment Boolean operation labels", || {
            segment_boolean_operation_labels(ctx, booleans, data_blocks)
        })?;
    let is_segment_boolean = |operation: &FeatureBooleanOperation| {
        ctx.contains_btree_set(
            &segment_boolean_operations,
            operation.operation_label.as_str(),
            "NX segment Boolean operation labels",
        )
    };
    let mut writers_reservation = ctx.reserve_scoped(0, "NX terminal last writers")?;
    let mut last_writers = BTreeMap::<u32, Option<usize>>::new();
    for binding in ctx.admit_iter(bindings, "NX terminal last writers")? {
        for identity in [binding.body_object_index, binding.body_alias_object_index] {
            let identity = canonical(identity)?;
            writers_reservation.with_storage(|| {
                ctx.insert_btree_map(
                    &mut last_writers,
                    identity,
                    None,
                    "NX terminal last writers",
                )
            })?;
        }
    }
    {
        let mut record_writer = |body, position| -> Result<(), cadmpeg_core::CodecError> {
            let writer = writers_reservation.with_storage(|| {
                ctx.entry_btree_map(&mut last_writers, body, "NX terminal last writers")
            })?;
            let writer = writer.or_default();
            if writer.is_none_or(|writer| writer < position) {
                *writer = Some(position);
            }
            Ok(())
        };
        for reference in ctx.admit_iter(&object_references, "NX terminal body writers")? {
            let Some(position) = position_of(reference.operation_label.as_str())? else {
                return Ok(None);
            };
            if matches!(kind_of(reference.operation_label.as_str())?, Some("DELETE")) {
                continue;
            }
            record_writer(canonical(reference.body.value())?, position)?;
        }
        for operation in ctx.admit_iter(booleans, "NX terminal Boolean writers")? {
            if !is_segment_boolean(operation)? {
                continue;
            }
            let Some(position) = position_of(operation.operation_label.as_str())? else {
                return Ok(None);
            };
            record_writer(canonical(operation.target.token.value())?, position)?;
        }
    }
    let precedes_writer = |body: u32, position: usize| -> Result<bool, CodecError> {
        Ok(ctx
            .get_btree_map(&last_writers, &body, "NX terminal last writers")?
            .is_some_and(|writer| writer.is_none_or(|writer| writer < position)))
    };
    let mut consumed = BTreeSet::new();
    let mut consumed_reservation = ctx.reserve_scoped(0, "NX consumed segment bodies")?;
    for operation in ctx.admit_iter(booleans, "NX consumed segment bodies")? {
        if !is_segment_boolean(operation)? {
            continue;
        }
        let Some(position) = position_of(operation.operation_label.as_str())? else {
            return Ok(None);
        };
        for tool in ctx.admit_iter(&operation.tools, "NX consumed segment bodies")? {
            let tool = canonical(tool.token.value())?;
            if precedes_writer(tool, position)? {
                consumed_reservation.with_storage(|| {
                    ctx.insert_btree_set(&mut consumed, tool, "NX consumed segment bodies")
                })?;
            }
        }
    }
    for reference in ctx.admit_iter(&object_references, "NX consumed segment bodies")? {
        if !matches!(kind_of(reference.operation_label.as_str())?, Some("DELETE")) {
            continue;
        }
        let Some(position) = position_of(reference.operation_label.as_str())? else {
            return Ok(None);
        };
        let body = canonical(reference.body.value())?;
        if precedes_writer(body, position)? {
            consumed_reservation.with_storage(|| {
                ctx.insert_btree_set(&mut consumed, body, "NX consumed segment bodies")
            })?;
        }
    }
    for operand in ctx.admit_iter(operands, "NX consumed segment bodies")? {
        // Offset-store operands use the operation-body namespace, even when
        // their serialized index happens to equal a segment-body identity.
        // Only the resolved segment-binding lane can consume a segment image.
        if operand.segment_body_bindings.is_empty()
            || !matches!(
                kind_of(operand.operation_label.as_str())?,
                Some("SEW" | "TRIM BODY")
            )
        {
            continue;
        }
        let Some(position) = position_of(operand.operation_label.as_str())? else {
            return Ok(None);
        };
        let body = canonical(operand.operand.atom.value())?;
        if precedes_writer(body, position)? {
            consumed_reservation.with_storage(|| {
                ctx.insert_btree_set(&mut consumed, body, "NX consumed segment bodies")
            })?;
        }
    }
    let mut terminal = BTreeSet::new();
    let mut retain_terminal = |identity: u32| -> Result<(), CodecError> {
        let root = canonical(identity)?;
        if ctx.contains_key_btree_map(&last_writers, &root, "NX terminal last writers")?
            && !ctx.contains_btree_set(&consumed, &root, "NX consumed segment bodies")?
        {
            ctx.insert_btree_set(&mut terminal, identity, "NX terminal segment bodies")?;
        }
        Ok(())
    };
    for reference in ctx.admit_iter(&object_references, "NX terminal segment bodies")? {
        retain_terminal(reference.body.value())?;
    }
    for binding in ctx.admit_iter(bindings, "NX terminal segment bodies")? {
        retain_terminal(binding.body_object_index)?;
        retain_terminal(binding.body_alias_object_index)?;
    }
    Ok(Some(terminal))
}

/// Resolve one atomic terminal status for every segment-bound body image.
pub(super) fn segment_body_lineage_statuses(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    body_lineage_inputs: &BodyLineageInputs<'_>,
) -> Result<Option<Vec<SegmentBodyLineageStatus>>, cadmpeg_core::CodecError> {
    let (terminal, _terminal_storage) = ctx
        .with_scoped_storage("NX local terminal storage", || {
            terminal_feature_body_indices(ctx, body_lineage_inputs)
        })?;
    let Some(terminal) = terminal else {
        return Ok(None);
    };
    let mut output = Vec::new();
    for binding in ctx.admit_iter(body_lineage_inputs.bindings, "NX segment lineage statuses")? {
        let mut statuses = [false; 2];
        for (status, identity) in statuses
            .iter_mut()
            .zip([binding.body_object_index, binding.body_alias_object_index])
        {
            *status = ctx.contains_btree_set(&terminal, &identity, "NX terminal segment bodies")?;
        }
        if statuses[0] != statuses[1] {
            return Ok(None);
        }
        let key = ctx
            .rsplit_once(&binding.id, "#", "NX segment lineage status identity")?
            .map_or(binding.id.as_str(), |(_, key)| key);
        let id = ctx.format_retained(
            format_args!("nx:segment-body-lineage:status#{key}"),
            "NX segment lineage status identity",
        )?;
        let segment_body_binding =
            ctx.copy_retained_text(&binding.id, "NX segment lineage binding identity")?;
        ctx.push_vec(
            &mut output,
            SegmentBodyLineageStatus {
                id,
                segment_body_binding,
                body_object_index: binding.body_object_index,
                body_alias_object_index: binding.body_alias_object_index,
                terminal: statuses[0],
                source_offset: binding.source_offset,
            },
            "NX segment lineage statuses",
        )?;
    }
    Ok(Some(output))
}

/// Namespace proof for one Boolean's target and ordered tool participants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum BooleanOffsetStoreResolution {
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
pub(super) fn boolean_offset_store_resolution(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    operation: &FeatureBooleanOperation,
    data_blocks: &[DataBlock],
) -> Result<BooleanOffsetStoreResolution, cadmpeg_core::CodecError> {
    let operation_name = "scan NX Boolean participants";
    let mut participants = Vec::new();
    let mut participant_reservation = ctx.reserve_scoped(0, operation_name)?;
    let mut has_offset_store_evidence = false;
    let mut every_participant_resolved = true;
    let mut section_ordinal = None;
    // Returns true when the participant set cannot have a one-store relation.
    let mut conflicts = |object_index: u32| -> Result<bool, CodecError> {
        let is_column_block = |block: &DataBlock| {
            Ok(block.role == DataBlockRole::Column && block.block_ordinal == object_index)
        };
        let Some(first) = ctx.position_by(data_blocks, is_column_block, operation_name)? else {
            every_participant_resolved = false;
            return Ok(false);
        };
        has_offset_store_evidence = true;
        let (found, rest) = data_blocks.split_at(first + 1);
        let Some(block) = found.last() else {
            return Ok(true);
        };
        if ctx.any_by(rest, is_column_block, operation_name)?
            || section_ordinal.is_some_and(|section| section != block.section_ordinal)
        {
            return Ok(true);
        }
        section_ordinal = Some(block.section_ordinal);
        ctx.reserve_scoped_vec(
            &mut participant_reservation,
            &mut participants,
            1,
            operation_name,
        )?;
        participants.push((object_index, block));
        Ok(false)
    };
    if conflicts(operation.target.token.value())? {
        return Ok(BooleanOffsetStoreResolution::Unresolved);
    }
    for tool in ctx.admit_iter(&operation.tools, operation_name)? {
        if conflicts(tool.token.value())? {
            return Ok(BooleanOffsetStoreResolution::Unresolved);
        }
    }
    if !has_offset_store_evidence {
        return Ok(BooleanOffsetStoreResolution::None);
    }
    if !every_participant_resolved {
        return Ok(BooleanOffsetStoreResolution::Unresolved);
    }
    let mut complete = BTreeMap::new();
    for &(object_index, block) in
        ctx.admit_iter(&participants, "NX Boolean offset-store participants")?
    {
        let id = ctx.copy_retained_text(&block.id, "NX Boolean offset-store block identity")?;
        ctx.insert_btree_map(
            &mut complete,
            object_index,
            id,
            "NX Boolean offset-store participants",
        )?;
    }
    Ok(BooleanOffsetStoreResolution::Complete(complete))
}

/// Return Boolean operations that are safe to treat as segment-object
/// lineage. Complete offset-store selections and unresolved offset-store
/// candidates have no segment-body effect; integer equality across the two
/// namespaces is not an identity proof. Only operations with no offset-store
/// participant evidence retain the native Boolean lineage rules.
fn segment_boolean_operation_labels(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    booleans: &[FeatureBooleanOperation],
    data_blocks: &[DataBlock],
) -> Result<BTreeSet<String>, cadmpeg_core::CodecError> {
    let mut labels = BTreeSet::new();

    for operation in ctx.admit_iter(booleans, "NX segment Boolean operation labels")? {
        if !matches!(
            boolean_offset_store_resolution(ctx, operation, data_blocks)?,
            BooleanOffsetStoreResolution::None
        ) {
            continue;
        }
        let label = ctx.copy_retained_text(
            &operation.operation_label,
            "allocate NX segment Boolean operation label",
        )?;
        ctx.insert_btree_set(&mut labels, label, "NX segment Boolean operation labels")?;
    }
    Ok(labels)
}

/// Map each segment body identity to the smallest identity in its transitive alias component.
pub(super) fn body_alias_roots(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    bindings: &[SegmentBodyBinding],
) -> Result<BTreeMap<u32, u32>, cadmpeg_core::CodecError> {
    let mut adjacency = BTreeMap::<u32, BTreeSet<u32>>::new();
    let mut adjacency_reservation = ctx.reserve_scoped(0, "NX segment alias adjacency")?;
    for binding in ctx.admit_iter(bindings, "NX segment alias adjacency")? {
        for (identity, alias) in [
            (binding.body_object_index, binding.body_alias_object_index),
            (binding.body_alias_object_index, binding.body_object_index),
        ] {
            adjacency_reservation.with_storage(|| {
                let neighbors = ctx
                    .entry_btree_map(&mut adjacency, identity, "NX segment alias identities")?
                    .or_default();
                ctx.insert_btree_set(neighbors, alias, "NX segment alias links")
            })?;
        }
    }
    let mut roots = BTreeMap::new();
    for &identity in ctx
        .admit_iter(&adjacency, "NX segment alias components")?
        .map(|(identity, _)| identity)
    {
        if ctx.contains_key_btree_map(&roots, &identity, "NX segment alias roots")? {
            continue;
        }
        let mut component = BTreeSet::new();
        let mut component_reservation = ctx.reserve_scoped(0, "NX segment alias component")?;
        let mut pending_reservation = ctx.reserve_scoped(0, "NX segment alias traversal")?;
        let mut pending = Vec::new();
        ctx.reserve_scoped_vec(
            &mut pending_reservation,
            &mut pending,
            1,
            "NX segment alias traversal",
        )?;
        pending.push(identity);
        loop {
            ctx.charge_work(1, "walk NX segment alias pending queue")?;
            let Some(member) = pending.pop() else {
                break;
            };
            if !component_reservation.with_storage(|| {
                ctx.insert_btree_set(&mut component, member, "NX segment alias component")
            })? {
                continue;
            }
            let Some(neighbors) =
                ctx.get_btree_map(&adjacency, &member, "NX segment alias adjacency")?
            else {
                continue;
            };
            for neighbor in ctx.admit_iter(neighbors, "NX segment alias traversal")? {
                if ctx.contains_btree_set(&component, neighbor, "NX segment alias component")? {
                    continue;
                }
                ctx.reserve_scoped_vec(
                    &mut pending_reservation,
                    &mut pending,
                    1,
                    "NX segment alias traversal",
                )?;
                pending.push(*neighbor);
            }
        }
        let root = ctx
            .admit_iter(&component, "NX segment alias component root")?
            .copied()
            .fold(identity, u32::min);
        for &member in ctx.admit_iter(&component, "NX segment alias roots")? {
            ctx.insert_btree_map(&mut roots, member, root, "NX segment alias roots")?;
        }
    }
    Ok(roots)
}

/// Resolve segment-index words that point to validated framed OM sections.
pub(super) fn segment_om_links(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    container: &Container,
) -> Result<Vec<SegmentOmLink>, cadmpeg_core::CodecError> {
    let Some((entry, index)) = container.segment_index() else {
        return Ok(Vec::new());
    };
    let entry_offset = entry.file_span().map_or(0, |(offset, _)| offset);
    let sections = container.om_sections(ctx)?;
    let mut links = Vec::new();
    for (row_ordinal, row) in index.rows().enumerate() {
        ctx.charge_work(1, "NX segment OM index rows")?;
        for (slot, relative) in [
            (SegmentIndexSlot::TypeCode, row.type_code),
            (SegmentIndexSlot::SubtypeCode, row.subtype_code),
            (SegmentIndexSlot::Value, row.value),
        ] {
            let relative = usize::try_from(relative).map_err(|_| {
                ctx.refuse_codec_limit("NX segment OM relative offset", 0, u64::from(relative))
            })?;
            let Some(relative_u64) = u64::try_from(relative).ok() else {
                continue;
            };
            let separated_marker = match entry_offset.checked_add(relative_u64) {
                Some(offset) => container
                    .bounded_entry_bytes(ctx, offset, 4)?
                    .is_some_and(|bytes| bytes == [0xc0, 0xd1, 0xf1, 0xed]),
                None => false,
            };
            let role_at = |offset| -> Result<Option<OmSchemaRole>, CodecError> {
                for (candidate, section) in sections.iter().rev() {
                    ctx.charge_work(1, "NX segment OM section lookup")?;
                    if section.offset == offset
                        && ctx.equal_bytes(
                            candidate.name.as_bytes(),
                            entry.name.as_bytes(),
                            "NX segment OM section entry",
                        )?
                    {
                        return classify_om_schema_role(ctx, section).map(Some);
                    }
                }
                Ok(None)
            };
            let (separator_byte_len, schema_role) = if let Some(role) = role_at(relative)? {
                (0usize, role)
            } else if separated_marker {
                let Some(offset) = relative.checked_add(4) else {
                    continue;
                };
                let Some(role) = role_at(offset)? else {
                    continue;
                };
                (4, role)
            } else {
                continue;
            };
            let Some(location) = entry_offset.checked_add(relative_u64).and_then(|offset| {
                OmLocation::new(offset, u32::try_from(separator_byte_len).ok()?)
            }) else {
                continue;
            };
            ctx.reserve_vec(&mut links, 1, "NX segment OM links")?;
            let id = segment_link_identity(ctx, "nx:segment-om-links:link#", links.len())?;
            let row = segment_link_identity(ctx, "nx:segment-index:row#", row_ordinal)?;
            links.push(SegmentOmLink {
                id,
                row,
                slot,
                schema_role,
                location,
            });
        }
    }
    Ok(links)
}

fn segment_link_identity(
    ctx: &DecodeContext<'_>,
    prefix: &str,
    ordinal: usize,
) -> Result<String, CodecError> {
    ctx.format_retained(
        format_args!("{prefix}{ordinal}"),
        "retain NX segment link identity",
    )
}

/// Resolve segment-index words that point to validated compressed wrappers.
pub(super) fn segment_stream_links(
    ctx: &DecodeContext<'_>,
    container: &Container,
    streams: &[Stream],
) -> Result<Vec<SegmentStreamLink>, CodecError> {
    let mut links = Vec::new();
    for candidate in segment_stream_candidates(ctx, container, streams) {
        ctx.charge_work(1, "nx segment stream links")?;
        let Some(candidate) = candidate? else {
            continue;
        };
        let ordinal = links.len();
        let stream_ordinal = u32::try_from(candidate.stream_ordinal)
            .map_err(|_| ctx.refuse_codec_limit("nx segment stream ordinal", 0, u64::MAX))?;
        let wrapper_byte_len = u32::try_from(candidate.wrapper.wrapper_byte_len)
            .map_err(|_| ctx.refuse_codec_limit("nx segment wrapper size", 0, u64::MAX))?;
        let source_offset = u64::try_from(candidate.wrapper.wrapper_offset)
            .map_err(|_| ctx.refuse_codec_limit("nx segment wrapper offset", 0, u64::MAX))?;
        ctx.charge_entities(1, "nx segment stream links")?;
        ctx.reserve_vec(&mut links, 1, "nx segment stream links")?;
        let id = ctx.format_retained(
            format_args!("nx:segment-stream-links:link#{ordinal}"),
            "nx segment stream link identity",
        )?;
        links.push(SegmentStreamLink {
            id,
            row: candidate.wrapper.row_ordinal,
            slot: candidate.slot,
            stream_ordinal,
            stream_kind: candidate.stream_kind,
            wrapper_byte_len,
            source_offset,
        });
    }
    Ok(links)
}

/// Bind partition and cached-body streams to feature-history body object indices.
pub(super) fn segment_body_bindings(
    ctx: &DecodeContext<'_>,
    container: &Container,
    streams: &[Stream],
) -> Result<Vec<SegmentBodyBinding>, CodecError> {
    let Some((entry, index)) = container.segment_index() else {
        return Ok(Vec::new());
    };
    let entry_offset = entry.file_span().map_or(0, |(offset, _)| offset);
    let word_at = |word: usize| {
        let row = index.row(word / 3)?;
        Some(match word % 3 {
            0 => row.type_code,
            1 => row.subtype_code,
            _ => row.value,
        })
    };
    let mut bindings = Vec::new();
    let mut link_ordinal = 0usize;
    for candidate in segment_stream_candidates(ctx, container, streams) {
        ctx.charge_work(1, "nx segment body bindings")?;
        let Some(candidate) = candidate? else {
            continue;
        };
        let ordinal = link_ordinal;
        link_ordinal = link_ordinal
            .checked_add(1)
            .ok_or_else(|| ctx.refuse_codec_limit("nx segment link ordinal", 0, u64::MAX))?;
        if !matches!(
            candidate.stream_kind,
            crate::parasolid::StreamKind::Partition | crate::parasolid::StreamKind::Plain
        ) {
            continue;
        }
        let slot = match candidate.slot {
            SegmentIndexSlot::TypeCode => 0,
            SegmentIndexSlot::SubtypeCode => 1,
            SegmentIndexSlot::Value => 2,
        };
        let Some(pointer_word) = candidate
            .wrapper
            .row_ordinal
            .checked_mul(3)
            .and_then(|row| row.checked_add(slot))
        else {
            continue;
        };
        let Some(fields) = pointer_word.checked_add(1).and_then(|after| {
            (word_at(after) == Some(0)).then_some((
                word_at(after.checked_add(1)?)?,
                word_at(after.checked_add(2)?)?,
                word_at(after.checked_add(3)?)?,
            ))
        }) else {
            continue;
        };
        let (body_object_index, body_alias_object_index, stream_role) = fields;
        if body_object_index == 0 || body_alias_object_index == 0 {
            continue;
        }
        let Some(source_offset) = pointer_word
            .checked_add(2)
            .and_then(|word| word.checked_mul(4))
            .and_then(|offset| u64::try_from(offset).ok())
            .and_then(|offset| entry_offset.checked_add(offset))
        else {
            continue;
        };
        let stream_ordinal = u32::try_from(candidate.stream_ordinal)
            .map_err(|_| ctx.refuse_codec_limit("nx segment stream ordinal", 0, u64::MAX))?;
        ctx.charge_entities(1, "nx segment body bindings")?;
        ctx.reserve_vec(&mut bindings, 1, "nx segment body bindings")?;
        let id = ctx.format_retained(
            format_args!("nx:segment-body-bindings:binding#{ordinal}"),
            "nx segment body binding identity",
        )?;
        let stream_link = ctx.format_retained(
            format_args!("nx:segment-stream-links:link#{ordinal}"),
            "nx segment body stream link identity",
        )?;
        bindings.push(SegmentBodyBinding {
            id,
            stream_link,
            stream_ordinal,
            stream_kind: candidate.stream_kind,
            body_object_index,
            body_alias_object_index,
            stream_role,
            source_offset,
        });
    }
    Ok(bindings)
}

#[cfg(test)]
mod tests {
    mod om_links;
    fn segment_alias_lineage_refusal(
        configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
    ) -> cadmpeg_core::CodecError {
        let bindings = [super::SegmentBodyBinding {
            id: "binding#0".to_string(),
            stream_link: "stream#0".to_string(),
            stream_ordinal: 0,
            stream_kind: crate::parasolid::StreamKind::Partition,
            body_object_index: 10,
            body_alias_object_index: 11,
            stream_role: 19,
            source_offset: 0,
        }];
        let route = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
            super::segment_body_lineage_statuses(
                ctx,
                &super::BodyLineageInputs {
                    labels: &[],
                    references: &[],
                    data_block_uses: &[],
                    data_blocks: &[],
                    booleans: &[],
                    operands: &[],
                    bindings: &bindings,
                    inputs: &[],
                },
            )
        };
        let admitted = crate::test_support::with_decode_context(|ctx| route(ctx))
            .expect("admitted segment alias lineage")
            .expect("complete segment alias lineage");
        assert_eq!(admitted.len(), 1);

        crate::test_support::with_decode_context_over(
            &[],
            |policy| {
                configure(policy);
            },
            |ctx| route(ctx).expect_err("segment alias resource limit"),
        )
    }

    #[test]
    fn segment_alias_lineage_refuses_collection_limit() {
        let error = segment_alias_lineage_refusal(|policy| policy.limits.max_collection_items = 0);
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
        );
    }

    #[test]
    fn segment_alias_lineage_refuses_retained_limit() {
        let error = segment_alias_lineage_refusal(|policy| policy.limits.max_retained_bytes = 0);
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes)
        );
    }

    #[test]
    fn segment_alias_lineage_refuses_scoped_limit() {
        let error =
            segment_alias_lineage_refusal(|policy| policy.limits.max_materialized_bytes = 0);
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes)
        );
    }

    #[test]
    fn segment_alias_lineage_refuses_work_limit() {
        let error = segment_alias_lineage_refusal(|policy| policy.limits.max_work_units = 0);
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
        );
    }

    use crate::test_support::test_om::segment_body_binding_payload;
    use crate::test_support::test_om::segment_body_binding_repeated_link_payload;
    use crate::test_support::test_om::segment_extended_wrapper_payload;
    use crate::test_support::test_om::segment_index_payload;
    use crate::test_support::test_om::segment_om_payload;
    use crate::test_support::test_om::segment_stream_payload;
    use crate::test_support::test_om::size_framed_om_section;
    use crate::test_support::test_prt::prt_with_named_payloads;
    use std::io::Cursor;

    use cadmpeg_ir::codec::{Codec, DecodeOptions};

    use crate::NxCodec;

    use crate::native::om::OmSchemaRole;

    #[test]
    fn decode_retains_ordered_ug_part_segment_index_rows() {
        let file = prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", segment_index_payload())]);
        let result = NxCodec
            .decode(&mut Cursor::new(file), &DecodeOptions::default())
            .expect("required invariant");
        let namespace = result.ir().native.namespace("nx").expect("NX namespace");
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
    fn segment_index_rows_refuse_collection_limit_before_record_allocation() {
        use cadmpeg_core::decode::ResourceDimension;

        let file = prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", segment_index_payload())]);
        let container =
            crate::test_support::with_decode_context(|ctx| crate::container::scan_bytes(ctx, file))
                .expect("valid segment-index container");

        crate::test_support::with_decode_context_over(
            &[],
            |policy| {
                policy.limits.max_collection_items = 1;
            },
            |ctx| {
                let error = super::segment_index_rows(ctx, &container)
                    .expect_err("two native rows exceed one collection item");
                assert!(matches!(
                    error,
                    cadmpeg_core::CodecError::ResourceLimit(limit)
                        if limit.dimension == ResourceDimension::CollectionItems
                            && limit.operation == "nx segment index rows"
                ));
            },
        );
    }

    #[test]
    fn segment_index_rows_refuse_identity_retained_limit() {
        use cadmpeg_core::decode::ResourceDimension;

        let file = prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", segment_index_payload())]);
        let container =
            crate::test_support::with_decode_context(|ctx| crate::container::scan_bytes(ctx, file))
                .expect("valid segment-index container");
        let row_slots = 4 * std::mem::size_of::<super::SegmentIndexRow>();
        let first_id = "nx:segment-index:row#0";

        crate::test_support::with_decode_context_over(
            &[],
            |policy| {
                policy.limits.max_retained_bytes =
                    u64::try_from(row_slots + first_id.len() - 1).unwrap();
            },
            |ctx| {
                let error = super::segment_index_rows(ctx, &container)
                    .expect_err("first identity exceeds the retained limit by one byte");
                assert!(matches!(
                    error,
                    cadmpeg_core::CodecError::ResourceLimit(limit)
                        if limit.dimension == ResourceDimension::RetainedBytes
                            && limit.operation == "nx segment index row identity"
                ));
            },
        );
    }

    #[test]
    fn segment_index_rows_refuse_source_entry_retained_limit() {
        use cadmpeg_core::decode::ResourceDimension;

        let file = prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", segment_index_payload())]);
        let container =
            crate::test_support::with_decode_context(|ctx| crate::container::scan_bytes(ctx, file))
                .expect("valid segment-index container");
        let row_slots = 4 * std::mem::size_of::<super::SegmentIndexRow>();
        let first_id = "nx:segment-index:row#0";
        let source_entry = "/Root/UG_PART/UG_PART";

        crate::test_support::with_decode_context_over(
            &[],
            |policy| {
                policy.limits.max_retained_bytes =
                    u64::try_from(row_slots + first_id.len() + source_entry.len() - 1).unwrap();
            },
            |ctx| {
                let error = super::segment_index_rows(ctx, &container)
                    .expect_err("source entry exceeds the retained limit by one byte");
                assert!(matches!(
                    error,
                    cadmpeg_core::CodecError::ResourceLimit(limit)
                        if limit.dimension == ResourceDimension::RetainedBytes
                            && limit.operation == "nx segment index source entry"
                ));
            },
        );
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
        assert_eq!(links[0].row, 0);
        assert_eq!(links[0].slot, super::SegmentIndexSlot::TypeCode);
        assert_eq!(links[0].stream_ordinal, 0);
        assert_eq!(links[0].stream_kind.label(), "deltas");
        assert_eq!(links[0].wrapper_byte_len, 8);
    }

    #[test]
    fn segment_stream_links_refuse_matching_work_limit() {
        use cadmpeg_core::decode::ResourceDimension;

        let file = prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", segment_stream_payload())]);

        crate::test_support::with_decode_context_over(
            &file,
            |_| {},
            |scan_ctx| {
                let root = cadmpeg_core::decode::View::over_retained(&file);

                let scan = crate::decode::scan(scan_ctx, root).expect("valid stream wrapper");

                crate::test_support::with_decode_context_over(
                    &[],
                    |policy| {
                        policy.limits.max_work_units = 0;
                    },
                    |ctx| {
                        let error =
                            super::segment_stream_links(ctx, &scan.container, &scan.streams)
                                .expect_err("stream matching needs work");
                        assert!(matches!(
                            error,
                            cadmpeg_core::CodecError::ResourceLimit(limit)
                                if limit.dimension == ResourceDimension::WorkUnits
                                    && limit.operation == "nx segment stream matching"
                        ));
                    },
                );
            },
        );
    }

    #[test]
    fn segment_stream_links_refuse_collection_limit() {
        use cadmpeg_core::decode::ResourceDimension;

        let file = prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", segment_stream_payload())]);

        crate::test_support::with_decode_context_over(
            &file,
            |_| {},
            |scan_ctx| {
                let root = cadmpeg_core::decode::View::over_retained(&file);

                let scan = crate::decode::scan(scan_ctx, root).expect("valid stream wrapper");

                crate::test_support::with_decode_context_over(
                    &[],
                    |policy| {
                        policy.limits.max_collection_items = 0;
                    },
                    |ctx| {
                        let error =
                            super::segment_stream_links(ctx, &scan.container, &scan.streams)
                                .expect_err("one stream link exceeds zero collection items");
                        assert!(matches!(
                            error,
                            cadmpeg_core::CodecError::ResourceLimit(limit)
                                if limit.dimension == ResourceDimension::CollectionItems
                                    && limit.operation == "nx segment stream links"
                        ));
                    },
                );
            },
        );
    }

    #[test]
    fn segment_stream_links_refuse_retained_identity_limit() {
        use cadmpeg_core::decode::ResourceDimension;

        let file = prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", segment_stream_payload())]);

        crate::test_support::with_decode_context_over(
            &file,
            |_| {},
            |scan_ctx| {
                let root = cadmpeg_core::decode::View::over_retained(&file);

                let scan = crate::decode::scan(scan_ctx, root).expect("valid stream wrapper");
                let slot = 4 * std::mem::size_of::<super::SegmentStreamLink>();
                let id = "nx:segment-stream-links:link#0";

                crate::test_support::with_decode_context_over(
                    &[],
                    |policy| {
                        policy.limits.max_retained_bytes =
                            u64::try_from(slot + id.len() - 1).unwrap();
                    },
                    |ctx| {
                        let error =
                            super::segment_stream_links(ctx, &scan.container, &scan.streams)
                                .expect_err(
                                    "stream link identity exceeds the retained limit by one byte",
                                );
                        assert!(matches!(
                            error,
                            cadmpeg_core::CodecError::ResourceLimit(limit)
                                if limit.dimension == ResourceDimension::RetainedBytes
                                    && limit.operation == "nx segment stream link identity"
                        ));
                    },
                );
            },
        );
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
        assert_eq!(bindings[0].stream_kind.label(), "partition");
        assert_eq!(bindings[0].body_object_index, 94);
        assert_eq!(bindings[0].body_alias_object_index, 150);
        assert_eq!(bindings[0].stream_role, 19);
        assert_eq!(bindings[0].source_offset, 108);
    }

    #[test]
    fn segment_body_bindings_refuse_collection_limit_before_record_allocation() {
        use cadmpeg_core::decode::ResourceDimension;

        let file = prt_with_named_payloads(&[(
            "/Root/UG_PART/UG_PART",
            segment_body_binding_payload("partition"),
        )]);

        crate::test_support::with_decode_context_over(
            &file,
            |_| {},
            |scan_ctx| {
                let root = cadmpeg_core::decode::View::over_retained(&file);

                let scan = crate::decode::scan(scan_ctx, root).expect("valid partition stream");

                crate::test_support::with_decode_context_over(
                    &[],
                    |policy| {
                        policy.limits.max_collection_items = 0;
                    },
                    |ctx| {
                        let error =
                            super::segment_body_bindings(ctx, &scan.container, &scan.streams)
                                .expect_err("one binding exceeds zero collection items");
                        assert!(matches!(
                            error,
                            cadmpeg_core::CodecError::ResourceLimit(limit)
                                if limit.dimension == ResourceDimension::CollectionItems
                                    && limit.operation == "nx segment body bindings"
                        ));
                    },
                );
            },
        );
    }

    #[test]
    fn segment_body_bindings_refuse_identity_retained_limit() {
        use cadmpeg_core::decode::ResourceDimension;

        let file = prt_with_named_payloads(&[(
            "/Root/UG_PART/UG_PART",
            segment_body_binding_payload("partition"),
        )]);

        crate::test_support::with_decode_context_over(
            &file,
            |_| {},
            |scan_ctx| {
                let root = cadmpeg_core::decode::View::over_retained(&file);

                let scan = crate::decode::scan(scan_ctx, root).expect("valid partition stream");
                let binding_slot = 4 * std::mem::size_of::<super::SegmentBodyBinding>();
                let first_id = "nx:segment-body-bindings:binding#0";

                crate::test_support::with_decode_context_over(
                    &[],
                    |policy| {
                        policy.limits.max_retained_bytes =
                            u64::try_from(binding_slot + first_id.len() - 1).unwrap();
                    },
                    |ctx| {
                        let error =
                            super::segment_body_bindings(ctx, &scan.container, &scan.streams)
                                .expect_err("binding identity exceeds retained limit by one byte");
                        assert!(matches!(
                            error,
                            cadmpeg_core::CodecError::ResourceLimit(limit)
                                if limit.dimension == ResourceDimension::RetainedBytes
                                    && limit.operation == "nx segment body binding identity"
                        ));
                    },
                );
            },
        );
    }

    #[test]
    fn segment_body_bindings_refuse_stream_link_identity_retained_limit() {
        use cadmpeg_core::decode::ResourceDimension;

        let file = prt_with_named_payloads(&[(
            "/Root/UG_PART/UG_PART",
            segment_body_binding_payload("partition"),
        )]);

        crate::test_support::with_decode_context_over(
            &file,
            |_| {},
            |scan_ctx| {
                let root = cadmpeg_core::decode::View::over_retained(&file);

                let scan = crate::decode::scan(scan_ctx, root).expect("valid partition stream");
                let binding_slot = 4 * std::mem::size_of::<super::SegmentBodyBinding>();
                let binding_id = "nx:segment-body-bindings:binding#0";
                let stream_link = "nx:segment-stream-links:link#0";

                crate::test_support::with_decode_context_over(
                    &[],
                    |policy| {
                        policy.limits.max_retained_bytes =
                            u64::try_from(binding_slot + binding_id.len() + stream_link.len() - 1)
                                .unwrap();
                    },
                    |ctx| {
                        let error =
                            super::segment_body_bindings(ctx, &scan.container, &scan.streams)
                                .expect_err(
                                    "stream-link identity exceeds retained limit by one byte",
                                );
                        assert!(matches!(
                            error,
                            cadmpeg_core::CodecError::ResourceLimit(limit)
                                if limit.dimension == ResourceDimension::RetainedBytes
                                    && limit.operation == "nx segment body stream link identity"
                        ));
                    },
                );
            },
        );
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
        assert_eq!(bindings[0].stream_kind.label(), "plain");
        assert_eq!(bindings[0].body_object_index, 94);
        assert_eq!(bindings[0].body_alias_object_index, 150);
        assert_eq!(bindings[0].stream_role, 19);
    }

    #[test]
    fn decode_assigns_distinct_binding_ids_to_repeated_stream_links() {
        let file = prt_with_named_payloads(&[(
            "/Root/UG_PART/UG_PART",
            segment_body_binding_repeated_link_payload(),
        )]);
        let result = NxCodec
            .decode(&mut Cursor::new(file), &DecodeOptions::default())
            .expect("required invariant");
        let namespace = result.ir().native.namespace("nx").expect("NX namespace");
        let links = namespace
            .arena_as::<super::SegmentStreamLink>("segment_stream_links")
            .expect("required invariant");
        assert_eq!(links.len(), 2);
        assert_eq!(links[0].stream_ordinal, links[1].stream_ordinal);
        let bindings = namespace
            .arena_as::<super::SegmentBodyBinding>("segment_body_bindings")
            .expect("required invariant");
        assert_eq!(bindings.len(), 2);
        assert_eq!(bindings[0].id, "nx:segment-body-bindings:binding#0");
        assert_eq!(bindings[1].id, "nx:segment-body-bindings:binding#1");
        assert_eq!(bindings[0].stream_link, links[0].id);
        assert_eq!(bindings[1].stream_link, links[1].id);
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
            assert_eq!(links[0].location.separator_byte_len(), expected_separator);
            assert_eq!(
                links[0].location.section_offset(),
                links[0].location.source_offset() + u64::from(expected_separator)
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
        declaration.push(u8::try_from(role.len() + 1).expect("fixture value fits u8"));
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
        declaration.push(u8::try_from(audit.len() + 1).expect("fixture value fits u8"));
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
            objects: crate::om::header_references::HeaderReferences([None; 4]),
            stable_identity: None,
            source_offset: 2 - u64::from(ordinal),
        };
        let labels = [label(2, "UNITE"), label(1, "EXTRUDE"), label(0, "EXTRUDE")];
        let reference = |operation: &str, body_object_index| FeatureBodyReference {
            ordinal: None,
            id: format!("reference#{body_object_index}"),
            operation_label: operation.to_string(),
            body: crate::om::reference_index::FeatureReferenceToken::from_wire(
                body_object_index,
                &[u8::try_from(body_object_index).expect("fixture value fits u8")],
            )
            .unwrap(),
            source_offset: 0,
        };
        let references = [reference("operation#0", 10), reference("operation#1", 20)];
        let booleans = [FeatureBooleanOperation {
            id: "boolean#0".to_string(),
            operation_label: "operation#2".to_string(),
            kind: FeatureBooleanKind::Unite,
            target: crate::test_support::native_references::boolean_reference(10, 0),
            tools: vec![crate::test_support::native_references::boolean_reference(
                20, 0,
            )],
            source_offset: 0,
        }];

        assert_eq!(
            crate::test_support::with_decode_context(|ctx| super::terminal_feature_body_indices(
                ctx,
                &super::BodyLineageInputs {
                    labels: &labels,
                    references: &references,
                    data_block_uses: &[],
                    data_blocks: &[],
                    booleans: &booleans,
                    operands: &[],
                    bindings: &[],
                    inputs: &[]
                },
            ))
            .expect("admitted terminal body lineage"),
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
            objects: crate::om::header_references::HeaderReferences([None; 4]),
            stable_identity: None,
            source_offset: 1 - u64::from(ordinal),
        };
        let labels = [label(1), label(0)];
        let boolean = |ordinal: usize, target: u32, tools: Vec<u32>| FeatureBooleanOperation {
            id: format!("boolean#{ordinal}"),
            operation_label: format!("operation#{ordinal}"),
            kind: FeatureBooleanKind::Unite,
            target: crate::test_support::native_references::boolean_reference(
                target,
                cadmpeg_core::decode::u64_from_index(ordinal),
            ),
            tools: tools
                .into_iter()
                .map(|value| crate::test_support::native_references::boolean_reference(value, 0))
                .collect(),
            source_offset: cadmpeg_core::decode::u64_from_index(ordinal),
        };
        let booleans = [boolean(0, 20, vec![10]), boolean(1, 10, vec![20])];
        let bindings = [
            SegmentBodyBinding {
                id: "binding#0".to_string(),
                stream_link: "stream#0".to_string(),
                stream_ordinal: 0,
                stream_kind: crate::parasolid::StreamKind::Partition,
                body_object_index: 10,
                body_alias_object_index: 11,
                stream_role: 19,
                source_offset: 0,
            },
            SegmentBodyBinding {
                id: "binding#1".to_string(),
                stream_link: "stream#1".to_string(),
                stream_ordinal: 1,
                stream_kind: crate::parasolid::StreamKind::Partition,
                body_object_index: 20,
                body_alias_object_index: 21,
                stream_role: 19,
                source_offset: 1,
            },
        ];

        assert_eq!(
            crate::test_support::with_decode_context(|ctx| super::terminal_feature_body_indices(
                ctx,
                &super::BodyLineageInputs {
                    labels: &labels,
                    references: &[],
                    data_block_uses: &[],
                    data_blocks: &[],
                    booleans: &booleans,
                    operands: &[],
                    bindings: &bindings,
                    inputs: &[]
                },
            ))
            .expect("admitted terminal body lineage"),
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
            objects: crate::om::header_references::HeaderReferences([None; 4]),
            stable_identity: None,
            source_offset: u64::from(ordinal),
        };
        let labels = [label(2, "EXTRUDE"), label(1, "UNITE"), label(0, "UNITE")];
        let references = [FeatureBodyReference {
            ordinal: None,
            id: "reference#10".to_string(),
            operation_label: "operation#2".to_string(),
            body: crate::om::reference_index::FeatureReferenceToken::from_wire(10, &[10]).unwrap(),
            source_offset: 2,
        }];
        let boolean = |ordinal: usize, target: u32, tools: Vec<u32>| FeatureBooleanOperation {
            id: format!("boolean#{ordinal}"),
            operation_label: format!("operation#{ordinal}"),
            kind: FeatureBooleanKind::Unite,
            target: crate::test_support::native_references::boolean_reference(
                target,
                cadmpeg_core::decode::u64_from_index(ordinal),
            ),
            tools: tools
                .into_iter()
                .map(|value| crate::test_support::native_references::boolean_reference(value, 0))
                .collect(),
            source_offset: cadmpeg_core::decode::u64_from_index(ordinal),
        };
        let booleans = [boolean(0, 10, Vec::new()), boolean(1, 20, vec![10])];
        let bindings = [
            SegmentBodyBinding {
                id: "binding#0".to_string(),
                stream_link: "stream#0".to_string(),
                stream_ordinal: 0,
                stream_kind: crate::parasolid::StreamKind::Partition,
                body_object_index: 10,
                body_alias_object_index: 11,
                stream_role: 19,
                source_offset: 0,
            },
            SegmentBodyBinding {
                id: "binding#1".to_string(),
                stream_link: "stream#1".to_string(),
                stream_ordinal: 1,
                stream_kind: crate::parasolid::StreamKind::Partition,
                body_object_index: 20,
                body_alias_object_index: 21,
                stream_role: 19,
                source_offset: 1,
            },
        ];

        assert_eq!(
            crate::test_support::with_decode_context(|ctx| super::terminal_feature_body_indices(
                ctx,
                &super::BodyLineageInputs {
                    labels: &labels,
                    references: &references,
                    data_block_uses: &[],
                    data_blocks: &[],
                    booleans: &booleans,
                    operands: &[],
                    bindings: &bindings,
                    inputs: &[]
                },
            ))
            .expect("admitted terminal body lineage"),
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
            objects: crate::om::header_references::HeaderReferences([None; 4]),
            stable_identity: None,
            source_offset: 0,
        }];
        let references = [FeatureBodyReference {
            ordinal: None,
            id: "reference#10".to_string(),
            operation_label: "operation#delete".to_string(),
            body: crate::om::reference_index::FeatureReferenceToken::from_wire(10, &[10]).unwrap(),
            source_offset: 0,
        }];
        let bindings = [SegmentBodyBinding {
            id: "binding#0".to_string(),
            stream_link: "stream#0".to_string(),
            stream_ordinal: 0,
            stream_kind: crate::parasolid::StreamKind::Partition,
            body_object_index: 10,
            body_alias_object_index: 11,
            stream_role: 19,
            source_offset: 0,
        }];

        assert_eq!(
            crate::test_support::with_decode_context(|ctx| super::terminal_feature_body_indices(
                ctx,
                &super::BodyLineageInputs {
                    labels: &labels,
                    references: &references,
                    data_block_uses: &[],
                    data_blocks: &[],
                    booleans: &[],
                    operands: &[],
                    bindings: &bindings,
                    inputs: &[]
                },
            ))
            .expect("admitted terminal body lineage"),
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
            objects: crate::om::header_references::HeaderReferences([None; 4]),
            stable_identity: None,
            source_offset: 0,
        }];
        let references = [
            FeatureBodyReference {
                ordinal: None,
                id: "reference#10".to_string(),
                operation_label: labels[0].id.clone(),
                body: crate::om::reference_index::FeatureReferenceToken::from_wire(10, &[10])
                    .unwrap(),
                source_offset: 0,
            },
            FeatureBodyReference {
                ordinal: None,
                id: "reference#20".to_string(),
                operation_label: labels[0].id.clone(),
                body: crate::om::reference_index::FeatureReferenceToken::from_wire(20, &[20])
                    .unwrap(),
                source_offset: 1,
            },
        ];
        let bindings = [
            SegmentBodyBinding {
                id: "binding#0".to_string(),
                stream_link: "stream#0".to_string(),
                stream_ordinal: 0,
                stream_kind: crate::parasolid::StreamKind::Partition,
                body_object_index: 10,
                body_alias_object_index: 11,
                stream_role: 19,
                source_offset: 0,
            },
            SegmentBodyBinding {
                id: "binding#1".to_string(),
                stream_link: "stream#1".to_string(),
                stream_ordinal: 1,
                stream_kind: crate::parasolid::StreamKind::Partition,
                body_object_index: 20,
                body_alias_object_index: 21,
                stream_role: 19,
                source_offset: 1,
            },
        ];

        assert_eq!(
            crate::test_support::with_decode_context(|ctx| super::terminal_feature_body_indices(
                ctx,
                &super::BodyLineageInputs {
                    labels: &labels,
                    references: &references,
                    data_block_uses: &[],
                    data_blocks: &[],
                    booleans: &[],
                    operands: &[],
                    bindings: &bindings,
                    inputs: &[]
                },
            ))
            .expect("admitted terminal body lineage"),
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
            objects: crate::om::header_references::HeaderReferences([None; 4]),
            stable_identity: None,
            source_offset: 0,
        }];
        let references = [FeatureBodyReference {
            ordinal: None,
            id: "reference#10".to_string(),
            operation_label: labels[0].id.clone(),
            body: crate::om::reference_index::FeatureReferenceToken::from_wire(10, &[10]).unwrap(),
            source_offset: 0,
        }];
        let binding = |ordinal, body_object_index, body_alias_object_index| SegmentBodyBinding {
            id: format!("binding#{ordinal}"),
            stream_link: format!("stream#{ordinal}"),
            stream_ordinal: ordinal,
            stream_kind: crate::parasolid::StreamKind::Partition,
            body_object_index,
            body_alias_object_index,
            stream_role: 19,
            source_offset: u64::from(ordinal),
        };
        let bindings = [binding(0, 10, 11), binding(1, 20, 21)];

        let statuses = crate::test_support::with_decode_context(|ctx| {
            super::segment_body_lineage_statuses(
                ctx,
                &super::BodyLineageInputs {
                    labels: &labels,
                    references: &references,
                    data_block_uses: &[],
                    data_blocks: &[],
                    booleans: &[],
                    operands: &[],
                    bindings: &bindings,
                    inputs: &[],
                },
            )
        })
        .expect("admitted segment lineage statuses")
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
            objects: crate::om::header_references::HeaderReferences([None; 4]),
            stable_identity: None,
            source_offset: 0,
        }];
        let references = [FeatureBodyReference {
            ordinal: None,
            id: "reference#11".to_string(),
            operation_label: "operation#delete".to_string(),
            body: crate::om::reference_index::FeatureReferenceToken::from_wire(11, &[11]).unwrap(),
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
            stream_kind: crate::parasolid::StreamKind::Partition,
            body_object_index: 10,
            body_alias_object_index: 11,
            stream_role: 19,
            source_offset: 0,
        }];

        let statuses = crate::test_support::with_decode_context(|ctx| {
            super::segment_body_lineage_statuses(
                ctx,
                &super::BodyLineageInputs {
                    labels: &labels,
                    references: &references,
                    data_block_uses: &data_block_uses,
                    data_blocks: &[],
                    booleans: &[],
                    operands: &[],
                    bindings: &bindings,
                    inputs: &[],
                },
            )
        })
        .expect("admitted segment lineage statuses")
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
            objects: crate::om::header_references::HeaderReferences([None; 4]),
            stable_identity: None,
            source_offset: 0,
        }];
        let references = [FeatureBodyReference {
            ordinal: None,
            id: "reference#11".to_string(),
            operation_label: "operation#delete".to_string(),
            body: crate::om::reference_index::FeatureReferenceToken::from_wire(11, &[11]).unwrap(),
            source_offset: 0,
        }];
        let inputs = [FeatureInputBlock {
            id: "input#3".to_string(),
            operation_label: "operation#delete".to_string(),
            input_slot: crate::om::header_references::HeaderSlot::Zero,
            object: crate::om::reference_index::FeatureReferenceToken::from_wire(3, &[3]).unwrap(),
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
            sha256: cadmpeg_ir::hash::digest::Sha256Digest::digest(&[]),
            stable_identity: None,
            source_entry: String::new(),
            source_offset: 0,
        }];
        let bindings = [SegmentBodyBinding {
            id: "binding#0".to_string(),
            stream_link: "stream#0".to_string(),
            stream_ordinal: 0,
            stream_kind: crate::parasolid::StreamKind::Partition,
            body_object_index: 10,
            body_alias_object_index: 11,
            stream_role: 19,
            source_offset: 0,
        }];

        let statuses = crate::test_support::with_decode_context(|ctx| {
            super::segment_body_lineage_statuses(
                ctx,
                &super::BodyLineageInputs {
                    labels: &labels,
                    references: &references,
                    data_block_uses: &[],
                    data_blocks: &blocks,
                    booleans: &[],
                    operands: &[],
                    bindings: &bindings,
                    inputs: &inputs,
                },
            )
        })
        .expect("admitted segment lineage statuses")
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
            objects: crate::om::header_references::HeaderReferences([None; 4]),
            stable_identity: None,
            source_offset: 0,
        }];
        let booleans = [FeatureBooleanOperation {
            id: "boolean#offset-store".to_string(),
            operation_label,
            kind: FeatureBooleanKind::Unite,
            target: crate::test_support::native_references::boolean_reference(11, 0),
            tools: vec![crate::test_support::native_references::boolean_reference(
                21, 0,
            )],
            source_offset: 0,
        }];
        let block = |ordinal| DataBlock {
            id: format!("nx:om-data-blocks-3:block#{ordinal}"),
            section_ordinal: 3,
            block_ordinal: ordinal,
            role: DataBlockRole::Column,
            section_offset: 0,
            byte_len: 0,
            sha256: cadmpeg_ir::hash::digest::Sha256Digest::digest(&[]),
            stable_identity: None,
            source_entry: String::new(),
            source_offset: 0,
        };
        let blocks = [block(11), block(21)];
        let binding = |ordinal, body, alias| SegmentBodyBinding {
            id: format!("binding#{ordinal}"),
            stream_link: format!("stream#{ordinal}"),
            stream_ordinal: ordinal,
            stream_kind: crate::parasolid::StreamKind::Partition,
            body_object_index: body,
            body_alias_object_index: alias,
            stream_role: 19,
            source_offset: u64::from(ordinal),
        };
        let bindings = [binding(0, 10, 11), binding(1, 20, 21)];

        assert_eq!(
            crate::test_support::with_decode_context(|ctx| super::terminal_feature_body_indices(
                ctx,
                &super::BodyLineageInputs {
                    labels: &labels,
                    references: &[],
                    data_block_uses: &[],
                    data_blocks: &blocks,
                    booleans: &booleans,
                    operands: &[],
                    bindings: &bindings,
                    inputs: &[]
                },
            ))
            .expect("admitted terminal body lineage"),
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
            objects: crate::om::header_references::HeaderReferences([None; 4]),
            stable_identity: None,
            source_offset: 0,
        }];
        let booleans = [FeatureBooleanOperation {
            id: "boolean#unresolved-offset-store".to_string(),
            operation_label,
            kind: FeatureBooleanKind::Unite,
            target: crate::test_support::native_references::boolean_reference(11, 0),
            tools: vec![crate::test_support::native_references::boolean_reference(
                21, 0,
            )],
            source_offset: 0,
        }];
        let blocks = [DataBlock {
            id: "nx:om-data-blocks-3:block#11".to_string(),
            section_ordinal: 3,
            block_ordinal: 11,
            role: DataBlockRole::Column,
            section_offset: 0,
            byte_len: 0,
            sha256: cadmpeg_ir::hash::digest::Sha256Digest::digest(&[]),
            stable_identity: None,
            source_entry: String::new(),
            source_offset: 0,
        }];
        let binding = |ordinal, body, alias| SegmentBodyBinding {
            id: format!("binding#{ordinal}"),
            stream_link: format!("stream#{ordinal}"),
            stream_ordinal: ordinal,
            stream_kind: crate::parasolid::StreamKind::Partition,
            body_object_index: body,
            body_alias_object_index: alias,
            stream_role: 19,
            source_offset: u64::from(ordinal),
        };
        let bindings = [binding(0, 10, 11), binding(1, 20, 21)];

        assert_eq!(
            crate::test_support::with_decode_context(|ctx| super::terminal_feature_body_indices(
                ctx,
                &super::BodyLineageInputs {
                    labels: &labels,
                    references: &[],
                    data_block_uses: &[],
                    data_blocks: &blocks,
                    booleans: &booleans,
                    operands: &[],
                    bindings: &bindings,
                    inputs: &[]
                },
            ))
            .expect("admitted terminal body lineage"),
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
            objects: crate::om::header_references::HeaderReferences([None; 4]),
            stable_identity: None,
            source_offset: u64::from(ordinal),
        };
        // Feature-history labels are newest-first within one section. The raw
        // order places the later writer before the earlier delete.
        let labels = [label(0, "EXTRUDE"), label(1, "DELETE")];
        let reference = |ordinal: u32| FeatureBodyReference {
            ordinal: None,
            id: format!("reference#{ordinal}"),
            operation_label: format!("operation#{ordinal}"),
            body: crate::om::reference_index::FeatureReferenceToken::from_wire(10, &[10]).unwrap(),
            source_offset: u64::from(ordinal),
        };
        let references = [reference(0), reference(1)];
        let bindings = [SegmentBodyBinding {
            id: "binding#0".to_string(),
            stream_link: "stream#0".to_string(),
            stream_ordinal: 0,
            stream_kind: crate::parasolid::StreamKind::Partition,
            body_object_index: 10,
            body_alias_object_index: 11,
            stream_role: 19,
            source_offset: 0,
        }];

        assert_eq!(
            crate::test_support::with_decode_context(|ctx| super::terminal_feature_body_indices(
                ctx,
                &super::BodyLineageInputs {
                    labels: &labels,
                    references: &references,
                    data_block_uses: &[],
                    data_blocks: &[],
                    booleans: &[],
                    operands: &[],
                    bindings: &bindings,
                    inputs: &[]
                },
            ))
            .expect("admitted terminal body lineage"),
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
            objects: crate::om::header_references::HeaderReferences([None; 4]),
            stable_identity: None,
            source_offset: u64::from(ordinal),
        };
        let reference = |ordinal: u32| FeatureBodyReference {
            ordinal: None,
            id: format!("reference#{ordinal}"),
            operation_label: format!("operation#{ordinal}"),
            body: crate::om::reference_index::FeatureReferenceToken::from_wire(10, &[10]).unwrap(),
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
            stream_kind: crate::parasolid::StreamKind::Partition,
            body_object_index: 10,
            body_alias_object_index: 11,
            stream_role: 19,
            source_offset: 0,
        }];

        assert_eq!(
            crate::test_support::with_decode_context(|ctx| super::terminal_feature_body_indices(
                ctx,
                &super::BodyLineageInputs {
                    labels: &labels,
                    references: &references,
                    data_block_uses: &[],
                    data_blocks: &[],
                    booleans: &[],
                    operands: &[],
                    bindings: &bindings,
                    inputs: &[]
                },
            ))
            .expect("admitted terminal body lineage"),
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
            objects: crate::om::header_references::HeaderReferences([None; 4]),
            stable_identity: None,
            source_offset: u64::from(ordinal),
        };
        let labels = [
            label("operation#early", "history#0", 0, "EXTRUDE"),
            label("operation#late", "history#1", 0, "UNITE"),
        ];
        let references = [FeatureBodyReference {
            ordinal: None,
            id: "reference#20".to_string(),
            operation_label: "operation#early".to_string(),
            body: crate::om::reference_index::FeatureReferenceToken::from_wire(20, &[20]).unwrap(),
            source_offset: 0,
        }];
        let booleans = [FeatureBooleanOperation {
            id: "boolean#0".to_string(),
            operation_label: "operation#late".to_string(),
            kind: FeatureBooleanKind::Unite,
            target: crate::test_support::native_references::boolean_reference(10, 1),
            tools: vec![crate::test_support::native_references::boolean_reference(
                20, 1,
            )],
            source_offset: 1,
        }];

        assert_eq!(
            crate::test_support::with_decode_context(|ctx| super::terminal_feature_body_indices(
                ctx,
                &super::BodyLineageInputs {
                    labels: &labels,
                    references: &references,
                    data_block_uses: &[],
                    data_blocks: &[],
                    booleans: &booleans,
                    operands: &[],
                    bindings: &[],
                    inputs: &[]
                },
            ))
            .expect("admitted terminal body lineage"),
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
            objects: crate::om::header_references::HeaderReferences([None; 4]),
            stable_identity: None,
            source_offset: 1 - u64::from(ordinal),
        };
        let labels = [label(1, "UNITE"), label(0, "EXTRUDE")];
        let references = [FeatureBodyReference {
            ordinal: None,
            id: "reference#150".to_string(),
            operation_label: "operation#0".to_string(),
            body: crate::om::reference_index::FeatureReferenceToken::from_wire(150, &[0x80, 150])
                .unwrap(),
            source_offset: 0,
        }];
        let booleans = [FeatureBooleanOperation {
            id: "boolean#0".to_string(),
            operation_label: "operation#1".to_string(),
            kind: FeatureBooleanKind::Unite,
            target: crate::test_support::native_references::boolean_reference(10, 0),
            tools: vec![crate::test_support::native_references::boolean_reference(
                94, 0,
            )],
            source_offset: 0,
        }];
        let bindings = [SegmentBodyBinding {
            id: "binding#0".to_string(),
            stream_link: "stream#0".to_string(),
            stream_ordinal: 0,
            stream_kind: crate::parasolid::StreamKind::Partition,
            body_object_index: 94,
            body_alias_object_index: 150,
            stream_role: 19,
            source_offset: 0,
        }];

        assert_eq!(
            crate::test_support::with_decode_context(|ctx| super::terminal_feature_body_indices(
                ctx,
                &super::BodyLineageInputs {
                    labels: &labels,
                    references: &references,
                    data_block_uses: &[],
                    data_blocks: &[],
                    booleans: &booleans,
                    operands: &[],
                    bindings: &bindings,
                    inputs: &[]
                },
            ))
            .expect("admitted terminal body lineage"),
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
            objects: crate::om::header_references::HeaderReferences([None; 4]),
            stable_identity: None,
            source_offset: 0,
        }];
        let bindings = [SegmentBodyBinding {
            id: "binding#0".to_string(),
            stream_link: "stream#0".to_string(),
            stream_ordinal: 0,
            stream_kind: crate::parasolid::StreamKind::Partition,
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
            operand: crate::om::compact::LocatedCompactIndex {
                atom: crate::om::compact::CompactIndexAtom::from_wire(30, &[30]).unwrap(),
                offset: 0,
            },
            operand_data_block: None,
            segment_body_bindings: vec!["binding#0".to_string()],
        }];
        assert_eq!(
            crate::test_support::with_decode_context(|ctx| super::terminal_feature_body_indices(
                ctx,
                &super::BodyLineageInputs {
                    labels: &labels,
                    references: &[],
                    data_block_uses: &[],
                    data_blocks: &[],
                    booleans: &[],
                    operands: &operands,
                    bindings: &bindings,
                    inputs: &[]
                },
            ))
            .expect("admitted terminal body lineage"),
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
            objects: crate::om::header_references::HeaderReferences([None; 4]),
            stable_identity: None,
            source_offset: 0,
        }];
        let bindings = [SegmentBodyBinding {
            id: "binding#0".to_string(),
            stream_link: "stream#0".to_string(),
            stream_ordinal: 0,
            stream_kind: crate::parasolid::StreamKind::Partition,
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
            operand: crate::om::compact::LocatedCompactIndex {
                atom: crate::om::compact::CompactIndexAtom::from_wire(30, &[30]).unwrap(),
                offset: 0,
            },
            operand_data_block: Some("data-block#0".to_string()),
            segment_body_bindings: Vec::new(),
        }];
        assert_eq!(
            crate::test_support::with_decode_context(|ctx| super::terminal_feature_body_indices(
                ctx,
                &super::BodyLineageInputs {
                    labels: &labels,
                    references: &[],
                    data_block_uses: &[],
                    data_blocks: &[],
                    booleans: &[],
                    operands: &operands,
                    bindings: &bindings,
                    inputs: &[]
                },
            ))
            .expect("admitted terminal body lineage"),
            Some([20, 30].into_iter().collect())
        );
    }

    #[test]
    fn body_alias_roots_refuses_work_limit_at_pending_pop() {
        let bindings = [super::SegmentBodyBinding {
            id: "binding#0".to_string(),
            stream_link: "stream#0".to_string(),
            stream_ordinal: 0,
            stream_kind: crate::parasolid::StreamKind::Partition,
            body_object_index: 10,
            body_alias_object_index: 11,
            stream_role: 19,
            source_offset: 0,
        }];
        let error = crate::test_support::resource_refusal_at(
            &[],
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            "walk NX segment alias pending queue",
            |ctx| super::body_alias_roots(ctx, &bindings),
        );
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
                    && limit.operation == "walk NX segment alias pending queue"
                    && limit.used == limit.limit
                    && limit.additional == 1
        ));
    }
}
