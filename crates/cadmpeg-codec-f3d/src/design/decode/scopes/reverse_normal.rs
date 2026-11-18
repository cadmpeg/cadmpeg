// SPDX-License-Identifier: Apache-2.0

use super::shared_frames::marked_record_reference;
use crate::design::decode::text::fixed_relaxed_guid_text;
use crate::layout::reverse_normal_prefix as compact;
use crate::layout::shifted_reverse_normal_prefix as shifted;
use crate::records::feature::body_ops::DesignReverseNormalOperation;
use crate::records::identity::Located;
use cadmpeg_core::decode::{u64_from_index, View};

const NULL_LINK_GUID: &str = "00000000-0000-0000-0000-000000000000";

struct PrefixFields {
    group_count: usize,
    group_count_value: u32,
    group_reference: usize,
    link_guid: usize,
    zero_tail: usize,
}

pub(super) fn reverse_normal_operation(
    bytes: &[u8],
    start: usize,
    reference_count_at: usize,
    references: &[u32],
) -> Option<DesignReverseNormalOperation> {
    let at = |offset: usize| start.checked_add(offset);
    let fields = match reference_count_at.checked_sub(start)? {
        compact::LEN => {
            if bytes.get(at(compact::ZERO_RUN)?..at(compact::GROUP_COUNT)?)?
                != [0; compact::GROUP_COUNT - compact::ZERO_RUN]
            {
                return None;
            }
            PrefixFields {
                group_count: compact::GROUP_COUNT,
                group_count_value: compact::GROUP_COUNT_VALUE,
                group_reference: compact::GROUP_REFERENCE,
                link_guid: compact::LINK_GUID,
                zero_tail: compact::ZERO_TAIL,
            }
        }
        shifted::LEN => {
            if bytes.get(at(shifted::ZERO_RUN)?..at(shifted::SHIFT_MARKER)?)?
                != [0; shifted::SHIFT_MARKER - shifted::ZERO_RUN]
                || *bytes.get(at(shifted::SHIFT_MARKER)?)? != shifted::SHIFT_MARKER_VALUE
                || bytes.get(at(shifted::ZERO_RUN_AFTER_MARKER)?..at(shifted::GROUP_COUNT)?)?
                    != [0; shifted::GROUP_COUNT - shifted::ZERO_RUN_AFTER_MARKER]
            {
                return None;
            }
            PrefixFields {
                group_count: shifted::GROUP_COUNT,
                group_count_value: shifted::GROUP_COUNT_VALUE,
                group_reference: shifted::GROUP_REFERENCE,
                link_guid: shifted::LINK_GUID,
                zero_tail: shifted::ZERO_TAIL,
            }
        }
        _ => return None,
    };
    let group = marked_record_reference(bytes, at(fields.group_reference)?)?;
    let (guid, guid_end) = fixed_relaxed_guid_text(bytes, at(fields.link_guid)?).ok()??;
    if View::u32_le_at(bytes, at(fields.group_count)?)? != fields.group_count_value
        || references.first() != Some(&group)
        || guid_end != at(fields.zero_tail)?
        || bytes
            .get(at(fields.zero_tail)?..reference_count_at)?
            .iter()
            .any(|byte| *byte != 0)
    {
        return None;
    }
    let link = if guid.as_str() == NULL_LINK_GUID {
        None
    } else {
        Some(Located {
            value: guid,
            offset: u64_from_index(at(fields.link_guid)?.checked_add(4)?),
        })
    };
    Some(DesignReverseNormalOperation {
        body_group: Located {
            value: group,
            offset: u64_from_index(at(fields.group_reference)?.checked_add(1)?),
        },
        link,
    })
}

#[cfg(test)]
mod tests;
