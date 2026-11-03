// SPDX-License-Identifier: Apache-2.0
//! Dimension rows from the binary `BasFullData` `dim_array` and the round
//! design-radius join shared with the legacy ASCII graph.

use cadmpeg_core::bytes::{find, find_iter};
use cadmpeg_core::decode::View;
use cadmpeg_ir::scalar::PositiveReal;

/// `dim_type` of a round's radius dimension.
const ROUND_DIMENSION_KIND: u32 = 3;

const DIM_ARRAY_FIELD: &[u8] = b"\xe0\x00dim_array\x00";
/// Labels that the prototype record states up to its `type` value.
const PROTOTYPE_HEAD: &[u8] =
    b"\xe0\x00dtl_named_item\x00\xe2\xe0\x00dtl_item\x00\xe2\xe0\x0aname\x00";
/// Opener, null `name`, and dimension `type` of a positional row.
const POSITIONAL_HEAD: &[u8] = b"\xe2\xe2\xe1\x08";
const NULL_NAME: u8 = 0xe1;
const DIMENSION_TYPE: u8 = 0x08;
/// Null `feat_id`: the dimension has no owning feature.
const NULL_FEATURE: u8 = 0xf6;
/// Prototype framing byte before a value; it is not part of the value.
const PROTOTYPE_FRAMING: u8 = 0xf1;
/// Opener of the `dim_dat_ptr` record.
const DIMENSION_DATA: u8 = 0xe3;

/// The state of a round's design-radius dimension join.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum RoundDimensionRadius {
    /// The complete dimension table has no radius row for this feature.
    NotPresent,
    /// All feature-owned radius rows carry the same positive value.
    Constant(PositiveReal),
    /// A matching radius row is malformed, non-positive, or disagrees with
    /// another matching row.
    Ambiguous,
}

impl RoundDimensionRadius {
    /// Fold the values of every matching radius row; `None` is a row whose
    /// value does not decode.
    pub(crate) fn from_values(values: impl IntoIterator<Item = Option<f64>>) -> Self {
        let mut state = Self::NotPresent;
        for value in values {
            let Some(value) = value.and_then(PositiveReal::new) else {
                return Self::Ambiguous;
            };
            match state {
                Self::Constant(first) if first.get().to_bits() != value.get().to_bits() => {
                    return Self::Ambiguous;
                }
                Self::Constant(_) => {}
                _ => state = Self::Constant(value),
            }
        }
        state
    }
}

/// One dimension (`type` 8) element of the `dim_array`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ModelDimension {
    /// Byte offset of the element.
    pub(crate) offset: usize,
    /// Stored `dim_type`.
    pub(crate) dim_type: u32,
    /// Owning feature, or `None` for the null `feat_id`.
    pub(crate) feature_id: Option<u32>,
    /// `dim_dat_ptr.value` in the frame-slot scalar lane, or `None` when its
    /// token is outside that lane.
    pub(crate) value: Option<f64>,
}

/// Read every dimension element of the unique `dim_array` in `payload`.
pub(crate) fn dimensions(payload: &[u8]) -> Vec<ModelDimension> {
    let mut fields = find_iter(payload, DIM_ARRAY_FIELD);
    let (Some(field), None) = (fields.next(), fields.next()) else {
        return Vec::new();
    };
    let Some(body) = array_body(payload, field) else {
        return Vec::new();
    };
    let Some(elements) = payload.get(body..) else {
        return Vec::new();
    };
    let prototype_start = find(elements, PROTOTYPE_HEAD).map(|relative| body + relative);
    let positional_starts = find_iter(elements, POSITIONAL_HEAD).map(|relative| body + relative);
    let mut rows = Vec::new();
    for start in prototype_start.into_iter().chain(positional_starts) {
        let mut view = View::over_retained(payload);
        let row = view.seek(start).and_then(|()| {
            if Some(start) == prototype_start {
                prototype(&mut view, start)
            } else {
                positional(&mut view, start)
            }
        });
        rows.extend(row);
    }
    rows.sort_by_key(|row| row.offset);
    rows
}

