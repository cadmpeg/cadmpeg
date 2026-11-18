// SPDX-License-Identifier: Apache-2.0

use super::point_direction::{point_direction_carrier_at, POINT_DIRECTION_TYPE_GUID};
use crate::bytes::local_reference_target;
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::design::decode::text::fixed_guid_end;
use crate::layout::{
    silhouette_split_scope_header as header, silhouette_split_selection_prefix as selection,
};
use crate::loss::F3dLossCode;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::feature::silhouette_split::{DesignSilhouetteDirection, DesignSilhouetteSplit};
use cadmpeg_core::decode::{u64_from_index, View};
use cadmpeg_ir::report::loss::LossNote;
use std::collections::HashMap;

pub(super) const TYPE_GUID: &str = "D1C361B7-FA73-4D59-9406-066780667135";

const LOCAL_REFERENCE_LEN: usize = 11;

pub(super) fn decode(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    types: &HashMap<u64, (&str, u32)>,
    losses: &mut Vec<LossNote>,
) -> Option<DesignSilhouetteSplit> {
    if !types
        .get(&u64::from(scope.record_index))
        .is_some_and(|(guid, version)| guid.eq_ignore_ascii_case(TYPE_GUID) && *version == 0)
    {
        return None;
    }
    let Some((mode_word, carrier, selector)) = prefix(bytes, scope) else {
        losses.push(
            F3dLossCode::FeatureSilhouetteSplitPrefixUnparsed.note(format!(
                "Silhouette Split {} has no admitted control prefix.",
                scope.id
            )),
        );
        return None;
    };
    Some(DesignSilhouetteSplit {
        mode_word,
        carrier: direction_carrier(bytes, records, types, carrier, selector),
    })
}

fn prefix(bytes: &[u8], scope: &DesignParameterScope) -> Option<(u32, u32, u32)> {
    let count_at = scope.reference_count_offset();
    let first_offset = count_at.checked_add(5)?;
    let last_offset = first_offset.checked_add(u64_from_index(3 * LOCAL_REFERENCE_LEN))?;
    if !scope
        .reference_members()
        .offsets()
        .copied()
        .eq((first_offset..=last_offset).step_by(LOCAL_REFERENCE_LEN))
    {
        return None;
    }
    let mut members = scope.reference_members().values().copied();
    let carrier = members.next()?;
    let selector = members.next()?;
    let group = members.next()?;
    let member = members.next()?;
    if [carrier, selector, group, member].contains(&0) || members.next().is_some() {
        return None;
    }
    let start = usize::try_from(scope.byte_offset()).ok()?;
    let table_at = usize::try_from(count_at).ok()?;
    let bytes = bytes.get(..table_at)?;
    let fixed_header = bytes.get(start..)?.get(..header::LEN)?;
    if fixed_header[header::ZERO_RUN..header::PROPERTY_FLAG] != [0; 9] {
        return None;
    }
    let mut view = View::over_retained(bytes);
    view.seek(start.checked_add(header::LEN)?)?;
    match fixed_header[header::PROPERTY_FLAG] {
        0 => {}
        1 if view.u32_le()? == 0 => {}
        _ => return None,
    }
    let fixed = view.take(selection::LEN)?;
    if fixed[selection::ZERO_RUN..selection::CARRIER] != [0; 8]
        || local_reference_target(fixed, selection::CARRIER, false) != Some(carrier)
        || View::u32_le_at(fixed, selection::ZERO_WORD)? != 0
    {
        return None;
    }
    let mode_word = View::u32_le_at(fixed, selection::RESULT_MODE)?;
    let browser_count = View::u32_le_at(fixed, selection::BROWSER_COUNT)?;
    let count = view
        .counted(u64::from(browser_count), LOCAL_REFERENCE_LEN)?
        .get();
    let browser = view.take(count.checked_mul(LOCAL_REFERENCE_LEN)?)?;
    if !browser.chunks_exact(LOCAL_REFERENCE_LEN).all(|reference| {
        local_reference_target(reference, 0, false).is_some_and(|target| target != 0)
    }) || view.u32_le()? != 0
        || view.u32_le()? != 1
        || local_reference_target(view.take(LOCAL_REFERENCE_LEN)?, 0, false) != Some(group)
    {
        return None;
    }
    let after_guid = fixed_guid_end(bytes, view.position())?;
    view.seek(after_guid)?;
    (view.take(3)? == [0; 3] && view.is_empty()).then_some((mode_word, carrier, selector))
}

fn direction_carrier(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    types: &HashMap<u64, (&str, u32)>,
    carrier: u32,
    selector: u32,
) -> Option<DesignSilhouetteDirection> {
    let (guid, version) = types.get(&u64::from(carrier))?;
    if !guid.eq_ignore_ascii_case(POINT_DIRECTION_TYPE_GUID) || *version != 4 {
        return None;
    }
    let mut frames = records.frames(carrier);
    let (start, end) = frames.next()?;
    if frames.next().is_some() {
        return None;
    }
    let decoded = point_direction_carrier_at(bytes, start, end, carrier, *version)?;
    let [input] = decoded.input_records.as_slice() else {
        return None;
    };
    let input_at = usize::try_from(input.offset.checked_sub(1)?).ok()?;
    if local_reference_target(bytes.get(..end)?, input_at, false) != Some(selector) {
        return None;
    }
    Some(DesignSilhouetteDirection {
        position: decoded.position,
        direction: decoded.direction,
        reference_type: decoded.reference_type,
    })
}

#[cfg(test)]
mod tests;
