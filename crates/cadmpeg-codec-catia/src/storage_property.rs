// SPDX-License-Identifier: Apache-2.0
//! Bounded storage-property member rows, separate from surface aliases.

use cadmpeg_core::decode::{u64_from_index, View};
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

fn numbered_label(data: &[u8], lower: usize, end: usize) -> Option<Range<usize>> {
    let digits_end = end.checked_sub(2)?;
    if digits_end < lower || data.get(digits_end..end) != Some(&[0, 0]) {
        return None;
    }
    let mut start = digits_end;
    while start > lower && data[start - 1].is_ascii_digit() {
        start -= 1;
    }
    if start == digits_end {
        return None;
    }
    let frame = start
        .checked_sub(number::LEN)
        .filter(|frame| *frame >= lower)?;
    let length = digits_end - start;
    if View::u32_le_at(data, frame + number::DIGIT_COUNT).and_then(|n| usize::try_from(n).ok())
        != Some(length)
        || View::u16_le_at(data, frame + number::INCLUSIVE_LENGTH).map(usize::from)
            != length.checked_add(4)
    {
        return None;
    }
    Some(start..digits_end)
}

fn member_source(data: &[u8], marker: usize, segment: &FinjplSegment) -> Option<MemberSource> {
    if segment.type_word != 0x8e
        || marker < segment.range.start
        || marker
            .checked_add(core::LEN)
            .is_none_or(|end| end > segment.range.end)
        || data.get(marker..marker + core::TAG) != Some(&[1, 0, 4, 0])
    {
        return None;
    }
    if let Some(start) = marker
        .checked_sub(nullable::LEN)
        .filter(|start| *start >= segment.range.start)
    {
        if View::u32_le_at(data, start + nullable::DISCRIMINATOR) == Some(0x133) {
            let state = match View::u32_le_at(data, start + nullable::STATE) {
                Some(0x0001_0001) => MemberState::Present,
                Some(0x0001_0000) => MemberState::Absent,
                _ => return None,
            };
            return Some(MemberSource {
                state,
                label: numbered_label(data, segment.range.start, start),
            });
        }
    }
    if let Some(start) = marker
        .checked_sub(direct::LEN)
        .filter(|start| *start >= segment.range.start)
    {
        if View::u32_le_at(data, start + direct::LEAD) == Some(1) {
            if let Some(label) = numbered_label(data, segment.range.start, start) {
                return Some(MemberSource {
                    state: MemberState::Direct,
                    label: Some(label),
                });
            }
        }
    }
    None
}

/// Exclude a source-closed storage member from alias admission.
pub(crate) fn is_member(data: &[u8], marker: usize, segments: &[FinjplSegment]) -> bool {
    let index = segments.partition_point(|segment| segment.range.start <= marker);
    let Some(segment) = index.checked_sub(1).and_then(|index| segments.get(index)) else {
        return false;
    };
    member_source(data, marker, segment).is_some()
}

/// Preserve complete members in their containing storage segments.
pub(crate) fn parse(
    data: &[u8],
    segments: &[FinjplSegment],
) -> Result<Vec<CatiaStoragePropertyMember>, CodecError> {
    let mut members = Vec::new();
    for (index, segment) in segments
        .iter()
        .enumerate()
        .filter(|(_, segment)| segment.type_word == 0x8e)
    {
        for offset in memchr::memmem::find_iter(&data[segment.range.clone()], &[1, 0, 4, 0]) {
            let marker = segment.range.start + offset;
            let Some(source) = member_source(data, marker, segment) else {
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
                    Ok::<_, CodecError>(text.to_owned())
                })
                .transpose()?;
            let id = format!("catia:outer:storage-member#{marker:010}");
            let segment_id = format!("catia:outer:finjpl#{index}");
            members.push(CatiaStoragePropertyMember {
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
            });
        }
    }
    Ok(members)
}

#[cfg(test)]
mod tests;
