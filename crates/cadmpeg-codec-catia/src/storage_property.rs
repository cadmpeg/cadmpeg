// SPDX-License-Identifier: Apache-2.0
//! Bounded storage-property member rows, separate from surface aliases.

use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::native::bytes::NativeBytes;
use serde::{Deserialize, Serialize};
use std::ops::Range;

use crate::container::FinjplSegment;
use crate::layout::{
    storage_member_core as core, storage_member_direct_prefix as direct,
    storage_member_nullable_prefix as nullable, storage_member_number_prefix as number,
};

/// Stored member framing and nullable surface state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MemberState {
    /// Numbered direct storage member.
    Direct,
    /// Nullable member whose state declares a surface carrier.
    Present,
    /// Nullable member whose state declares no surface carrier.
    Absent,
}

/// One complete storage-property member with its full allocation slot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CatiaStoragePropertyMember {
    /// Persistent identity from the source marker offset.
    pub(crate) id: String,
    /// Absolute marker offset.
    pub(crate) byte_offset: u64,
    /// Containing FINJPL segment identity.
    pub(crate) segment: String,
    /// Exact stored branch and nullable state.
    pub(crate) state: MemberState,
    /// Complete stored tag, without an inferred geometric target.
    pub(crate) tag_raw: u32,
    /// Retained single-byte flag.
    pub(crate) flag: u8,
    /// Two unassigned bytes before the slot.
    pub(crate) prefix: NativeBytes<[u8; 2]>,
    /// Complete four-byte slot.
    pub(crate) slot: u32,
    /// Five unassigned trailing bytes.
    pub(crate) tail: NativeBytes<[u8; 5]>,
    /// Numbered label only when its complete string frame closes.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_label"
    )]
    pub(crate) label: Option<String>,
}

cadmpeg_core::named_optional_field!(deserialize_label, String, "label");

struct MemberSource {
    state: MemberState,
    label: Option<Range<usize>>,
}

fn numbered_label(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    lower: usize,
    end: usize,
) -> Result<Option<Range<usize>>, CodecError> {
    let Some(digits_end) = end.checked_sub(2) else {
        return Ok(None);
    };
    if digits_end < lower || data.get(digits_end..end) != Some(&[0, 0]) {
        return Ok(None);
    }
    let mut start = digits_end;
    while start > lower && data[start - 1].is_ascii_digit() {
        ctx.charge_work(1, "catia_storage_property_label_digits")?;
        start -= 1;
    }
    if start == digits_end {
        return Ok(None);
    }
    let Some(frame) = start
        .checked_sub(number::LEN)
        .filter(|frame| *frame >= lower)
    else {
        return Ok(None);
    };
    let length = digits_end - start;
    if View::u32_le_at(data, frame + number::DIGIT_COUNT).and_then(|n| usize::try_from(n).ok())
        != Some(length)
        || View::u16_le_at(data, frame + number::INCLUSIVE_LENGTH).map(usize::from)
            != length.checked_add(4)
    {
        return Ok(None);
    }
    Ok(Some(start..digits_end))
}

fn member_source(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    marker: usize,
    segment: &FinjplSegment,
) -> Result<Option<MemberSource>, CodecError> {
    if segment.type_word != 0x8e
        || marker < segment.range.start
        || marker
            .checked_add(core::LEN)
            .is_none_or(|end| end > segment.range.end)
        || data.get(marker..marker + core::TAG) != Some(&[1, 0, 4, 0])
    {
        return Ok(None);
    }
    if let Some(start) = marker
        .checked_sub(nullable::LEN)
        .filter(|start| *start >= segment.range.start)
    {
        if View::u32_le_at(data, start + nullable::DISCRIMINATOR) == Some(0x133) {
            let state = match View::u32_le_at(data, start + nullable::STATE) {
                Some(0x0001_0001) => MemberState::Present,
                Some(0x0001_0000) => MemberState::Absent,
                _ => return Ok(None),
            };
            return Ok(Some(MemberSource {
                state,
                label: numbered_label(ctx, data, segment.range.start, start)?,
            }));
        }
    }
    if let Some(start) = marker
        .checked_sub(direct::LEN)
        .filter(|start| *start >= segment.range.start)
    {
        if View::u32_le_at(data, start + direct::LEAD) == Some(1) {
            if let Some(label) = numbered_label(ctx, data, segment.range.start, start)? {
                return Ok(Some(MemberSource {
                    state: MemberState::Direct,
                    label: Some(label),
                }));
            }
        }
    }
    Ok(None)
}

/// Exclude a source-closed storage member from alias admission.
pub(crate) fn is_member(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    marker: usize,
    segments: &[FinjplSegment],
) -> Result<bool, CodecError> {
    let index = segments.partition_point(|segment| segment.range.start <= marker);
    let Some(segment) = index.checked_sub(1).and_then(|index| segments.get(index)) else {
        return Ok(false);
    };
    Ok(member_source(ctx, data, marker, segment)?.is_some())
}

/// Preserve complete members in their containing storage segments.
pub(crate) fn parse(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    segments: &[FinjplSegment],
) -> Result<Vec<CatiaStoragePropertyMember>, CodecError> {
    let mut members = Vec::new();
    for (index, segment) in segments
        .iter()
        .enumerate()
        .filter(|(_, segment)| segment.type_word == 0x8e)
    {
        ctx.charge_work(
            u64_from_index(segment.range.len()),
            "catia_storage_property_members_scan",
        )?;
        for offset in memchr::memmem::find_iter(&data[segment.range.clone()], &[1, 0, 4, 0]) {
            let marker = segment.range.start + offset;
            let Some(source) = member_source(ctx, data, marker, segment)? else {
                continue;
            };
            let (Some(tag_raw), Some(slot)) = (
                View::u32_le_at(data, marker + core::TAG),
                View::u32_le_at(data, marker + core::SLOT),
            ) else {
                continue;
            };
            let prefix = [data[marker + core::PREFIX], data[marker + core::PREFIX + 1]];
            let tail: [u8; 5] = data[marker + core::TAIL..marker + core::LEN]
                .try_into()
                .map_err(|_| CodecError::malformed("CATIA storage member tail extent"))?;
            let label = source
                .label
                .map(|range| {
                    let text = std::str::from_utf8(&data[range])
                        .map_err(|_| CodecError::malformed("CATIA numbered storage label"))?;
                    ctx.copy_retained_text(text, "catia_storage_property_label")
                })
                .transpose()?;
            let id = ctx.format_retained(
                format_args!("catia:outer:storage-member#{marker:010}"),
                "catia_storage_property_member_id",
            )?;
            let segment_id = ctx.format_retained(
                format_args!("catia:outer:finjpl#{index}"),
                "catia_storage_property_member_segment",
            )?;
            ctx.push_vec(
                &mut members,
                CatiaStoragePropertyMember {
                    id,
                    byte_offset: u64_from_index(marker),
                    segment: segment_id,
                    state: source.state,
                    tag_raw,
                    flag: data[marker + core::FLAG],
                    prefix: prefix.into(),
                    slot,
                    tail: tail.into(),
                    label,
                },
                "catia_storage_property_members",
            )?;
        }
    }
    Ok(members)
}

#[cfg(test)]
mod tests;
