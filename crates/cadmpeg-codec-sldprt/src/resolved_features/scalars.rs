//! Named scalar records, operands and feature object names.

use super::relation_records::{
    compact_scalar_layout, legacy_scalar_layout, scalar_role, shifted_value_only_scalar_trailer,
};
use super::{COMPACT_SCALAR_HEADER, NAME_MARKER, SCALAR_HEADER, VALUE_ONLY_SCALAR_HEADER};
use crate::records::{
    FeatureInputLane, FeatureInputName, FeatureInputOperand, FeatureInputOperandKind,
    FeatureInputScalar,
};
use cadmpeg_core::decode::{u64_from_index, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::scalar::FiniteReal;
use std::collections::HashMap;

use crate::layout::feature_input_operand_cell12 as operand_cell;
use crate::records::ObjectId;

pub(crate) fn named_scalars(
    payload: &[u8],
    parent: &str,
    names: &[FeatureInputName],
) -> Result<Vec<FeatureInputScalar>, CodecError> {
    let lane_key = parent.rsplit_once('#').map_or(parent, |(_, key)| key);
    let mut scalars = Vec::new();
    for name in names {
        let Some(name_offset) = usize::try_from(name.offset).ok() else {
            continue;
        };
        let Some(value_offset) = scalar_value_offset(payload, name_offset) else {
            continue;
        };
        let Some(value) = View::f64_le_at(payload, value_offset).and_then(FiniteReal::new) else {
            continue;
        };
        let Some(trailer_offset) = value_offset.checked_add(8) else {
            continue;
        };
        let Some(object_id) = trailer_offset
            .checked_add(3)
            .and_then(|offset| View::u32_le_at(payload, offset))
        else {
            continue;
        };
        let ordinal = u32::try_from(scalars.len())
            .map_err(|_| CodecError::malformed("SLDPRT named scalar ordinal overflows u32"))?;
        scalars.push(FeatureInputScalar {
            id: format!("sldprt:feature-input:scalar#{lane_key}:{value_offset}"),
            parent: parent.to_owned(),
            feature_ref: None,
            ordinal,
            offset: u64_from_index(value_offset),
            object_id,
            name: name.id.clone(),
            value,
            role: scalar_role(payload, trailer_offset),
            operands: scalar_operands(payload, trailer_offset, parent),
        });
    }
    Ok(scalars)
}

/// The scalar payload offset that follows the serialized object name at
/// `name_offset`.
///
/// The name length comes from the payload's own length byte, so the offset is a
/// function of the retained bytes alone and never of a stored name value.
fn scalar_value_offset(payload: &[u8], name_offset: usize) -> Option<usize> {
    let units = usize::from(*payload.get(name_offset.checked_add(NAME_MARKER.len())?)?);
    let header_offset = name_offset
        .checked_add(NAME_MARKER.len() + 1)?
        .checked_add(units.checked_mul(2)?)?;
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
                && ulp_distance(actual.value.get(), expected.value.get()) <= 4
                && actual.role == expected.role
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
    operand_cells(payload, trailer_offset)
        .into_iter()
        .flatten()
        .map(|(offset, kind, entity_index)| FeatureInputOperand {
            offset: u64_from_index(offset),
            reference_ref: format!("sldprt:feature-input:reference#{lane_key}:{offset}"),
            kind,
            entity_index,
            entity_ref: None,
        })
        .collect()
}

fn operand_cells(
    payload: &[u8],
    trailer_offset: usize,
) -> [Option<(usize, FeatureInputOperandKind, u16)>; 2] {
    let compact = compact_scalar_layout(payload, trailer_offset);
    let first = if compact || !legacy_scalar_layout(payload, trailer_offset) {
        35
    } else {
        36
    };
    let second = if compact {
        43
    } else {
        first + operand_cell::LEN
    };
    [first, second].map(|relative| {
        let offset = trailer_offset.checked_add(relative)?;
        if compact {
            let cell = payload.get(offset..offset.checked_add(8)?)?;
            if cell[4..8] != [0xff; 4] {
                return None;
            }
            return Some((
                offset,
                operand_kind([cell[0], cell[1]])?,
                View::u16_le_at(cell, 2)?,
            ));
        }
        let cell = payload.get(offset..offset.checked_add(operand_cell::LEN)?)?;
        if cell[operand_cell::REFERENCE_SENTINEL..operand_cell::ZERO_TRAILER] != [0xff; 4]
            || cell[operand_cell::ZERO_TRAILER..operand_cell::LEN] != [0; 4]
        {
            return None;
        }
        Some((
            offset,
            operand_kind([
                cell[operand_cell::CLASS_TOKEN],
                cell[operand_cell::CLASS_TOKEN + 1],
            ])?,
            View::u16_le_at(cell, operand_cell::MARKER_ADDRESS)?,
        ))
    })
}

pub(super) fn operand_kind(tag: [u8; 2]) -> Option<FeatureInputOperandKind> {
    match tag {
        [0, 0] | [0xff, 0xff] => None,
        [0xd6, 0x80] => Some(FeatureInputOperandKind::D6),
        [0xe1, 0x80] => Some(FeatureInputOperandKind::E1),
        bytes => Some(FeatureInputOperandKind::Native(
            View::u16_le_at(&bytes, 0)?.try_into().ok()?,
        )),
    }
}

/// Unique source and display-name lookups for repeated feature ownership queries.
pub(super) struct FeatureObjectNames<'a> {
    by_source: HashMap<u32, Option<&'a FeatureInputName>>,
    by_value: HashMap<&'a str, Option<&'a FeatureInputName>>,
}

impl<'a> FeatureObjectNames<'a> {
    pub(super) fn new(names: &'a [FeatureInputName]) -> Self {
        let mut by_source = HashMap::new();
        let mut by_value = HashMap::<&str, Option<&FeatureInputName>>::new();
        for name in names {
            if let Some(source) = name.object_id.and_then(ObjectId::value) {
                by_source
                    .entry(source)
                    .and_modify(|value| *value = None)
                    .or_insert(Some(name));
            }
            by_value
                .entry(name.value.as_str())
                .and_modify(|value| *value = None)
                .or_insert(Some(name));
        }
        Self {
            by_source,
            by_value,
        }
    }

    pub(super) fn get(&self, source: Option<u32>, name: &str) -> Option<&'a FeatureInputName> {
        if let Some(source) = source {
            if let Some(found) = self.by_source.get(&source) {
                return *found;
            }
        }
        self.by_value.get(name).copied().flatten()
    }
}

pub(crate) fn feature_object_name<'a>(
    feature: &crate::records::Feature,
    lane: &'a FeatureInputLane,
) -> Option<&'a FeatureInputName> {
    if let Some(source_id) = feature.source_value() {
        let mut matches = lane
            .names
            .iter()
            .filter(|name| name.object_id.and_then(ObjectId::value) == Some(source_id));
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
