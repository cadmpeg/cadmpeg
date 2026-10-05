// SPDX-License-Identifier: Apache-2.0
//! Exact combine operations and external body identities.

use super::draft::contains_consecutive_guid_pair;
use super::parameter_scope::parameter_scope_payload_length;
use crate::bytes::lp_utf16_bounded_charged;
use crate::bytes::take_reference;
use crate::design::decode::byte_fields::{bytes_at, zeros_at};
use crate::design::decode::reference_runs::reference_position;
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::layout::combine_compact_operation_prefix as combine_compact;
use crate::layout::combine_extended_reference_operation_prefix as combine_extended;
use crate::layout::combine_external_selector_prefix as combine_external;
use crate::layout::combine_standard_operation_prefix as combine_standard;
use crate::records::feature::combine;
use crate::records::feature::combine::DesignCombineBodySelection;
use crate::records::feature::combine::DesignCombineExternalBodyIdentity;
use crate::records::feature::combine::DesignCombineExternalBodyIdentityWire;
use crate::records::feature::combine::DesignCombineForm;
use crate::records::feature::combine::DesignCombineOperation;
use crate::records::feature::scope::{DesignParameterScope, DesignScopePayload};
use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::BooleanKind;

/// Unwrap an `Option`, ending a fallible parse with `Ok(None)` when it is empty.
macro_rules! try_some {
    ($value:expr) => {
        match $value {
            Some(value) => value,
            None => return Ok(None),
        }
    };
}

