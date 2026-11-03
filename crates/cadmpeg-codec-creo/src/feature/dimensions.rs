// SPDX-License-Identifier: Apache-2.0
//! Dimension rows from the binary `FullMData` and `BasFullData` `dim_array`
//! and the round design-radius join shared with the legacy ASCII graph.

use cadmpeg_core::bytes::{find_from, find_iter};
use cadmpeg_core::decode::View;
use cadmpeg_ir::scalar::PositiveReal;

/// `dim_type` of a round's radius dimension.
const ROUND_DIMENSION_KIND: u32 = 3;
/// `dim_type` of an angular dimension. Its value is in degrees.
pub(crate) const ANGULAR_DIMENSION_TYPE: u32 = 21;

const DIM_ARRAY_FIELD: &[u8] = b"\xe0\x00dim_array\x00";
/// Label and opener of the `dtl_named_item` compound.
const NAMED_ITEM_LABEL: &[u8] = b"\xe0\x00dtl_named_item\x00\xe2";
/// Label and opener of the `dtl_item` compound and the `name` label.
const ITEM_LABELS: &[u8] = b"\xe0\x00dtl_item\x00\xe2\xe0\x0aname\x00";
const TYPE_LABEL: &[u8] = b"\xe0\x01type\x00";
/// Unlabeled `dtl_named_item` and `dtl_item` openers and the null `name`.
const NAMED_ITEM_FIELDS: &[u8] = b"\xe2\xe2\xe1";
const NULL_ELEMENT: u8 = 0xe1;
const NULL_NAME: u8 = 0xe1;
/// Opener of an element and of its `dim_dat_ptr` record.
const COMPOUND_OPEN: u8 = 0xe3;
const CLASS_REFERENCE: u8 = 0xf7;
const DIMENSION_TYPE: u8 = 0x08;
/// Null `feat_id`: the dimension has no owning feature.
const NULL_FEATURE: u8 = 0xf6;
/// Prototype framing byte before a value; it is not part of the value.
const PROTOTYPE_FRAMING: u8 = 0xf1;
const ATTRIBUTE_PREFIX: u8 = 0xf5;
/// Opener of a later element's unlabeled `symbol` compound.
const SYMBOL_OPEN: u8 = 0xe2;
const SYMBOL_LABEL: &[u8] = b"\xe0\x00symbol\x00";
const SYMBOL_TOKEN: &[u8] = b"\xf4\x04";
const SYMBOL_TYPE_LABEL: &[u8] = b"\xe2\xe0\x01type\x00";
const SYMBOL_INDEX_LABEL: &[u8] = b"\xe0\x01text(i_val)\x00";
/// Closer of the prototype's labeled `symbol` compound.
const PROTOTYPE_SYMBOL_CLOSE: u8 = 0xf2;

/// Whether the elements of one `dim_array` store the `dtl_named_item` fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ElementForm {
    /// Each element stores the `dtl_named_item` and `dtl_item` compounds and
    /// `name` before `type`.
    NamedItem,
    /// Each element starts at `type`.
    Bare,
}

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

