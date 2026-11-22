// SPDX-License-Identifier: Apache-2.0
//! Extrude selection groups, members, and identity frames.

use super::construction_groups::take_record_reference;
use super::face_sources::indexed_operand_headers;
use crate::bytes::{is_guid_relaxed, lp_ascii_filtered_view, lp_utf16_bounded};
use crate::container::ContainerScan;
use crate::design::decode::text::design_record_id;
use crate::design::{design_feature_family, DesignFeatureFamily};
use crate::ids::native_stream;
use crate::layout::{
    extrude_selection_member_fixed_frame as extrude_member,
    typed_reference_extrude_selection_group_prefix as typed_group,
};
use crate::records::decal::DesignRecordHeader;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::sketch_geometry::{SketchCurveIdentity, SketchPoint};
use crate::records::sketch_relations::SketchRelationOperand;
use crate::records::topology::construction::DesignConstructionOperandIdentity;
use crate::records::topology::extrude_selection::{
    DesignExtrudeSelectionGroup, DesignExtrudeSelectionMember, DesignExtrudeSelectionScopeTail,
};
use cadmpeg_core::container::ContainerRole;
use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;
use std::collections::{HashMap, HashSet};

/// Decode compact counted selections in Extrude and Emboss scopes.
pub(crate) fn decode_extrude_selection_groups(
    scan: &ContainerScan,
    scopes: &[DesignParameterScope],
    headers: &[DesignRecordHeader],
) -> Result<Vec<DesignExtrudeSelectionGroup>, CodecError> {
    let headers = indexed_operand_headers(headers);
    let mut out = Vec::new();
    for scope in scopes.iter().filter(|scope| {
        design_feature_family(&scope.kind()) == Some(DesignFeatureFamily::Extrude)
            || scope.emboss_operand_construction().is_some()
    }) {
        let Some(stream) = native_stream(&scope.id) else {
            continue;
        };
        let Some(entry) = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, stream)
        else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        for (ordinal, record_index) in scope.reference_members().values().copied().enumerate() {
            if scope.emboss_operand_construction().is_some() && ordinal != 8 {
                continue;
            }
            let Ok(ordinal) = u32::try_from(ordinal) else {
                continue;
            };
            let Some(header) = headers.get(&(stream, record_index)) else {
                continue;
            };
            if let Some(group) = parse_extrude_selection_group(bytes, scope, ordinal, header)? {
                push_extrude_selection_group(&mut out, group, &entry.name, header.byte_offset);
            }
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

pub(super) fn push_extrude_selection_group(
    out: &mut Vec<DesignExtrudeSelectionGroup>,
    mut group: DesignExtrudeSelectionGroup,
    stream: &str,
    offset: u64,
) {
    group.id = design_record_id(stream, ":design-extrude-selection-group#", offset);
    out.push(group);
}

pub(super) fn parse_extrude_selection_group(
    bytes: &[u8],
    scope: &DesignParameterScope,
    scope_reference_ordinal: u32,
    header: &DesignRecordHeader,
) -> Result<Option<DesignExtrudeSelectionGroup>, CodecError> {
    let wire = (|| -> Option<
        Result<
            crate::records::topology::extrude_selection::DesignExtrudeSelectionGroupWire,
            CodecError,
        >,
    > {
        let start = usize::try_from(header.byte_offset).ok()?;
        let scope_reference_at = start + typed_group::SCOPE_REFERENCE;
        if bytes.get(start + typed_group::ZERO_RUN_10..scope_reference_at)? != [0; 10] {
            return None;
        }
        // Every reference of one group uses one spelling; the scope
        // reference selects it.
        let typed = crate::bytes::typed_local_reference_target(bytes, scope_reference_at).is_some();
        let width = if typed {
            crate::bytes::TYPED_LOCAL_REFERENCE_LEN
        } else {
            11
        };
        let reference = |at: usize| crate::bytes::local_reference_target(bytes, at, typed);
        if reference(scope_reference_at)? != scope.record_index {
            return None;
        }
        let member_count_at = scope_reference_at + width;
        let member_count = usize::try_from(View::u32_le_at(bytes, member_count_at)?).ok()?;
        let mut position = member_count_at.checked_add(4)?;
        // A count the remaining bytes cannot supply is corrupt and must not
        // reach the allocator.
        if member_count == 0 || member_count > bytes.len().checked_sub(position)? / width {
            return None;
        }
        let mut members = Vec::new();
        let mut member_offsets = Vec::new();
        for _ in 0..member_count {
            members.push(reference(position)?);
            member_offsets.push(u64::try_from(position + 1).ok()?);
            position = position.checked_add(width)?;
        }
        let opaque_index = View::u32_le_at(bytes, position)?;
        let opaque_scalar = View::f64_le_at(bytes, position + 4)?;
        if View::u32_le_at(bytes, position + 12)? != opaque_index
            || reference(position + 16)? != header.record_index.checked_add(2)?
        {
            return None;
        }
        // Two zero bytes or, in the plain form, `01 + u8 variant + 00`
        // follow the first nested reference.
        let after_first = position + 16 + width;
        let (variant, second_at) = match bytes.get(after_first..after_first + 3)? {
            [0, 0, _] => (None, after_first + 2),
            [1, variant @ (0 | 1), 0] if !typed => (Some(*variant != 0), after_first + 3),
            _ => return None,
        };
        let scope_at = second_at + width + 1;
        if reference(second_at)? != header.record_index.checked_add(1)?
            || bytes.get(second_at + width) != Some(&0)
            || reference(scope_at)? != scope.record_index
        {
            return None;
        }
        let paired_at = scope_at.checked_add(width)?;
        let (paired_class_tag, after_paired_tag) =
            lp_ascii_filtered_view(bytes, paired_at, 0..=2000, u8::is_ascii_graphic)?;
        if View::u32_le_at(bytes, after_paired_tag)? != header.record_index {
            return None;
        }
        let paired_class_tag =
            crate::design::decode::text::class_tag_from_view(paired_class_tag).ok()?;
        Some(Ok(
            crate::records::topology::extrude_selection::DesignExtrudeSelectionGroupWire {
                id: String::new(),
                scope_record_index: scope.record_index,
                scope_reference_ordinal,
                record_index: header.record_index,
                byte_offset: header.byte_offset,
                class_tag: header.class_tag.clone().into(),
                member_count_offset: u64::try_from(member_count_at).ok()?,
                members,
                member_offsets,
                opaque_index,
                opaque_index_offset: u64::try_from(position).ok()?,
                opaque_scalar,
                opaque_scalar_offset: u64::try_from(position + 4).ok()?,
                variant,
                typed_references: typed,
                paired_class_tag: paired_class_tag.into(),
                paired_byte_offset: u64::try_from(paired_at).ok()?,
            },
        ))
    })();
    let Some(wire) = wire else {
        return Ok(None);
    };
    Ok(DesignExtrudeSelectionGroup::try_from(wire?).ok())
}

/// Decode the fixed-width records named by Extrude selection groups.
pub(crate) fn decode_extrude_selection_members(
    scan: &ContainerScan,
    scopes: &[DesignParameterScope],
    groups: &[DesignExtrudeSelectionGroup],
    headers: &[DesignRecordHeader],
) -> Result<Vec<DesignExtrudeSelectionMember>, CodecError> {
    let headers = indexed_operand_headers(headers);
    let extrude_scopes = scopes
        .iter()
        .filter_map(|scope| {
            (design_feature_family(&scope.kind()) == Some(DesignFeatureFamily::Extrude))
                .then(|| native_stream(&scope.id).map(|stream| (stream, scope.record_index)))
                .flatten()
        })
        .collect::<HashSet<_>>();
    let mut out = Vec::new();
    for group in groups {
        let Some(stream) = native_stream(&group.id) else {
            continue;
        };
        if !extrude_scopes.contains(&(stream, group.scope_record_index)) {
            continue;
        }
        let Some(entry) = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, stream)
        else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        for (ordinal, record_index) in group
            .members()
            .iter()
            .map(|member| member.value)
            .enumerate()
        {
            let Ok(ordinal) = u32::try_from(ordinal) else {
                continue;
            };
            let Some(header) = headers.get(&(stream, record_index)) else {
                continue;
            };
            if let Some(mut member) = parse_extrude_selection_member(bytes, group, ordinal, header)
            {
                member.id = design_record_id(
                    &entry.name,
                    ":design-extrude-selection-member#",
                    header.byte_offset,
                );

                out.push(member);
            }
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

/// Resolve selection-member local identities against persistent point and
/// curve identities owned by the Extrude scope's selected Sketch.
pub(crate) fn bind_extrude_selection_geometry(
    members: &mut [DesignExtrudeSelectionMember],
    groups: &[DesignExtrudeSelectionGroup],
    scopes: &[DesignParameterScope],
    points: &[SketchPoint],
    curves: &[SketchCurveIdentity],
) {
    let mut selected_sketches = HashMap::new();
    for group in groups {
        let Some(stream) = native_stream(&group.id) else {
            continue;
        };
        let Some(scope) = scopes.iter().find(|scope| {
            native_stream(&scope.id) == Some(stream)
                && scope.record_index == group.scope_record_index
        }) else {
            continue;
        };
        let Some(profile) = scope.extrude_profile() else {
            continue;
        };
        let key = (stream, group.record_index);
        selected_sketches.insert(key, profile.entity_id.suffix());
    }
    for member in members {
        let Some(stream) = native_stream(&member.id) else {
            continue;
        };
        let Some(entity_suffix) = selected_sketches.get(&(stream, member.group_record_index))
        else {
            continue;
        };
        let Ok(entity_suffix) = u32::try_from(*entity_suffix) else {
            continue;
        };
        let point_operands = points.iter().filter_map(|point| {
            (native_stream(&point.id) == Some(stream)
                && (point.owner_reference.is_none()
                    || point.owner_reference == Some(entity_suffix))
                && point.persistent_id() == Some(member.local_id))
            .then_some(SketchRelationOperand::Point {
                record_index: point.record_index,
                persistent_id: point.persistent_id(),
            })
        });
        let curve_operands = curves.iter().filter_map(|curve| {
            (native_stream(&curve.id) == Some(stream)
                && (curve.owner_reference.is_none()
                    || curve.owner_reference == Some(entity_suffix))
                && (curve.primary_id.get() == member.local_id
                    || curve.secondary_id != 0 && curve.secondary_id == member.local_id))
                .then_some(SketchRelationOperand::Curve {
                    record_index: curve.record_index,
                    primary_id: curve.primary_id.get(),
                    secondary_id: curve.secondary_id,
                })
        });
        let mut matches = point_operands.chain(curve_operands);
        if let (Some(resolved), None) = (matches.next(), matches.next()) {
            member.resolved_geometry = Some(resolved);
        } else {
            member.resolved_geometry = None;
        }
    }
}

/// Bind selection members to construction-operand identity chains that
/// terminate at the same fixed persistent identity record.
pub(crate) fn bind_extrude_selection_identities(
    members: &mut [DesignExtrudeSelectionMember],
    identities: &[DesignConstructionOperandIdentity],
) {
    for member in members {
        let Some(stream) = native_stream(&member.id) else {
            continue;
        };
        let mut matches = identities
            .iter()
            .filter(|identity| {
                native_stream(&identity.id) == Some(stream)
                    && identity.following_record_index() == member.record_index()
                    && identity.following_byte_offset() == member.byte_offset()
                    && identity.persistent_identity().is_some_and(|persistent| {
                        persistent.local_id == member.local_id
                            && persistent.asset_id == member.asset_id
                            && persistent.context_id == member.context_id
                    })
            })
            .collect::<Vec<_>>();
        matches.sort_by_key(|identity| {
            identity
                .wrappers()
                .first()
                .map(|wrapper| wrapper.byte_offset)
        });
        member.operand_identity_ids = matches
            .into_iter()
            .map(|identity| identity.id.clone())
            .collect();
    }
}

pub(super) fn parse_extrude_selection_member(
    bytes: &[u8],
    group: &DesignExtrudeSelectionGroup,
    group_member_ordinal: u32,
    header: &DesignRecordHeader,
) -> Option<DesignExtrudeSelectionMember> {
    let start = usize::try_from(header.byte_offset).ok()?;
    let member = parse_extrude_identity_member(bytes, start, Some(group.scope_record_index))?;
    DesignExtrudeSelectionMember::try_new(
        crate::records::topology::extrude_selection::DesignExtrudeSelectionMemberDraft {
            id: String::new(),
            group_record_index: group.record_index,
            group_member_ordinal,
            record_index: header.record_index,
            byte_offset: header.byte_offset,
            class_tag: header.class_tag.clone(),
            local_id: member.local_id,
            local_id_offset: member.local_id_offset,
            asset_id: member.asset_id.try_into().ok()?,
            asset_id_offset: member.asset_id_offset,
            context_id: member.context_id.try_into().ok()?,
            context_id_offset: member.context_id_offset,
            tail_slot_present: member.tail_slot_present,
            tail_slot_offset: member.tail_slot_offset,
            scope_tail: member.scope_tail,
            resolved_geometry: None,
            operand_identity_ids: Vec::new(),
            historical: None,
            next_record_index: member.next_record_index,
            next_byte_offset: member.next_byte_offset,
        },
    )
    .ok()
}

/// Width of a same-segment marked reference.
pub(super) const MARKED_REFERENCE_LEN: usize = 11;
/// Bytes between the two chain references of a selection-member scope tail.
pub(super) const EXTRUDE_SCOPE_TAIL_SEPARATOR: [u8; 3] = [1, 0, 0];

pub(super) struct ParsedExtrudeIdentityMember {
    pub(super) local_id: u64,
    pub(super) local_id_offset: u64,
    pub(super) asset_id: String,
    pub(super) asset_id_offset: u64,
    pub(super) context_id: String,
    pub(super) context_id_offset: u64,
    pub(super) tail_slot_present: bool,
    pub(super) tail_slot_offset: u64,
    pub(super) scope_tail: Option<DesignExtrudeSelectionScopeTail>,
    pub(super) next_record_index: u32,
    pub(super) next_byte_offset: u64,
}

/// Parse the fifty-byte scope tail that follows a 190-byte member frame.
///
/// Returns the tail and the offset where the tail ends. The closing marked
/// reference has to name `scope_record_index`.
pub(super) fn parse_extrude_selection_scope_tail(
    bytes: &[u8],
    tail_start: usize,
    scope_record_index: u32,
) -> Option<(DesignExtrudeSelectionScopeTail, usize)> {
    use crate::layout::extrude_selection_member_scope_tail as tail;
    let tail_end = tail_start.checked_add(tail::LEN)?;
    bytes.get(tail_start..tail_end)?;
    let marked_reference = |offset: usize| -> Option<u32> {
        let mut cursor = tail_start.checked_add(offset)?;
        let (record_index, _) = take_record_reference(bytes, &mut cursor)?;
        (cursor
            == tail_start
                .checked_add(offset)?
                .checked_add(MARKED_REFERENCE_LEN)?)
        .then_some(record_index)
    };
    if bytes.get(tail_start + tail::LEADING_ZERO) != Some(&tail::LEADING_ZERO_VALUE)
        || View::u32_le_at(bytes, tail_start + tail::OPAQUE_WORD)? != tail::OPAQUE_WORD_VALUE
        || bytes.get(tail_start + tail::SEPARATOR..tail_start + tail::CHAIN_REFERENCE_SECOND)?
            != EXTRUDE_SCOPE_TAIL_SEPARATOR
        || bytes.get(tail_start + tail::SECOND_SEPARATOR) != Some(&tail::SECOND_SEPARATOR_VALUE)
    {
        return None;
    }
    let first = marked_reference(tail::CHAIN_REFERENCE_FIRST)?;
    let second = marked_reference(tail::CHAIN_REFERENCE_SECOND)?;
    let scope = marked_reference(tail::SCOPE_REFERENCE)?;
    if scope != scope_record_index {
        return None;
    }
    Some((
        DesignExtrudeSelectionScopeTail {
            opaque_scalar_bits: View::u64_le_at(bytes, tail_start + tail::OPAQUE_SCALAR)?,
            opaque_word: tail::OPAQUE_WORD_VALUE,
            chain_references: [first, second],
            scope_record_index: scope,
        },
        tail_end,
    ))
}

/// Fields shared by every selection-member tail form: the identity run up to
/// and including the optional-slot marker.
pub(super) struct ExtrudeIdentityFrame {
    pub(super) local_id: u64,
    pub(super) asset_id: String,
    pub(super) after_asset_id: usize,
    pub(super) context_id: String,
    pub(super) after_context_id: usize,
    pub(super) tail_slot_offset: usize,
    pub(super) tail_slot_present: bool,
}

pub(super) fn parse_extrude_identity_frame(
    bytes: &[u8],
    start: usize,
) -> Option<ExtrudeIdentityFrame> {
    if bytes.get(start + extrude_member::ZERO_RUN_10..start + extrude_member::LOCAL_IDENTITY)?
        != [0; 10]
    {
        return None;
    }
    let local_id = View::u64_le_at(bytes, start + extrude_member::LOCAL_IDENTITY)?;
    let (asset_id, after_asset_id) =
        lp_utf16_bounded(bytes, start + extrude_member::ASSET_UUID_LENGTH, 1..=256)?;
    let (context_id, after_context_id) = lp_utf16_bounded(bytes, after_asset_id, 1..=256)?;
    let tail_slot_offset = after_context_id.checked_add(4)?;
    let tail_slot_present = match bytes.get(tail_slot_offset)? {
        0 => false,
        1 => true,
        _ => return None,
    };
    if !is_guid_relaxed(&asset_id)
        || !is_guid_relaxed(&context_id)
        || View::u32_le_at(bytes, after_context_id)? != 2
    {
        return None;
    }
    Some(ExtrudeIdentityFrame {
        local_id,
        asset_id,
        after_asset_id,
        context_id,
        after_context_id,
        tail_slot_offset,
        tail_slot_present,
    })
}

/// Longest extended selection-member tail that a following record header ends.
pub(super) const EXTRUDE_EXTENDED_TAIL_MAX: usize = 1024;

/// End of an extended selection-member tail that starts at `tail_start`.
///
/// The tail runs to the next indexed record header: u32 `3`, three ASCII
/// digits and a u32 record index. When no header follows, the
/// tail runs to the end of the stream and the record index is zero. Returns
/// the record index and the end offset. The fourteen-byte and fifty-byte
/// lengths belong to the typed tail forms and are not extended.
pub(super) fn extended_extrude_tail_end(bytes: &[u8], tail_start: usize) -> Option<(u32, usize)> {
    let is_header = |at: usize| -> Option<u32> {
        let header = bytes.get(at..at.checked_add(11)?)?;
        (header[..4] == 3u32.to_le_bytes() && header[4..7].iter().all(u8::is_ascii_digit))
            .then(|| View::u32_le_at(bytes, at + 7))?
    };
    let typed_length = |end: usize| matches!(end - tail_start, 14 | 50);
    for at in tail_start.checked_add(1)?..bytes.len() {
        if let Some(index) = is_header(at) {
            return (at - tail_start <= EXTRUDE_EXTENDED_TAIL_MAX && !typed_length(at))
                .then_some((index, at));
        }
    }
    (bytes.len() > tail_start && !typed_length(bytes.len())).then_some((0, bytes.len()))
}

/// Parse a selection-member identity frame.
///
/// `scope_record_index` names the scope that owns the selection group; the
/// scope-tail and extended-tail forms are admitted only when it is given.
pub(super) fn parse_extrude_identity_member(
    bytes: &[u8],
    start: usize,
    scope_record_index: Option<u32>,
) -> Option<ParsedExtrudeIdentityMember> {
    parse_extrude_identity_member_exact(bytes, start, scope_record_index).or_else(|| {
        scope_record_index?;
        let frame = parse_extrude_identity_frame(bytes, start)?;
        let fixed_end = start.checked_add(extrude_member::LEN)?;
        if View::u32_le_at(bytes, frame.tail_slot_offset + 1) != Some(0)
            || frame.after_context_id.checked_add(9)? != fixed_end
        {
            return None;
        }
        let (next_record_index, tail_end) = extended_extrude_tail_end(bytes, fixed_end)?;
        Some(ParsedExtrudeIdentityMember {
            local_id: frame.local_id,
            local_id_offset: u64::try_from(start + extrude_member::LOCAL_IDENTITY).ok()?,
            asset_id: frame.asset_id,
            asset_id_offset: u64::try_from(start + extrude_member::ASSET_UUID_UTF16).ok()?,
            context_id: frame.context_id,
            context_id_offset: u64::try_from(frame.after_asset_id + 4).ok()?,
            tail_slot_present: frame.tail_slot_present,
            tail_slot_offset: u64::try_from(frame.tail_slot_offset).ok()?,
            scope_tail: None,
            next_record_index,
            next_byte_offset: u64::try_from(tail_end).ok()?,
        })
    })
}

pub(super) fn parse_extrude_identity_member_exact(
    bytes: &[u8],
    start: usize,
    scope_record_index: Option<u32>,
) -> Option<ParsedExtrudeIdentityMember> {
    let ExtrudeIdentityFrame {
        local_id,
        asset_id,
        after_asset_id,
        context_id,
        after_context_id,
        tail_slot_offset,
        tail_slot_present,
    } = parse_extrude_identity_frame(bytes, start)?;
    let fixed_end = start.checked_add(extrude_member::LEN)?;
    if let Some(scope_record_index) = scope_record_index {
        if !tail_slot_present
            && View::u32_le_at(bytes, tail_slot_offset + 1) == Some(0)
            && after_context_id.checked_add(9)? == fixed_end
            && bytes.get(fixed_end) == Some(&0)
        {
            if let Some((scope_tail, tail_end)) =
                parse_extrude_selection_scope_tail(bytes, fixed_end, scope_record_index)
            {
                let (next_record_index, next_byte_offset) = if tail_end == bytes.len() {
                    (0, u64::try_from(tail_end).ok()?)
                } else {
                    let (_, after_next_tag) =
                        lp_ascii_filtered_view(bytes, tail_end, 1..=2000, u8::is_ascii_graphic)?;
                    (
                        View::u32_le_at(bytes, after_next_tag)?,
                        u64::try_from(tail_end).ok()?,
                    )
                };
                return Some(ParsedExtrudeIdentityMember {
                    local_id,
                    local_id_offset: u64::try_from(start + extrude_member::LOCAL_IDENTITY).ok()?,
                    asset_id,
                    asset_id_offset: u64::try_from(start + extrude_member::ASSET_UUID_UTF16)
                        .ok()?,
                    context_id,
                    context_id_offset: u64::try_from(after_asset_id + 4).ok()?,
                    tail_slot_present,
                    tail_slot_offset: u64::try_from(tail_slot_offset).ok()?,
                    scope_tail: Some(scope_tail),
                    next_record_index,
                    next_byte_offset,
                });
            }
        }
    }
    let fixed_end = if tail_slot_present
        && View::u32_le_at(bytes, tail_slot_offset + 1) == Some(0)
        && after_context_id.checked_add(9)? == fixed_end
        && bytes.get(fixed_end) == Some(&0)
    {
        use crate::layout::extrude_selection_member_long_tail as tail;
        if bytes.get(fixed_end + tail::LEADING_ZERO) != Some(&tail::LEADING_ZERO_VALUE)
            || View::u32_le_at(bytes, fixed_end + tail::KIND) != Some(tail::KIND_VALUE)
            || View::u64_le_at(bytes, fixed_end + tail::VALUE) != Some(tail::VALUE_VALUE)
            || bytes.get(fixed_end + tail::TRAILING_ZERO) != Some(&tail::TRAILING_ZERO_VALUE)
        {
            return None;
        }
        fixed_end.checked_add(tail::LEN)?
    } else {
        fixed_end
    };
    let (next_record_index, next_byte_offset) = if View::u32_le_at(bytes, tail_slot_offset + 1)
        == Some(0)
        && after_context_id.checked_add(9)? == start.checked_add(extrude_member::LEN)?
    {
        if fixed_end == bytes.len() {
            (0, u64::try_from(fixed_end).ok()?)
        } else {
            let (_, after_next_tag) =
                lp_ascii_filtered_view(bytes, fixed_end, 1..=2000, u8::is_ascii_graphic)?;
            (
                View::u32_le_at(bytes, after_next_tag)?,
                u64::try_from(fixed_end).ok()?,
            )
        }
    } else if bytes.get(tail_slot_offset + 1..tail_slot_offset + 4)? == [0; 3] {
        let mut cursor = tail_slot_offset.checked_add(4)?;
        let (next_record_index, _) = take_record_reference(bytes, &mut cursor)?;
        let next_at = cursor;
        let (_, after_next_tag) =
            lp_ascii_filtered_view(bytes, next_at, 1..=2000, u8::is_ascii_graphic)?;
        if View::u32_le_at(bytes, after_next_tag)? != next_record_index {
            return None;
        }
        (next_record_index, u64::try_from(next_at).ok()?)
    } else {
        return None;
    };
    Some(ParsedExtrudeIdentityMember {
        local_id,
        local_id_offset: u64::try_from(start + extrude_member::LOCAL_IDENTITY).ok()?,
        asset_id,
        asset_id_offset: u64::try_from(start + extrude_member::ASSET_UUID_UTF16).ok()?,
        context_id,
        context_id_offset: u64::try_from(after_asset_id + 4).ok()?,
        tail_slot_present,
        tail_slot_offset: u64::try_from(tail_slot_offset).ok()?,
        scope_tail: None,
        next_record_index,
        next_byte_offset,
    })
}

pub(super) struct ParsedEdgeIdentityMember {
    pub(super) layout: crate::records::topology::edge_identity::DesignEdgeIdentityLayout,
    pub(super) local_id: u64,
    pub(super) asset_id: String,
    pub(super) asset_id_offset: u64,
    pub(super) context_id: String,
    pub(super) context_id_offset: u64,
}

pub(super) fn parse_edge_identity_member(
    bytes: &[u8],
    start: usize,
) -> Option<ParsedEdgeIdentityMember> {
    use crate::records::topology::edge_identity::DesignEdgeIdentityLayout;
    let layout = if bytes.get(start + 11..start + 23) == Some(&[0; 12]) {
        DesignEdgeIdentityLayout::Full
    } else if bytes.get(start + 11..start + 22) == Some(&[0; 11]) {
        DesignEdgeIdentityLayout::Compact
    } else if bytes.get(start + 11..start + 21) == Some(&[0; 10]) {
        DesignEdgeIdentityLayout::Shortest
    } else {
        return None;
    };
    let marker_offset = usize::try_from(layout.marker_offset()).ok()?;
    let local_id_offset = marker_offset + 1;
    let asset_offset = marker_offset + 15;
    if bytes.get(start + marker_offset) != Some(&1)
        || bytes.get(start + marker_offset + 5..start + marker_offset + 11)? != [0; 6]
        || View::u32_le_at(bytes, start + marker_offset + 11)? != 1
    {
        return None;
    }
    let local_id = u64::from(View::u32_le_at(bytes, start + local_id_offset)?);
    let (asset_id, after_asset_id) = lp_utf16_bounded(bytes, start + asset_offset, 1..=256)?;
    let (context_id, _after_context_id) = lp_utf16_bounded(bytes, after_asset_id, 1..=256)?;
    if !is_guid_relaxed(&asset_id) || !is_guid_relaxed(&context_id) {
        return None;
    }
    Some(ParsedEdgeIdentityMember {
        layout,
        local_id,
        asset_id,
        asset_id_offset: u64::try_from(start + asset_offset + 4).ok()?,
        context_id,
        context_id_offset: u64::try_from(after_asset_id + 4).ok()?,
    })
}

pub(super) fn utf16_decimal_u64(bytes: &[u8], at: usize) -> Option<(u64, usize)> {
    let count = usize::try_from(View::u32_le_at(bytes, at)?).ok()?;
    if !(1..=256).contains(&count) {
        return None;
    }
    let start = at.checked_add(4)?;
    let end = start.checked_add(count.checked_mul(2)?)?;
    bytes.get(start..end)?;
    let mut value = 0u64;
    let mut digits = 0usize;
    for index in 0..count {
        let unit = View::u16_le_at(bytes, start.checked_add(index.checked_mul(2)?)?)?;
        if index == 0 && unit == u16::from(b'+') {
            continue;
        }
        let digit = u8::try_from(unit).ok()?.checked_sub(b'0')?;
        if digit > 9 {
            return None;
        }
        value = value.checked_mul(10)?.checked_add(u64::from(digit))?;
        digits += 1;
    }
    (digits > 0).then_some((value, end))
}
