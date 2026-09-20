// SPDX-License-Identifier: Apache-2.0
//! Small record-reference, transform, fixed-scalar and operation-code readers shared by several
//! scope families.

use crate::bytes::f64s_at;
use crate::bytes::lp_ascii_filtered;
use crate::bytes::take_reference;
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::records::feature::extrude::DesignExtrudeOperation;
use cadmpeg_core::decode::View;

pub(in crate::design::decode) fn exact_indexed_header_at(
    bytes: &[u8],
    start: usize,
    record_index: u32,
) -> Option<String> {
    let (class_tag, after_tag) = lp_ascii_filtered(bytes, start, 3..=3, u8::is_ascii_digit)?;
    (View::u32_le_at(bytes, after_tag)? == record_index).then_some(class_tag)
}

pub(super) fn exact_same_segment_record_reference(bytes: &[u8], at: usize) -> Option<(u32, u64)> {
    let mut cursor = at;
    let reference = take_reference(bytes, &mut cursor)?;
    let target = u32::try_from(reference.local()?.0).ok()?;
    (cursor == at.checked_add(11)?).then_some((target, u64::try_from(at.checked_add(1)?).ok()?))
}

pub(in crate::design::decode) fn rigid_transform_at(
    bytes: &[u8],
    at: usize,
) -> Option<crate::records::sketch_placement::SketchPlacementMatrix> {
    let values = f64s_at(bytes, at, 16)?;
    let mut transform = [[0.0; 4]; 4];
    for (ordinal, value) in values.into_iter().enumerate() {
        transform[ordinal / 4][ordinal % 4] = value;
    }
    transform.try_into().ok()
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct FixedScalarFrame {
    pub(super) owner_record_index: Option<u32>,
    pub(super) ordinal: u8,
    pub(super) value: f64,
    pub(super) value_offset: u64,
}

pub(super) fn exact_fixed_scalar(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    record_index: u32,
) -> Option<FixedScalarFrame> {
    let candidates = records
        .frames(record_index)
        .filter_map(|(start, end)| {
            let frame_length = end.checked_sub(start)?;
            matches!(frame_length, 100 | 103 | 104 | 105).then_some(())?;
            if frame_length == 100 || frame_length == 103 {
                let (class_tag, after_tag) =
                    lp_ascii_filtered(bytes, start, 0..=2000, u8::is_ascii_graphic)?;
                if after_tag != start + 7
                    || class_tag.len() != 3
                    || !class_tag.bytes().all(|byte| byte.is_ascii_digit())
                    || bytes.get(start + 11..start + 19) != Some(&[0; 8])
                    || bytes.get(start + 19..start + 24) != Some(&[1, 1, 0, 0, 0])
                    || bytes.get(start + 29..start + 35) != Some(&[0; 6])
                    || bytes.get(start + 36..start + 40) != Some(&[0; 4])
                {
                    return None;
                }
                if frame_length == 103
                    && (marked_record_reference(bytes, start + 24).is_none()
                        || marked_record_reference(bytes, start + 48).is_none()
                        || marked_record_reference(bytes, start + 67)
                            != View::u32_le_at(bytes, start + 25)
                        || bytes.get(start + 78..start + 80) != Some(&[0; 2])
                        || marked_record_reference(bytes, start + 80).is_none()
                        || bytes.get(start + 85..start + 92) != Some(&[0; 7])
                        || marked_record_reference(bytes, start + 92)
                            != View::u32_le_at(bytes, start + 25))
                {
                    return None;
                }
            }
            let value = View::f64_le_at(bytes, start + 40)?;
            value.is_finite().then_some(FixedScalarFrame {
                owner_record_index: (bytes.get(start + 24) == Some(&1))
                    .then(|| View::u32_le_at(bytes, start + 25))
                    .flatten(),
                ordinal: *bytes.get(start + 35)?,
                value,
                value_offset: u64::try_from(start + 40).ok()?,
            })
        })
        .collect::<Vec<_>>();
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    Some(*candidate)
}

pub(in crate::design::decode) fn marked_reference(bytes: &[u8], at: usize) -> Option<u32> {
    (bytes.get(at) == Some(&1)).then(|| View::u32_le_at(bytes, at + 1))?
}

pub(in crate::design::decode) fn marked_record_reference(bytes: &[u8], at: usize) -> Option<u32> {
    if bytes.get(at) != Some(&1) || bytes.get(at + 5..at + 11)? != [0; 6] {
        return None;
    }
    View::u32_le_at(bytes, at + 1)
}

pub(super) fn extrude_operation_at(bytes: &[u8], offset: usize) -> Option<DesignExtrudeOperation> {
    match View::u32_le_at(bytes, offset)? {
        1 => Some(DesignExtrudeOperation::Join),
        2 => Some(DesignExtrudeOperation::Cut),
        3 => Some(DesignExtrudeOperation::Intersect),
        4 => Some(DesignExtrudeOperation::NewBody),
        _ => None,
    }
}
