// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;

use cadmpeg_core::decode::View;
use cadmpeg_ir::scalar::PositiveReal;

use super::shared_frames::marked_record_reference;
use crate::design::decode::joint_motion::JOINT_TYPE_GUID;
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::layout::{
    motion_link_rotation_relation as relation, motion_link_rotation_scope as envelope,
};
use crate::records::feature::motion_link::{DesignLinkedRotations, ROTATE_Z, SCOPE_TYPE};
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};

const RELATION_TYPE: &str = "3A218162-55F3-492E-93A4-390F8A56FBEA";

pub(super) fn read(
    source: View<'_>,
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    types: &HashMap<u64, (&str, u32)>,
) -> Option<DesignLinkedRotations> {
    let has_type = |index: u32, guid: &str, version: u32| {
        types
            .get(&u64::from(index))
            .is_some_and(|(saved_guid, saved_version)| {
                saved_guid.eq_ignore_ascii_case(guid) && *saved_version == version
            })
    };
    if scope.kind() != DesignFeatureKind::MotionLink || !has_type(scope.record_index, SCOPE_TYPE, 3)
    {
        return None;
    }
    let start = usize::try_from(scope.byte_offset()).ok()?;
    let end = usize::try_from(scope.paired_byte_offset()).ok()?;
    let scope_bytes = source.child(start, end)?.window();
    let index = marked_record_reference(scope_bytes, envelope::RELATION)?;
    let relation_start = start.checked_sub(relation::LEN)?;
    if Some(index) != scope.record_index.checked_sub(1)
        || !has_type(index, RELATION_TYPE, 2)
        || records
            .offsets(index)
            .binary_search(&relation_start)
            .is_err()
    {
        return None;
    }
    let bytes = source.child(relation_start, start)?.window();
    if View::u32_le_at(bytes, relation::PARENT_COUNT)? != 2 {
        return None;
    }
    let first = marked_record_reference(bytes, relation::FIRST_PARENT)?;
    let second = marked_record_reference(bytes, relation::SECOND_PARENT)?;
    if first == second
        || ![first, second]
            .into_iter()
            .all(|index| has_type(index, JOINT_TYPE_GUID, 5))
        || marked_record_reference(bytes, relation::FIRST_JOINT)? != first
        || marked_record_reference(bytes, relation::SECOND_JOINT)? != second
        || View::u32_le_at(bytes, relation::FIRST_AXIS)? != ROTATE_Z
        || View::u32_le_at(bytes, relation::SECOND_AXIS)? != ROTATE_Z
        || View::u64_le_at(bytes, relation::PHASE)? != 0
        || bytes.get(relation::FLAGS..relation::FIRST_ANGLE)? != relation::FLAGS_VALUE
    {
        return None;
    }
    let first_angle = PositiveReal::new(View::f64_le_at(bytes, relation::FIRST_ANGLE)?)?;
    let second_angle = PositiveReal::new(View::f64_le_at(bytes, relation::SECOND_ANGLE)?)?;
    let link = DesignLinkedRotations {
        joints: [first, second],
        angles: [first_angle, second_angle],
    };
    link.is_valid().then_some(link)
}

#[cfg(test)]
mod tests;