/// Read every dimension element of the unique `dim_array` in `payload`. An
/// array whose slots do not all read supplies no rows.
pub(crate) fn dimensions(payload: &[u8]) -> Vec<ModelDimension> {
    let mut fields = find_iter(payload, DIM_ARRAY_FIELD);
    let (Some(field), None) = (fields.next(), fields.next()) else {
        return Vec::new();
    };
    slots(payload, field).unwrap_or_default()
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

/// The rows of the `<count>` slots after
/// `dim_array\0 [f1 | f3 | f4 04] f8 <count> f7 <class> fb`.
fn slots(payload: &[u8], field: usize) -> Option<Vec<ModelDimension>> {
    let mut view = View::over_retained(payload);
    view.seek(field.checked_add(DIM_ARRAY_FIELD.len())?)?;
    optional_token(&mut view)?;
    expect(&mut view, &[0xf8])?;
    let count = view.counted(u64::from(compact(&mut view)?), 1)?.get();
    expect(&mut view, &[CLASS_REFERENCE])?;
    compact(&mut view)?;
    expect(&mut view, &[0xfb])?;
    let mut rows = Vec::new();
    let mut layout = None;
    for slot in 0..count {
        while view.unread().first() == Some(&CLASS_REFERENCE) {
            view.skip(1)?;
            compact(&mut view)?;
        }
        match *view.unread().first()? {
            NULL_ELEMENT => view.skip(1)?,
            COMPOUND_OPEN => match layout {
                None => {
                    let (row, prototype_layout) = prototype(payload, &mut view, slot)?;
                    rows.push(row);
                    layout = Some(prototype_layout);
                }
                Some(layout) => {
                    let last = slot.checked_add(1)? == count;
                    let mut fields = later_element(payload, &mut view, slot, last)?;
                    rows.extend(positional_fields(&mut fields, layout));
                }
            },
            _ => return None,
        }
    }
    Some(rows)
}

/// The field form and `attr_arr` length that the prototype states for every
/// element of its array.
#[derive(Debug, Clone, Copy)]
struct Layout {
    form: ElementForm,
    attributes: usize,
}

/// The labeled first element at `slot`, read up to its `symbol` field
/// `e0 00 symbol\0 [f4 04] e2 e0 01 type\0 <type> e0 01 text(i_val)\0 <slot> f2`.
/// The view ends after the `f2`.
fn prototype(payload: &[u8], view: &mut View<'_>, slot: usize) -> Option<(ModelDimension, Layout)> {
    let opener = view.position();
    let symbol = find_from(payload, SYMBOL_LABEL, opener)?;
    view.seek(symbol.checked_add(SYMBOL_LABEL.len())?)?;
    if view.unread().starts_with(SYMBOL_TOKEN) {
        view.skip(SYMBOL_TOKEN.len())?;
    }
    expect(view, SYMBOL_TYPE_LABEL)?;
    compact(view)?;
    expect(view, SYMBOL_INDEX_LABEL)?;
    (usize::try_from(compact(view)?).ok()? == slot).then_some(())?;
    expect(view, &[PROTOTYPE_SYMBOL_CLOSE])?;
    let mut fields = View::over_retained(payload.get(..symbol)?);
    fields.seek(opener)?;
    prototype_fields(&mut fields)
}

fn prototype_fields(view: &mut View<'_>) -> Option<(ModelDimension, Layout)> {
    expect(view, &[COMPOUND_OPEN])?;
    let offset = view.position();
    let form = if view.unread().starts_with(NAMED_ITEM_LABEL) {
        view.skip(NAMED_ITEM_LABEL.len())?;
        if view.unread().starts_with(ITEM_LABELS) {
            view.skip(ITEM_LABELS.len())?;
        } else {
            expect(view, &[CLASS_REFERENCE])?;
            compact(view)?;
            expect(view, &[0xe2])?;
        }
        labeled_byte(view, NULL_NAME)?;
        ElementForm::NamedItem
    } else {
        ElementForm::Bare
    };
    expect(view, TYPE_LABEL)?;
    labeled_byte(view, DIMENSION_TYPE)?;
    expect(view, b"\xe0\x01dim_type\x00")?;
    framing(view)?;
    let dim_type = compact(view)?;
    expect(view, b"\xe0\x01feat_id\x00")?;
    framing(view)?;
    let feature_id = if null_feature(view)? {
        None
    } else {
        Some(compact(view)?)
    };
    expect(view, b"\xe0\x01attr_arr\x00")?;
    framing(view)?;
    expect(view, &[0xf8])?;
    let attributes = view.counted(u64::from(compact(view)?), 1)?.get();
    attribute_values(view, attributes)?;
    expect(view, b"\xe0\x00dim_dat_ptr\x00")?;
    labeled_byte(view, COMPOUND_OPEN)?;
    expect(view, b"\xe0\x02value\x00")?;
    framing(view)?;
    let row = ModelDimension {
        offset,
        dim_type,
        feature_id,
        value: value(view),
    };
    Some((row, Layout { form, attributes }))
}

/// The fields of the later element at `slot`, which ends with its `symbol`
/// field `e2 <type> <slot>`. The view ends after the symbol.
fn later_element<'a>(
    payload: &'a [u8],
    view: &mut View<'_>,
    slot: usize,
    last: bool,
) -> Option<View<'a>> {
    let opener = view.position();
    let (symbol, end) = later_symbol(payload, opener.checked_add(1)?, slot, last)?;
    view.seek(end)?;
    let mut fields = View::over_retained(payload.get(..symbol)?);
    fields.seek(opener)?;
    Some(fields)
}

