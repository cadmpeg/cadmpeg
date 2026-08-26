//! Named scalar records, operands and feature object names.

use super::relation_records::{
    compact_scalar_layout, legacy_scalar_layout, scalar_role, shifted_value_only_scalar_trailer,
};
use super::{COMPACT_SCALAR_HEADER, NAME_MARKER, SCALAR_HEADER, VALUE_ONLY_SCALAR_HEADER};
use crate::records::{
    FeatureInputLane, FeatureInputName, FeatureInputOperand, FeatureInputOperandKind,
    FeatureInputScalar,
};
use cadmpeg_core::decode::View;

use crate::layout::feature_input_operand_cell12 as operand_cell;

pub(crate) fn named_scalars(
    payload: &[u8],
    parent: &str,
    names: &[FeatureInputName],
) -> Vec<FeatureInputScalar> {
    let lane_key = parent.rsplit_once('#').map_or(parent, |(_, key)| key);
    names
        .iter()
        .filter_map(|name| {
            let name_offset = usize::try_from(name.offset).ok()?;
            let value_offset = scalar_value_offset(payload, name_offset, &name.value)?;
            let value = View::f64_le_at(payload, value_offset)?;
            let trailer_offset = value_offset.checked_add(8)?;
            let object_id = View::u32_le_at(payload, trailer_offset + 3)?;
            let role = scalar_role(payload, trailer_offset);
            let operands = scalar_operands(payload, trailer_offset, parent);
            let entity_indices = operands
                .iter()
                .filter(|operand| operand.kind == FeatureInputOperandKind::D6)
                .map(|operand| operand.entity_index)
                .collect();
            value.is_finite().then_some((
                name,
                value_offset,
                object_id,
                value,
                role,
                entity_indices,
                operands,
            ))
        })
        .enumerate()
        .map(
            |(ordinal, (name, offset, object_id, value, role, entity_indices, operands))| {
                FeatureInputScalar {
                    id: format!("sldprt:feature-input:scalar#{lane_key}:{offset}"),
                    parent: parent.to_string(),
                    feature_ref: None,
                    ordinal: ordinal as u32,
                    offset: offset as u64,
                    object_id,
                    name: name.id.clone(),
                    value,
                    role,
                    entity_indices,
                    operands,
                }
            },
        )
        .collect()
}

fn scalar_value_offset(payload: &[u8], name_offset: usize, name: &str) -> Option<usize> {
    let header_offset = name_offset
        .checked_add(NAME_MARKER.len() + 1)?
        .checked_add(name.encode_utf16().count().checked_mul(2)?)?;
    let value_offset = header_offset.checked_add(SCALAR_HEADER.len())?;
    if payload.get(header_offset..value_offset) == Some(SCALAR_HEADER) {
        return Some(value_offset);
    }
    let compact_value_offset = header_offset.checked_add(COMPACT_SCALAR_HEADER.len())?;
    if payload.get(header_offset..compact_value_offset) == Some(COMPACT_SCALAR_HEADER)
        && compact_scalar_layout(payload, compact_value_offset.checked_add(8)?)
    {
        return Some(compact_value_offset);
    }
    let value_only_offset = header_offset.checked_add(VALUE_ONLY_SCALAR_HEADER.len())?;
    let shifted_value_offset = value_only_offset.checked_add(4)?;
    let shifted_trailer_offset = shifted_value_offset.checked_add(8)?;
    if payload.get(header_offset..value_only_offset) == Some(VALUE_ONLY_SCALAR_HEADER)
        && payload.get(value_only_offset..shifted_value_offset) == Some(&[0; 4])
        && View::f64_le_at(payload, shifted_value_offset).is_some_and(f64::is_finite)
        && shifted_value_only_scalar_trailer(payload, shifted_trailer_offset)
    {
        return Some(shifted_value_offset);
    }
    (payload.get(header_offset..value_only_offset) == Some(VALUE_ONLY_SCALAR_HEADER))
        .then_some(value_only_offset)
}