pub(super) fn exact_combine_operation(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Result<Option<DesignCombineOperation>, CodecError> {
    let references = scope.reference_members();
    if !matches!(scope.payload(), DesignScopePayload::Combine(_))
        || references.len() < 4
        || !references.len().is_multiple_of(2)
    {
        return Ok(None);
    }
    let Ok(start) = usize::try_from(scope.byte_offset()) else {
        return Ok(None);
    };
    let compact = scope.class_tag.as_str() == "387"
        && scope.paired_class_tag.as_str() == "258"
        && parameter_scope_payload_length(ctx, scope)? == Some(314);
    let extended_reference = scope.class_tag.as_str() == "329"
        && scope.paired_class_tag.as_str() == "261"
        && scope.frame_length() == 363;
    let Some(prefix) = combine_prefix(ctx, bytes, start, compact, extended_reference)? else {
        return Ok(None);
    };
    let mut target = None;
    let mut first_tool = None;
    let mut additional_tools = Vec::new();
    // The run alternates operation and selection records; its length is even.
    // The scan stops at the first pair that is no operand or a second target.
    let mut operation_record_index = None;
    let rejected = reference_position(
        ctx,
        references,
        |member| {
            let Some(operation_record_index) = operation_record_index.take() else {
                operation_record_index = Some(*member);
                return Ok(false);
            };
            let selection_record_index = *member;
            let Some((role, selection_at, selection_end)) = combine_operand(
                ctx,
                bytes,
                records,
                operation_record_index,
                selection_record_index,
            )?
            else {
                return Ok(true);
            };
            match role {
                CombineOperandRole::Target => {
                    if target.replace(selection_record_index).is_some() {
                        return Ok(true);
                    }
                }
                CombineOperandRole::Tool => {
                    let selection = DesignCombineBodySelection {
                        record_index: selection_record_index,
                        external_identity: exact_combine_external_body_identity(
                            ctx,
                            bytes,
                            selection_at,
                            selection_end,
                            scope.record_index,
                            selection_record_index,
                        )?,
                    };
                    if first_tool.is_none() {
                        first_tool = Some(selection);
                    } else {
                        ctx.push_vec(
                            &mut additional_tools,
                            selection,
                            "f3d Combine additional tools",
                        )?;
                    }
                }
            }
            Ok(false)
        },
        "scan F3D combine scope references",
    )?;
    if rejected.is_some() {
        return Ok(None);
    }
    let (Some(target_record_index), Some(first)) = (target, first_tool) else {
        return Ok(None);
    };
    let CombinePrefix {
        form,
        operation,
        operation_offset,
        keep_tools,
        keep_tools_offset,
    } = prefix;
    Ok(Some(DesignCombineOperation {
        form,
        operation,
        operation_offset,
        keep_tools,
        keep_tools_offset,
        target_record_index,
        tools: combine::DesignCombineTools {
            first,
            additional: additional_tools,
        },
    }))
}

/// The fixed operation prefix of a Combine scope frame.
struct CombinePrefix {
    form: DesignCombineForm,
    operation: BooleanKind,
    operation_offset: u64,
    keep_tools: bool,
    keep_tools_offset: u64,
}

fn combine_prefix(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    start: usize,
    compact: bool,
    extended_reference: bool,
) -> Result<Option<CombinePrefix>, CodecError> {
    let (form, operation_offset, keep_tools_offset) = if compact {
        if !zeros_at::<10>(bytes, start + combine_compact::ZERO_RUN_10)
            || !zeros_at::<3>(bytes, start + combine_compact::ZERO_RUN_3)
            || bytes_at::<2>(bytes, start + combine_compact::REFERENCE_FORM) != Some(&[1, 0])
            || View::u32_le_at(bytes, start + combine_compact::CONSTANT_ONE) != Some(1)
            || bytes.get(start + combine_compact::REFERENCE_MARKER) != Some(&1)
            || View::u64_le_at(bytes, start + combine_compact::REFERENCE_VALUE) == Some(0)
            || !zeros_at::<{ combine_compact::LEN - combine_compact::REFERENCE_TAIL }>(
                bytes,
                start + combine_compact::REFERENCE_TAIL,
            )
        {
            return Ok(None);
        }
        (
            DesignCombineForm::Compact,
            start + combine_compact::OPERATION,
            start + combine_compact::KEEP_TOOLS,
        )
    } else if extended_reference {
        let mut reference_at = try_some!(start.checked_add(combine_extended::REFERENCE_MARKER));
        let reference = try_some!(take_reference(ctx, bytes, &mut reference_at)?);
        if !zeros_at::<18>(bytes, start + combine_extended::ZERO_RUN_18)
            || bytes.get(start + combine_extended::FORM_MARKER) != Some(&1)
            || reference.local().is_none_or(|(target, _)| target == 0)
            || reference_at != try_some!(start.checked_add(combine_extended::LEN))
        {
            return Ok(None);
        }
        (
            DesignCombineForm::ExtendedReference,
            start + combine_extended::OPERATION,
            start + combine_extended::KEEP_TOOLS,
        )
    } else {
        if !zeros_at::<9>(bytes, start + combine_standard::ZERO_RUN_9)
            || bytes.get(start + combine_standard::ZERO_FLAG) != Some(&0)
            || !zeros_at::<{ combine_standard::LEN - combine_standard::ZERO_RUN_7 }>(
                bytes,
                start + combine_standard::ZERO_RUN_7,
            )
        {
            return Ok(None);
        }
        (
            DesignCombineForm::Standard,
            start + combine_standard::OPERATION,
            start + combine_standard::KEEP_TOOLS,
        )
    };
    let operation = match try_some!(View::u32_le_at(bytes, operation_offset)) {
        1 => BooleanKind::Join,
        2 => BooleanKind::Cut,
        3 => BooleanKind::Intersect,
        _ => return Ok(None),
    };
    let keep_tools = match try_some!(bytes.get(keep_tools_offset)) {
        0 => false,
        1 => true,
        _ => return Ok(None),
    };
    Ok(Some(CombinePrefix {
        form,
        operation,
        operation_offset: try_some!(u64::try_from(operation_offset).ok()),
        keep_tools,
        keep_tools_offset: try_some!(u64::try_from(keep_tools_offset).ok()),
    }))
}

/// The role of the selection named by one operation/selection reference
/// pair, with the selection frame. Both records have exactly one frame, the
/// selection frame carries two consecutive GUIDs, and the operation frame names
/// the selection as target or tool.
fn combine_operand(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    operation_record_index: u32,
    selection_record_index: u32,
) -> Result<Option<(CombineOperandRole, usize, usize)>, CodecError> {
    let Some(role) = records
        .only_frame(operation_record_index)
        .and_then(|(start, end)| bytes.get(start..end))
        .and_then(|frame| combine_operation_identity_role(frame, selection_record_index))
    else {
        return Ok(None);
    };
    let Some((selection_at, selection_end)) = records.only_frame(selection_record_index) else {
        return Ok(None);
    };
    let Some(selection_frame) = bytes.get(selection_at..selection_end) else {
        return Ok(None);
    };
    if !contains_consecutive_guid_pair(ctx, selection_frame)? {
        return Ok(None);
    }
    Ok(Some((role, selection_at, selection_end)))
}

pub(super) struct ExternalReferenceIdentity {
    pub(super) target: u64,
    pub(super) target_offset: u64,
    pub(super) segment: u32,
    pub(super) segment_offset: u64,
    pub(super) asset_id: crate::records::mesh::DesignRelaxedGuidText,
    pub(super) asset_id_offset: u64,
    pub(super) link_name: String,
    pub(super) link_name_offset: u64,
    pub(super) version: Option<combine::DesignExternalVersion>,
}

pub(super) fn take_external_reference_identity(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    cursor: &mut usize,
) -> Result<Option<ExternalReferenceIdentity>, CodecError> {
    let Some((target, target_at, segment, segment_at)) = external_reference_target(bytes, *cursor)
    else {
        return Ok(None);
    };
    let Some(asset_at) = segment_at.checked_add(4) else {
        return Ok(None);
    };
    let Some((asset_id, after_asset_id)) =
        lp_utf16_bounded_charged(ctx, bytes, asset_at, 1..=256, "f3d Design UTF-16 text")?
    else {
        return Ok(None);
    };
    let Ok(asset_id) = crate::records::mesh::DesignRelaxedGuidText::try_from(asset_id) else {
        return Ok(None);
    };
    if bytes.get(after_asset_id) != Some(&0) {
        return Ok(None);
    }
    let Some(link_name_at) = after_asset_id.checked_add(1) else {
        return Ok(None);
    };
    let Some((link_name, after_link_name)) =
        lp_utf16_bounded_charged(ctx, bytes, link_name_at, 1..=256, "f3d Design UTF-16 text")?
    else {
        return Ok(None);
    };
    let (version, end) = match bytes.get(after_link_name) {
        Some(0) => (None, after_link_name + 1),
        Some(1) => {
            let property_key_at = after_link_name + 1;
            let Some((property_key, version_urn_at)) = lp_utf16_bounded_charged(
                ctx,
                bytes,
                property_key_at,
                1..=256,
                "f3d Design UTF-16 text",
            )?
            else {
                return Ok(None);
            };
            let Some((version_urn, end)) = lp_utf16_bounded_charged(
                ctx,
                bytes,
                version_urn_at,
                1..=256,
                "f3d Design UTF-16 text",
            )?
            else {
                return Ok(None);
            };
            let Ok(property_key) =
                crate::records::mesh::DesignRelaxedGuidText::try_from(property_key)
            else {
                return Ok(None);
            };
            (
                Some(combine::DesignExternalVersion {
                    property_key: crate::records::identity::Located {
                        value: property_key,
                        offset: u64_from_index(property_key_at + 4),
                    },
                    version_urn: crate::records::identity::Located {
                        value: version_urn,
                        offset: u64_from_index(version_urn_at + 4),
                    },
                }),
                end,
            )
        }
        _ => return Ok(None),
    };
    *cursor = end;
    Ok(Some(ExternalReferenceIdentity {
        target,
        target_offset: u64_from_index(target_at),
        segment,
        segment_offset: u64_from_index(segment_at),
        asset_id,
        asset_id_offset: u64_from_index(asset_at + 4),
        link_name,
        link_name_offset: u64_from_index(link_name_at + 4),
        version,
    }))
}

/// The marked nonzero external target and its segment at `at`, with their
/// offsets.
fn external_reference_target(bytes: &[u8], at: usize) -> Option<(u64, usize, u32, usize)> {
    if bytes.get(at) != Some(&1) {
        return None;
    }
    let target_at = at.checked_add(1)?;
    let target = View::u64_le_at(bytes, target_at)?;
    if target == 0 || bytes.get(target_at.checked_add(8)?) != Some(&1) {
        return None;
    }
    let segment_at = target_at.checked_add(9)?;
    Some((
        target,
        target_at,
        View::u32_le_at(bytes, segment_at)?,
        segment_at,
    ))
}

fn exact_combine_external_body_identity(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    start: usize,
    paired_at: usize,
    scope_record_index: u32,
    record_index: u32,
) -> Result<Option<DesignCombineExternalBodyIdentity>, CodecError> {
    let Some(selector_asset_at) = external_selector_prefix(ctx, bytes, start, record_index)? else {
        return Ok(None);
    };
    let Some((selector_asset_id, selector_context_at)) = lp_utf16_bounded_charged(
        ctx,
        bytes,
        selector_asset_at,
        1..=256,
        "f3d Design UTF-16 text",
    )?
    else {
        return Ok(None);
    };
    let Some((selector_context_id, after_selector_context_id)) = lp_utf16_bounded_charged(
        ctx,
        bytes,
        selector_context_at,
        1..=256,
        "f3d Design UTF-16 text",
    )?
    else {
        return Ok(None);
    };
    let (Ok(selector_asset_id), Ok(selector_context_id)) = (
        crate::records::mesh::DesignRelaxedGuidText::try_from(selector_asset_id),
        crate::records::mesh::DesignRelaxedGuidText::try_from(selector_context_id),
    ) else {
        return Ok(None);
    };
    let Some((occurrence_reference, occurrence_reference_at, mut cursor)) =
        external_occurrence(ctx, bytes, after_selector_context_id)?
    else {
        return Ok(None);
    };
    let Some(external) = take_external_reference_identity(ctx, bytes, &mut cursor)? else {
        return Ok(None);
    };
    let Some(tail) = external_body_tail(
        ctx,
        bytes,
        cursor,
        paired_at,
        scope_record_index,
        record_index,
    )?
    else {
        return Ok(None);
    };
    let (
        external_property_key,
        external_property_key_offset,
        external_version_urn,
        external_version_urn_offset,
    ) = match external.version {
        Some(version) => (
            Some(version.property_key.value),
            Some(version.property_key.offset),
            Some(version.version_urn.value),
            Some(version.version_urn.offset),
        ),
        None => (None, None, None, None),
    };
    Ok(
        DesignCombineExternalBodyIdentity::try_from(DesignCombineExternalBodyIdentityWire {
            selector_asset_id,
            selector_asset_id_offset: u64_from_index(selector_asset_at + 4),
            selector_context_id,
            selector_context_id_offset: u64_from_index(selector_context_at + 4),
            occurrence_reference,
            occurrence_reference_offset: u64_from_index(occurrence_reference_at),
            external_body_reference: external.target,
            external_body_reference_offset: external.target_offset,
            external_segment: external.segment,
            external_segment_offset: external.segment_offset,
            external_asset_id: external.asset_id,
            external_asset_id_offset: external.asset_id_offset,
            external_link_name: external.link_name,
            external_link_name_offset: external.link_name_offset,
            external_property_key,
            external_property_key_offset,
            external_version_urn,
            external_version_urn_offset,
            tail_values: tail.values,
            tail_value_offsets: tail.offsets,
        })
        .ok(),
    )
}

/// The selector asset-ID offset after the fixed selector prefix of a Combine
/// tool selection frame at `start`, whose nested reference names the record
/// three past `record_index`.
fn external_selector_prefix(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    start: usize,
    record_index: u32,
) -> Result<Option<usize>, CodecError> {
    if !zeros_at::<14>(bytes, start + combine_external::ZERO_RUN_14) {
        return Ok(None);
    }
    let mut cursor = try_some!(start.checked_add(combine_external::NESTED_REFERENCE_MARKER));
    let nested = try_some!(take_reference(ctx, bytes, &mut cursor)?);
    if try_some!(nested.local()).0 != u64::from(try_some!(record_index.checked_add(3)))
        || try_some!(View::u32_le_at(bytes, cursor)) != 1
    {
        return Ok(None);
    }
    Ok(cursor.checked_add(4))
}

/// The nonzero occurrence reference after the selector GUIDs at `at`, its
/// offset, and the offset of the external reference that follows it.
fn external_occurrence(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    at: usize,
) -> Result<Option<(u64, usize, usize)>, CodecError> {
    if try_some!(View::u32_le_at(bytes, at)) != 2
        || try_some!(View::u32_le_at(bytes, try_some!(at.checked_add(4)))) != 0
        || try_some!(View::u32_le_at(bytes, try_some!(at.checked_add(8)))) != 1
    {
        return Ok(None);
    }
    let mut cursor = try_some!(at.checked_add(12));
    let occurrence_reference_at = try_some!(cursor.checked_add(1));
    let occurrence = try_some!(take_reference(ctx, bytes, &mut cursor)?);
    let (occurrence_reference, _) = try_some!(occurrence.local());
    if occurrence_reference == 0 || try_some!(View::u32_le_at(bytes, cursor)) != 1 {
        return Ok(None);
    }
    Ok(Some((
        occurrence_reference,
        occurrence_reference_at,
        try_some!(cursor.checked_add(4)),
    )))
}

struct ExternalBodyTail {
    values: [u64; 2],
    offsets: [u64; 2],
}

/// The two tail values after the external reference at `at` and the closing
/// local references, which end exactly at `paired_at`.
fn external_body_tail(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    at: usize,
    paired_at: usize,
    scope_record_index: u32,
    record_index: u32,
) -> Result<Option<ExternalBodyTail>, CodecError> {
    if try_some!(View::u32_le_at(bytes, at)) != 9
        || try_some!(View::u16_le_at(bytes, try_some!(at.checked_add(4)))) != 2
    {
        return Ok(None);
    }
    let first_at = try_some!(at.checked_add(6));
    let first = try_some!(View::u64_le_at(bytes, first_at));
    if try_some!(View::u32_le_at(bytes, try_some!(first_at.checked_add(8)))) != 48 {
        return Ok(None);
    }
    let second_at = try_some!(first_at.checked_add(12));
    let second = try_some!(View::u64_le_at(bytes, second_at));
    let mut cursor = try_some!(second_at.checked_add(8));
    let take_local = |cursor: &mut usize, expected| -> Result<Option<()>, CodecError> {
        let reference = try_some!(take_reference(ctx, bytes, cursor)?);
        Ok((try_some!(reference.local()).0 == u64::from(expected)).then_some(()))
    };
    try_some!(take_local(
        &mut cursor,
        try_some!(record_index.checked_add(2))
    )?);
    if !zeros_at::<2>(bytes, cursor) {
        return Ok(None);
    }
    cursor = try_some!(cursor.checked_add(2));
    try_some!(take_local(
        &mut cursor,
        try_some!(record_index.checked_add(1))
    )?);
    if bytes.get(cursor) != Some(&0) {
        return Ok(None);
    }
    cursor = try_some!(cursor.checked_add(1));
    try_some!(take_local(&mut cursor, scope_record_index)?);
    Ok((cursor == paired_at).then_some(ExternalBodyTail {
        values: [first, second],
        offsets: [u64_from_index(first_at), u64_from_index(second_at)],
    }))
}

#[derive(Clone, Copy)]
enum CombineOperandRole {
    Target,
    Tool,
}

fn combine_operation_identity_role(
    frame: &[u8],
    selection_record_index: u32,
) -> Option<CombineOperandRole> {
    let selection_reference = selection_record_index.to_le_bytes();
    if zeros_at::<10>(frame, 11)
        && View::u32_le_at(frame, 21)? == 1
        && frame.get(25) == Some(&1)
        && bytes_at::<4>(frame, 26)? == &selection_reference
        && zeros_at::<6>(frame, 30)
    {
        return Some(CombineOperandRole::Target);
    }
    if !zeros_at::<9>(frame, 11) || frame.get(20) != Some(&1) || View::u32_le_at(frame, 21)? != 1 {
        return None;
    }
    let after_property = lp_ascii_literal(frame, 25, b"DcFeatureOperationIdFlag")?;
    let after_property_type = lp_ascii_literal(frame, after_property, b"IntrinsicMetaTypeuint64")?;
    let count_at = after_property_type.checked_add(8)?;
    if View::u32_le_at(frame, count_at)? != 1
        || frame.get(count_at + 4) != Some(&1)
        || bytes_at::<4>(frame, count_at + 5)? != &selection_reference
        || !zeros_at::<6>(frame, count_at + 9)
    {
        return None;
    }
    Some(CombineOperandRole::Tool)
}

fn lp_ascii_literal<const N: usize>(bytes: &[u8], at: usize, literal: &[u8; N]) -> Option<usize> {
    if View::u32_le_at(bytes, at)? != u32::try_from(N).ok()? {
        return None;
    }
    let start = at.checked_add(4)?;
    (bytes_at::<N>(bytes, start)? == literal).then_some(start + N)
}
