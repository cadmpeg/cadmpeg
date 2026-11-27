// SPDX-License-Identifier: Apache-2.0
//! The owner record that a `Derived Base Feature` frame names.

use crate::container::ContainerScan;
use crate::layout::{
    derived_base_mirror_companion as companion, derived_base_mirror_scope as mirror,
    derived_base_owner_reference as owner,
};
use crate::records::entity_header::DESIGN_MODULE_FUSION;
use crate::records::feature::derived_base::{
    is_derived_base, DesignDerivedBaseOwner, DesignDerivedBaseSource, REPEAT_FROM_END,
};
use crate::records::feature::scope::DesignParameterScope;
use crate::records::identity::Located;
use cadmpeg_core::container::ContainerRole;
use cadmpeg_core::decode::{u64_from_index, View};
use cadmpeg_core::CodecError;
use std::collections::HashSet;

use super::meta::{metadata_for_bulk_stream, scan_design_primary_frames, DesignPrimaryFrame};

const MIRROR_COMPONENT: &str = "71BBEDD5-81F8-4585-816B-05908C207204";
const MIRROR_DERIVED_BASE: &str = "B1E5B971-C007-48C9-8FF5-F4CCD91F87A4";
const PRESENTATION: &str = "6CCF41D5-40BE-48ED-A834-18F3EAED6C57";

/// Record the owner of each `Derived Base Feature` scope whose frame names it.
pub(crate) fn mark_derived_base_owners(
    scan: &ContainerScan,
    scopes: &mut [DesignParameterScope],
) -> Result<(), CodecError> {
    for stream_entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let stream = crate::ids::native_scope(&stream_entry.name);
        let in_stream = |scope: &DesignParameterScope| {
            crate::ids::native_stream(&scope.id) == Some(stream.as_str()) && is_derived_base(scope)
        };
        let mut stream_view = None;
        for scope in scopes.iter_mut().filter(|scope| in_stream(scope)) {
            let view = match stream_view {
                Some(view) => view,
                None => {
                    *stream_view.insert(scan.entry_view(&stream_entry.name).ok_or_else(|| {
                        CodecError::malformed(format_args!("entry {} not found", stream_entry.name))
                    })?)
                }
            };
            let (Ok(start), Ok(end)) = (
                usize::try_from(scope.byte_offset()),
                usize::try_from(scope.paired_byte_offset()),
            ) else {
                continue;
            };
            scope.derived_base_owner = view
                .child(start, end)
                .and_then(|frame| derived_base_owner(frame.window(), start));
            scope.derived_base_source = view
                .child(start, end)
                .and_then(|frame| derived_base_source(frame.window(), start));
        }
        let Some(view) = stream_view else {
            continue;
        };
        let mut candidates = scopes
            .iter_mut()
            .filter(|scope| {
                in_stream(scope)
                    && scope.derived_base_owner.is_none()
                    && scope.frame_length() == u64_from_index(mirror::PAIRED_HEADER)
            })
            .peekable();
        if candidates.peek().is_none() {
            continue;
        }
        let Some(meta) = metadata_for_bulk_stream(scan, &stream_entry.name)? else {
            continue;
        };
        let Ok(frames) = scan_design_primary_frames(scan, &stream_entry.name, &meta) else {
            continue;
        };
        let mirror_components = frames
            .iter()
            .filter(|frame| registered_as(frame, MIRROR_COMPONENT, 0))
            .map(|frame| frame.entity_id)
            .collect::<HashSet<_>>();
        for scope in candidates {
            scope.derived_base_owner = mirror_owner(view, scope, &frames, &mirror_components);
        }
    }
    Ok(())
}

pub(crate) fn derived_base_source(frame: &[u8], start: usize) -> Option<DesignDerivedBaseSource> {
    use crate::layout::derived_base_source_pair_prefix as prefix;
    let frame = frame.get(..prefix::LEN)?;
    if frame.get(crate::layout::indexed_design_record_header::LEN..prefix::BODY_MARKER)? != [0; 8]
        || frame.get(prefix::BODY_MARKER) != Some(&prefix::BODY_MARKER_VALUE)
        || View::u32_le_at(frame, prefix::BODY_REFERENCE_COUNT)?
            != prefix::BODY_REFERENCE_COUNT_VALUE
        || frame.get(prefix::COPY_MARKER) != Some(&prefix::COPY_MARKER_VALUE)
        || View::u32_le_at(frame, prefix::COPY_ZERO)? != prefix::COPY_ZERO_VALUE
        || View::u32_le_at(frame, prefix::SOURCE_PAIR_COUNT)? != prefix::SOURCE_PAIR_COUNT_VALUE
        || frame.get(prefix::TERMINATOR) != Some(&prefix::TERMINATOR_VALUE)
    {
        return None;
    }
    let read = |at: usize, trailing: usize| {
        if frame.get(at.checked_sub(1)?) != Some(&1)
            || frame
                .get(at.checked_add(8)?..at.checked_add(8 + trailing)?)?
                .iter()
                .any(|byte| *byte != 0)
        {
            return None;
        }
        Some(Located {
            value: View::u64_le_at(frame, at)?,
            offset: u64_from_index(start.checked_add(at)?),
        })
    };
    let result_body = read(prefix::RESULT_BODY_RECORD, 6)?;
    let repeat = read(prefix::REPEAT_RECORD, 2)?;
    if result_body.value != repeat.value {
        return None;
    }
    Some(DesignDerivedBaseSource {
        parameter_body: read(prefix::PARAMETER_BODY_RECORD, 6)?,
        result_body,
        owner_record: read(prefix::OWNER_RECORD, 2)?,
        source_body: read(prefix::SOURCE_RECORD, 2)?,
        result_repeat_offset: repeat.offset,
        snapshot: None,
    })
}