pub(crate) fn scalar_indices_match(
    actual: &[FeatureInputScalar],
    expected: &[FeatureInputScalar],
) -> bool {
    actual.len() == expected.len()
        && actual.iter().zip(expected).all(|(actual, expected)| {
            actual.id == expected.id
                && actual.parent == expected.parent
                && actual.feature_ref == expected.feature_ref
                && actual.ordinal == expected.ordinal
                && actual.offset == expected.offset
                && actual.object_id == expected.object_id
                && actual.name == expected.name
                && ulp_distance(actual.value, expected.value) <= 4
                && actual.role == expected.role
                && actual.entity_indices == expected.entity_indices
                && actual.operands == expected.operands
        })
}

fn ulp_distance(left: f64, right: f64) -> u64 {
    fn ordered(value: f64) -> u64 {
        let bits = value.to_bits();
        if bits & (1 << 63) == 0 {
            bits | (1 << 63)
        } else {
            !bits
        }
    }
    ordered(left).abs_diff(ordered(right))
}

fn scalar_operands(
    payload: &[u8],
    trailer_offset: usize,
    parent: &str,
) -> Vec<FeatureInputOperand> {
    let lane_key = parent.rsplit_once('#').map_or(parent, |(_, key)| key);
    if compact_scalar_layout(payload, trailer_offset) {
        return [35, 43]
            .into_iter()
            .filter_map(|relative| {
                let offset = trailer_offset.checked_add(relative)?;
                let cell = payload.get(offset..offset + 8)?;
                if cell[4..8] != [0xff; 4] {
                    return None;
                }
                let kind = operand_kind([cell[0], cell[1]])?;
                Some(FeatureInputOperand {
                    offset: offset as u64,
                    reference_ref: format!("sldprt:feature-input:reference#{lane_key}:{offset}"),
                    kind,
                    entity_index: View::u16_le_at(cell, 2)?,
                    entity_ref: None,
                })
            })
            .collect();
    }
    let first = if legacy_scalar_layout(payload, trailer_offset) {
        36
    } else {
        35
    };
    [first, first + operand_cell::LEN]
        .into_iter()
        .filter_map(|relative| {
            let offset = trailer_offset.checked_add(relative)?;
            let cell = payload.get(offset..offset + operand_cell::LEN)?;
            if cell[operand_cell::REFERENCE_SENTINEL..operand_cell::ZERO_TRAILER] != [0xff; 4]
                || cell[operand_cell::ZERO_TRAILER..operand_cell::LEN] != [0; 4]
            {
                return None;
            }
            let kind = operand_kind([
                cell[operand_cell::CLASS_TOKEN],
                cell[operand_cell::CLASS_TOKEN + 1],
            ])?;
            Some(FeatureInputOperand {
                offset: offset as u64,
                reference_ref: format!("sldprt:feature-input:reference#{lane_key}:{offset}"),
                kind,
                entity_index: View::u16_le_at(cell, operand_cell::MARKER_ADDRESS)?,
                entity_ref: None,
            })
        })
        .collect()
}

pub(super) fn operand_kind(tag: [u8; 2]) -> Option<FeatureInputOperandKind> {
    match tag {
        [0, 0] | [0xff, 0xff] => None,
        [0xd6, 0x80] => Some(FeatureInputOperandKind::D6),
        [0xe1, 0x80] => Some(FeatureInputOperandKind::E1),
        bytes => Some(FeatureInputOperandKind::Native(View::u16_le_at(&bytes, 0)?)),
    }
}

pub(crate) fn feature_object_name<'a>(
    feature: &crate::records::Feature,
    lane: &'a FeatureInputLane,
) -> Option<&'a FeatureInputName> {
    if let Some(source_id) = feature
        .source_id
        .as_deref()
        .and_then(|value| value.parse::<u32>().ok())
    {
        let mut matches = lane
            .names
            .iter()
            .filter(|name| name.object_id == Some(source_id));
        if let Some(first) = matches.next() {
            if matches.next().is_none() {
                return Some(first);
            }
            return None;
        }
    }
    let mut matches = lane.names.iter().filter(|name| name.value == feature.name);
    let first = matches.next()?;
    matches.next().is_none().then_some(first)
}

#[cfg(test)]
mod scalars_tests;
