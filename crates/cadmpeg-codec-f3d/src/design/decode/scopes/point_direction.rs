// SPDX-License-Identifier: Apache-2.0
use super::parameter_scope::payload_prologue;
use crate::bytes::finite_reals_at;
use crate::bytes::lp_ascii_filtered_view;
use crate::bytes::take_reference;
use crate::design::decode::sketch::next_indexed_record_offset;
use crate::records::feature::hole::DesignHoleTangentPoint;
use crate::records::identity::Located;
use cadmpeg_core::decode::View;
use cadmpeg_ir::scalar::FiniteReal;

pub(super) const POINT_DIRECTION_TYPE_GUID: &str = "F2A7590D-6654-4674-B393-A2AEF4FEC48A";

#[derive(Debug, Clone, PartialEq)]
pub(super) struct PointDirectionCarrier {
    pub(super) bounding_box_index: u32,
    pub(super) position: [FiniteReal; 3],
    pub(super) position_at: usize,
    pub(super) direction: [FiniteReal; 3],
    pub(super) direction_at: usize,
    pub(super) point_parameters: [FiniteReal; 2],
    pub(super) point_parameters_at: usize,
    pub(super) reference_type: u32,
    pub(super) reference_type_at: usize,
    pub(super) tangent_point_data: Option<DesignHoleTangentPoint>,
    pub(super) input_records: Vec<Located<u32>>,
}

pub(super) fn point_direction_carrier_at(
    bytes: &[u8],
    start: usize,
    paired_at: usize,
    record_index: u32,
    version: u32,
) -> Option<PointDirectionCarrier> {
    let (_, after_tag) = lp_ascii_filtered_view(bytes, start, 3..=3, u8::is_ascii_digit)?;
    if after_tag != start.checked_add(7)? || View::u32_le_at(bytes, after_tag) != Some(record_index)
    {
        return None;
    }
    let name_at = after_tag.checked_add(8)?;
    let name_length = usize::try_from(View::u32_le_at(bytes, name_at)?).ok()?;
    if name_length > 256 {
        return None;
    }
    let payload_at = name_at.checked_add(4)?.checked_add(name_length)?;
    if !bytes
        .get(name_at.checked_add(4)?..payload_at)?
        .iter()
        .all(u8::is_ascii_graphic)
    {
        return None;
    }
    let body = bytes.get(..paired_at)?;
    let mut cursor = payload_prologue(body, payload_at, paired_at)?;
    let bounding_box_index = View::u32_le_at(body, cursor)?;
    cursor = cursor.checked_add(4)?;
    let position_at = cursor;
    cursor = cursor.checked_add(24)?;
    let direction_at = cursor;
    cursor = cursor.checked_add(24)?;
    let point_parameters_at = cursor;
    cursor = cursor.checked_add(16)?;
    let reference_type_at = cursor;
    let reference_type = View::u32_le_at(body, cursor)?;
    cursor = cursor.checked_add(4)?;
    let tangent_point_data = match version {
        2 | 4 => {
            let prefix = if version == 4 {
                let prefix = *body.get(cursor)?;
                cursor = cursor.checked_add(1)?;
                Some(prefix)
            } else {
                None
            };
            let tangent_point_data_at = cursor;
            cursor = cursor.checked_add(24)?;
            Some(DesignHoleTangentPoint {
                prefix,
                data: Located {
                    value: finite_reals_at(body, tangent_point_data_at)?,
                    offset: u64::try_from(tangent_point_data_at).ok()?,
                },
            })
        }
        1 => None,
        _ => return None,
    };
    let input_count = usize::try_from(View::u32_le_at(body, cursor)?).ok()?;
    cursor = cursor.checked_add(4)?;
    if input_count == 0 || input_count > paired_at.checked_sub(cursor)? {
        return None;
    }
    let position = finite_reals_at(body, position_at)?;
    let direction = finite_reals_at(body, direction_at)?;
    let point_parameters = finite_reals_at(body, point_parameters_at)?;
    let mut input_records = Vec::new();
    for _ in 0..input_count {
        let reference_at = cursor;
        let reference = take_reference(body, &mut cursor)?;
        input_records.push(Located {
            value: u32::try_from(reference.target()?).ok()?,
            offset: u64::try_from(reference_at.checked_add(1)?).ok()?,
        });
    }
    if cursor != paired_at && next_indexed_record_offset(bytes, cursor)? != paired_at {
        return None;
    }
    Some(PointDirectionCarrier {
        bounding_box_index,
        position,
        position_at,
        direction,
        direction_at,
        point_parameters,
        point_parameters_at,
        reference_type,
        reference_type_at,
        tangent_point_data,
        input_records,
    })
}