fn registered_as(frame: &DesignPrimaryFrame<'_>, guid: &str, version: u32) -> bool {
    frame
        .design_type
        .type_guid
        .as_str()
        .eq_ignore_ascii_case(guid)
        && frame.design_type.version == version
        && frame.design_type.module == DESIGN_MODULE_FUSION
}

fn mirror_owner(
    view: View<'_>,
    scope: &DesignParameterScope,
    frames: &[DesignPrimaryFrame<'_>],
    mirror_components: &HashSet<u64>,
) -> Option<DesignDerivedBaseOwner> {
    let start = usize::try_from(scope.byte_offset()).ok()?;
    let paired = usize::try_from(scope.paired_byte_offset()).ok()?;
    let index = frames
        .binary_search_by_key(&start, |frame| frame.start)
        .ok()?;
    let primary = frames.get(index)?;
    let presentation = frames.get(index.checked_add(1)?)?;
    if primary.entity_id != u64::from(scope.record_index)
        || primary.member_end != paired
        || start.checked_add(mirror::PAIRED_HEADER)? != paired
        || start.checked_add(mirror::LEN)? != primary.end
        || primary.end != presentation.start
        || !registered_as(primary, MIRROR_DERIVED_BASE, 10)
        || !registered_as(presentation, PRESENTATION, 1)
        || presentation.start.checked_add(companion::LEN)? != presentation.member_end
        || presentation.member_end != presentation.end
    {
        return None;
    }
    let scope_frame = view.child(start, paired)?;
    let companion_frame = view.child(presentation.start, presentation.end)?;
    let owner_record = plain_reference(scope_frame, owner::MARKER)?;
    if !mirror_components.contains(&u64::from(owner_record))
        || u64::from(plain_reference(scope_frame, mirror::COMPANION_REFERENCE)?)
            != presentation.entity_id
        || scope_frame.window().get(mirror::CLOSING_ZEROES..) != Some(&[0, 0])
        || plain_reference(companion_frame, companion::OWNER_REFERENCE)? != owner_record
        || plain_reference(companion_frame, companion::SCOPE_REFERENCE)? != scope.record_index
        || plain_reference(companion_frame, companion::REPEATED_SCOPE_REFERENCE)?
            != scope.record_index
    {
        return None;
    }
    Some(DesignDerivedBaseOwner {
        record_index: Located {
            value: owner_record,
            offset: u64_from_index(start.checked_add(owner::RECORD_INDEX)?),
        },
        repeat_offset: u64_from_index(presentation.start.checked_add(companion::OWNER_REFERENCE)?),
        result_body: mirror_result_body(scope_frame),
    })
}

fn mirror_result_body(frame: View<'_>) -> Option<Located<u32>> {
    if View::u32_le_at(frame.window(), mirror::BODY_REFERENCE_COUNT)
        != Some(mirror::BODY_REFERENCE_COUNT_VALUE)
    {
        return None;
    }
    plain_reference(frame, mirror::FIRST_BODY_REFERENCE)?;
    Some(Located {
        value: plain_reference(frame, mirror::RESULT_BODY_REFERENCE)?,
        offset: u64_from_index(
            frame
                .start()
                .checked_add(mirror::RESULT_BODY_REFERENCE + 1)?,
        ),
    })
}

fn plain_reference(frame: View<'_>, at: usize) -> Option<u32> {
    let start = frame.start().checked_add(at)?;
    let mut reference = frame.child(start, start.checked_add(owner::LEN - owner::MARKER)?)?;
    if reference.u8()? != owner::MARKER_VALUE {
        return None;
    }
    let index = u32::try_from(reference.u64_le()?).ok()?;
    (index != 0 && reference.u16_le()? == 0).then_some(index)
}

/// The owner that `frame`, which starts at stream offset `start`, names at
/// its owner-reference offset and again twelve bytes before its end, with a
/// zero last byte. `None` when either reference is malformed or they differ.
pub(crate) fn derived_base_owner(frame: &[u8], start: usize) -> Option<DesignDerivedBaseOwner> {
    let reference = frame.get(owner::MARKER..owner::LEN)?;
    let repeat_at = frame.len().checked_sub(REPEAT_FROM_END)?;
    if repeat_at < owner::LEN
        || frame.get(repeat_at..repeat_at.checked_add(owner::LEN - owner::MARKER)?)? != reference
        || frame.last() != Some(&0)
        || reference.first() != Some(&owner::MARKER_VALUE)
        || frame.get(owner::ZERO_RUN_2..owner::LEN) != Some(&[0; 2])
    {
        return None;
    }
    let record_index = u32::try_from(View::u64_le_at(frame, owner::RECORD_INDEX)?).ok()?;
    Some(DesignDerivedBaseOwner {
        record_index: Located {
            value: record_index,
            offset: u64_from_index(start.checked_add(owner::RECORD_INDEX)?),
        },
        repeat_offset: u64_from_index(start.checked_add(repeat_at)?),
        result_body: None,
    })
}

#[cfg(test)]
mod tests;