/// The design radius that the dimension rows state for round `feature_id`.
pub(crate) fn round_radius(dimensions: &[ModelDimension], feature_id: u32) -> RoundDimensionRadius {
    RoundDimensionRadius::from_values(
        dimensions
            .iter()
            .filter(|row| {
                row.dim_type == ROUND_DIMENSION_KIND && row.feature_id == Some(feature_id)
            })
            .map(|row| row.value),
    )
}

/// First element byte after `dim_array\0 f8 <count> f7 <class> fb`.
fn array_body(payload: &[u8], field: usize) -> Option<usize> {
    let mut view = View::over_retained(payload);
    view.seek(field.checked_add(DIM_ARRAY_FIELD.len())?)?;
    expect(&mut view, &[0xf8])?;
    compact(&mut view)?;
    expect(&mut view, &[0xf7])?;
    compact(&mut view)?;
    expect(&mut view, &[0xfb])?;
    Some(view.position())
}

/// `dtl_named_item` with every field labeled: the first element of the array.
fn prototype(view: &mut View<'_>, offset: usize) -> Option<ModelDimension> {
    expect(view, PROTOTYPE_HEAD)?;
    labeled_byte(view, NULL_NAME)?;
    expect(view, b"\xe0\x01type\x00")?;
    labeled_byte(view, DIMENSION_TYPE)?;
    expect(view, b"\xe0\x01dim_type\x00")?;
    framing(view)?;
    let dim_type = compact(view)?;
    expect(view, b"\xe0\x01feat_id\x00")?;
    framing(view)?;
    let feature_id = feature(view)?;
    expect(view, b"\xe0\x01attr_arr\x00")?;
    framing(view)?;
    expect(view, &[0xf8])?;
    compact(view)?;
    attributes(view)?;
    expect(view, b"\xe0\x00dim_dat_ptr\x00")?;
    labeled_byte(view, DIMENSION_DATA)?;
    expect(view, b"\xe0\x02value\x00")?;
    framing(view)?;
    Some(ModelDimension {
        offset,
        dim_type,
        feature_id,
        value: value(view),
    })
}

/// A later element: the prototype fields without labels, framing bytes, or
/// the `attr_arr` count.
fn positional(view: &mut View<'_>, offset: usize) -> Option<ModelDimension> {
    expect(view, POSITIONAL_HEAD)?;
    let dim_type = compact(view)?;
    let feature_id = feature(view)?;
    attributes(view)?;
    expect(view, &[DIMENSION_DATA])?;
    Some(ModelDimension {
        offset,
        dim_type,
        feature_id,
        value: value(view),
    })
}

/// Skip the integer elements of `attr_arr`: compact integers, `c0..df XX YY`,
/// `eb` with four bytes, an `f5` prefix, and `e4..e6` repeats.
fn attributes(view: &mut View<'_>) -> Option<()> {
    loop {
        match view.unread().first()? {
            0x00..=0xbf => {
                compact(view)?;
            }
            0xc0..=0xdf => view.skip(3)?,
            0xeb => view.skip(5)?,
            0xe4..=0xe6 | 0xf5 => view.skip(1)?,
            _ => return Some(()),
        }
    }
}

fn value(view: &mut View<'_>) -> Option<f64> {
    let head = view.u8()?;
    crate::assembly::frame_scalar(head, view)
}

fn feature(view: &mut View<'_>) -> Option<Option<u32>> {
    if view.unread().first() == Some(&NULL_FEATURE) {
        view.skip(1)?;
        return Some(None);
    }
    compact(view).map(Some)
}

fn labeled_byte(view: &mut View<'_>, byte: u8) -> Option<()> {
    framing(view)?;
    expect(view, &[byte])
}

fn framing(view: &mut View<'_>) -> Option<()> {
    if view.unread().first() == Some(&PROTOTYPE_FRAMING) {
        view.skip(1)?;
    }
    Some(())
}

fn expect(view: &mut View<'_>, bytes: &[u8]) -> Option<()> {
    (view.take(bytes.len())? == bytes).then_some(())
}

fn compact(view: &mut View<'_>) -> Option<u32> {
    let (value, end) = crate::psb::complete_compact_int(view.window(), view.position())?;
    view.seek(end)?;
    Some(value)
}

#[cfg(test)]
mod tests;
