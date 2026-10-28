// SPDX-License-Identifier: Apache-2.0
//! Leading fields of model `dim_array` elements in model-data sections.

use cadmpeg_core::bytes::find_from;

use crate::psb;

/// `dim_type` of an angular model dimension. Its value is in degrees.
pub(crate) const ANGULAR_DIMENSION_TYPE: u32 = 21;

const DIMENSION_ARRAY_LABEL: &[u8] = b"\xe0\x00dim_array\0";
const DIMENSION_ELEMENT_TYPE: u8 = 0x08;
const ATTRIBUTE_FORM_PREFIX: u8 = 0xf5;
const FOUR_BYTE_INTEGER: u8 = 0xeb;

/// The `type`, `dim_type`, `feat_id`, and `dim_dat_ptr.value` fields of one
/// model `dim_array` element.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ModelDimension {
    /// Stored `dim_type`.
    pub(crate) dimension_type: u32,
    /// Stored `feat_id` of the owning feature.
    pub(crate) feature_id: u32,
    /// `dim_dat_ptr.value` when it is a three-byte short-form scalar.
    pub(crate) value: Option<f64>,
    /// Byte offset of the element `type` field.
    pub(crate) offset: usize,
}

/// Decode every element head after the named `dim_array` record of one
/// model-data section payload.
pub(crate) fn model_dimensions(payload: &[u8]) -> Vec<ModelDimension> {
    let Some(start) = find_from(payload, DIMENSION_ARRAY_LABEL, 0) else {
        return Vec::new();
    };
    (start + DIMENSION_ARRAY_LABEL.len()..payload.len())
        .filter(|&at| {
            payload.get(at) == Some(&DIMENSION_ELEMENT_TYPE) && element_opens_before(payload, at)
        })
        .filter_map(|at| element_head(payload, at))
        .collect()
}

fn element_opens_before(payload: &[u8], at: usize) -> bool {
    let before = |distance: usize| {
        at.checked_sub(distance)
            .and_then(|i| payload.get(i).copied())
    };
    let compound_close = Some(psb::token::COMPOUND_CLOSE);
    let entity_ref = Some(psb::token::ENTITY_REF);
    before(1) == compound_close
        || (before(1) == Some(0xe1) && before(2) == Some(0xe2) && before(3) == Some(0xe2))
        || (before(1).is_some_and(|b| b <= 0x7f)
            && before(2) == entity_ref
            && before(3) == compound_close)
        || (before(2).is_some_and(|b| (0x80..=0xbf).contains(&b))
            && before(3) == entity_ref
            && before(4) == compound_close)
}

fn element_head(payload: &[u8], at: usize) -> Option<ModelDimension> {
    let (dimension_type, cursor) = psb::complete_compact_int(payload, at + 1)?;
    let (feature_id, mut cursor) = psb::complete_compact_int(payload, cursor)?;
    if payload.get(cursor) == Some(&ATTRIBUTE_FORM_PREFIX) {
        cursor += 1;
    }
    for _ in 0..2 {
        cursor = attribute_integer_end(payload, cursor)?;
    }
    if payload.get(cursor) != Some(&psb::token::COMPOUND_CLOSE) {
        return None;
    }
    Some(ModelDimension {
        dimension_type,
        feature_id,
        value: psb::short_form_float(payload, cursor + 1).map(|(value, _)| value),
        offset: at,
    })
}

fn attribute_integer_end(payload: &[u8], at: usize) -> Option<usize> {
    match *payload.get(at)? {
        FOUR_BYTE_INTEGER => (at + 5 <= payload.len()).then_some(at + 5),
        _ => psb::parent_feature_id(payload, at).map(|(_, end)| end),
    }
}

#[cfg(test)]
mod tests;