/// The offset and end of the first `e2 <type> <i_val>` at or after `from`
/// whose `i_val` is `slot` and, unless `slot` is the last slot, which a slot
/// opener `e1`, `e3`, or `f7` follows.
fn later_symbol(payload: &[u8], from: usize, slot: usize, last: bool) -> Option<(usize, usize)> {
    let rest = payload.get(from..)?;
    find_iter(rest, &[SYMBOL_OPEN]).find_map(|relative| {
        let symbol = from.checked_add(relative)?;
        let mut view = View::over_retained(payload);
        view.seek(symbol.checked_add(1)?)?;
        compact(&mut view)?;
        (usize::try_from(compact(&mut view)?).ok()? == slot).then_some(())?;
        let next = view.unread().first();
        (last
            || matches!(
                next,
                Some(&(NULL_ELEMENT | COMPOUND_OPEN | CLASS_REFERENCE))
            ))
        .then_some((symbol, view.position()))
    })
}

/// `e3`, an optional `f7 <class>`, and the prototype fields without labels,
/// framing bytes, or the `attr_arr` count. A dimension whose fields do not
/// read yields no row.
fn positional_fields(view: &mut View<'_>, layout: Layout) -> Option<ModelDimension> {
    expect(view, &[COMPOUND_OPEN])?;
    if view.unread().first() == Some(&CLASS_REFERENCE) {
        view.skip(1)?;
        compact(view)?;
    }
    let offset = view.position();
    if layout.form == ElementForm::NamedItem {
        expect(view, NAMED_ITEM_FIELDS)?;
    }
    expect(view, &[DIMENSION_TYPE])?;
    let dim_type = compact(view)?;
    let feature_id = if null_feature(view)? {
        None
    } else {
        Some(compact(view)?)
    };
    attribute_values(view, layout.attributes)?;
    expect(view, &[COMPOUND_OPEN])?;
    Some(ModelDimension {
        offset,
        dim_type,
        feature_id,
        value: value(view),
    })
}

/// Read `count` integer elements of `attr_arr`: compact integers,
/// `c0..df XX YY`, `eb` with four bytes, and `e4` repeats, each after any
/// `f5` prefixes.
fn attribute_values(view: &mut View<'_>, count: usize) -> Option<()> {
    for _ in 0..count {
        while view.unread().first() == Some(&ATTRIBUTE_PREFIX) {
            view.skip(1)?;
        }
        match view.unread().first()? {
            0x00..=0xbf => {
                compact(view)?;
            }
            0xc0..=0xdf => view.skip(3)?,
            0xeb => view.skip(5)?,
            0xe4 => view.skip(1)?,
            _ => return None,
        }
    }
    Some(())
}

/// Skip the optional `f1`, `f3`, or `f4 04` token.
fn optional_token(view: &mut View<'_>) -> Option<()> {
    match view.unread() {
        [0xf1 | 0xf3, ..] => view.skip(1),
        [0xf4, 0x04, ..] => view.skip(2),
        _ => Some(()),
    }
}

fn value(view: &mut View<'_>) -> Option<f64> {
    let head = view.u8()?;
    crate::assembly::frame_scalar(head, view)
}

/// Consume the null `feat_id` when it comes next.
fn null_feature(view: &mut View<'_>) -> Option<bool> {
    let null = view.unread().first() == Some(&NULL_FEATURE);
    if null {
        view.skip(1)?;
    }
    Some(null)
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
