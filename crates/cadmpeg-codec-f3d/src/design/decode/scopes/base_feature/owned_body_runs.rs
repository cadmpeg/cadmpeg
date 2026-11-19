// SPDX-License-Identifier: Apache-2.0

use super::{read_base_feature_entry_at, TimelineBodyOwner};
use crate::bytes::{take_reference, TYPED_LOCAL_REFERENCE_LEN};
use crate::design::presentation::BODY_PRESENTATION_TYPE_GUID;
use crate::layout::base_feature_result_body_entry as fixed_row;
use crate::layout::base_feature_result_body_prefix as prefix;
use crate::layout::timeline_owned_base_feature_typed_body_entry as typed_row;
use crate::records::feature::base_feature::{local_reference_field, DesignBaseFeatureEntry};
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;

pub(super) struct OwnedBodyRuns {
    pub(super) parameter_bodies: Vec<DesignBaseFeatureEntry<u64>>,
    pub(super) bodies: Vec<DesignBaseFeatureEntry<u64>>,
    pub(super) end: usize,
    pub(super) typed: bool,
}

pub(super) fn decode(
    ctx: &DecodeContext<'_>,
    stream: View<'_>,
    scope: &DesignParameterScope,
    owner: TimelineBodyOwner,
) -> Result<Option<OwnedBodyRuns>, CodecError> {
    let Some(rows) = frame(stream, scope, owner.body_count) else {
        return Ok(None);
    };
    let typed = owner.paired && !owner.pastes && read_typed_entry(rows.window(), &mut 0).is_some();
    let width = if typed {
        typed_row::LEN
    } else {
        fixed_row::LEN
    };
    let Some(count) = rows.counted(u64_from_index(owner.body_count), 2 * width) else {
        return Ok(None);
    };
    let mut cursor = 0;
    let mut run = || -> Result<Option<Vec<DesignBaseFeatureEntry<u64>>>, CodecError> {
        let mut entries = ctx.alloc_filled(
            count.get(),
            DesignBaseFeatureEntry {
                value: 0,
                offset: 0,
                field: [0; 6],
            },
            "decode F3D timeline-owned Base Feature body run",
        )?;
        for entry in &mut entries {
            let decoded = if typed {
                read_typed_entry(rows.window(), &mut cursor)
            } else {
                read_base_feature_entry_at(rows.window(), &mut cursor)
            };
            let Some(mut decoded) = decoded else {
                return Ok(None);
            };
            decoded.offset += u64_from_index(rows.position());
            *entry = decoded;
        }
        Ok(Some(entries))
    };
    let Some(parameter_bodies) = run()? else {
        return Ok(None);
    };
    let Some(bodies) = run()? else {
        return Ok(None);
    };
    Ok(Some(OwnedBodyRuns {
        parameter_bodies,
        bodies,
        end: rows.position() + cursor,
        typed,
    }))
}

fn frame<'a>(
    stream: View<'a>,
    scope: &DesignParameterScope,
    body_count: usize,
) -> Option<View<'a>> {
    if scope.kind() != DesignFeatureKind::BaseFeature {
        return None;
    }
    let start = usize::try_from(scope.byte_offset()).ok()?;
    let paired = usize::try_from(scope.paired_byte_offset()).ok()?;
    let references = usize::try_from(scope.reference_count_offset()).ok()?;
    let mut frame = stream.child(start, paired)?.child(start, references)?;
    frame.skip(prefix::ZERO_RUN_8)?;
    if frame.take(prefix::BODY_COUNT_MARKER - prefix::ZERO_RUN_8)? != [0; 8]
        || frame.u8()? != 1
        || usize::try_from(frame.u32_le()?).ok()? != body_count.checked_mul(2)?
    {
        return None;
    }
    frame.child(frame.position(), references)
}

fn read_typed_entry(bytes: &[u8], cursor: &mut usize) -> Option<DesignBaseFeatureEntry<u64>> {
    let at = *cursor;
    let mut next = at;
    let (target, guid) = take_reference(bytes, &mut next)?.into_local()?;
    if next.checked_sub(at)? != TYPED_LOCAL_REFERENCE_LEN
        || !guid?.eq_ignore_ascii_case(BODY_PRESENTATION_TYPE_GUID)
    {
        return None;
    }
    let end = at.checked_add(typed_row::LEN)?;
    let field =
        local_reference_field(bytes.get(at.checked_add(typed_row::REFERENCE_FIELD)?..end)?)?;
    *cursor = end;
    Some(DesignBaseFeatureEntry {
        value: target,
        offset: u64_from_index(at + typed_row::REFERENCE_VALUE),
        field,
    })
}
