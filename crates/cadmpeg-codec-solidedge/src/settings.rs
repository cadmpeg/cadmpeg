// SPDX-License-Identifier: Apache-2.0
//! Document settings: the typed property sets of `PSMcluster0` and the
//! document unit system they select (`docs/formats/solidedge.md` §2).

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_ir::design::units::{UnitSymbol, UnitSystem};
use cadmpeg_ir::report::loss::LossNote;

use crate::layout::property_set_header as header;
use crate::loss::SolidEdgeLossCode;

/// The stream that holds the document property sets.
pub(crate) const CLUSTER_STREAM: &str = "PSMcluster0";

/// The smallest record: name count, a four-byte name, three words and a
/// four-byte value.
const MIN_RECORD_LEN: usize = 24;
/// The largest name field a record stores, in bytes.
const MAX_NAME_BYTES: usize = 512;

/// The record naming the document length unit.
const LENGTH_PROPERTY: &str = "DefaultDistancePrimary";
/// The record naming the document angle unit.
const ANGLE_PROPERTY: &str = "DefaultAnglePrimary";

/// One property value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Value {
    /// Types 1, 2 and 6: a little-endian `u32`.
    Word(u32),
    /// Type 4: a little-endian IEEE 754 binary64.
    Real(f64),
}

/// One named property record.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Property {
    pub(crate) name: String,
    pub(crate) value: Value,
}

/// One property set: its index and records in stored order.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PropertySet {
    pub(crate) index: u32,
    pub(crate) properties: Vec<Property>,
}

impl PropertySet {
    fn word(&self, name: &str) -> Option<u32> {
        self.properties
            .iter()
            .find(|property| property.name == name)
            .and_then(|property| match property.value {
                Value::Word(word) => Some(word),
                Value::Real(_) => None,
            })
    }
}

/// Every property set in `bytes` that walks to its declared record count.
///
/// A tag match that does not walk is not a property set: the stream holds
/// other object data, and the tag bytes can occur in it.
pub(crate) fn property_sets(ctx: &DecodeContext<'_>, bytes: &[u8]) -> Vec<PropertySet> {
    let mut sets = Vec::new();
    let mut at = 0;
    while let Some(found) = find(bytes, at) {
        match walk(ctx, bytes, found) {
            Some((set, end)) => {
                sets.push(set);
                at = end;
            }
            None => at = found + 1,
        }
    }
    sets
}

/// The next header start at or after `from`: a zero `u32` and the `u16`
/// set tag. The `u16` set form that follows does not change the grammar.
fn find(bytes: &[u8], from: usize) -> Option<usize> {
    let [low, high] = header::TAG_VALUE.to_le_bytes();
    let opening = [0, 0, 0, 0, low, high];
    bytes
        .get(from..)?
        .windows(opening.len())
        .position(|window| window == opening)
        .map(|offset| from + offset)
}

fn walk(ctx: &DecodeContext<'_>, bytes: &[u8], start: usize) -> Option<(PropertySet, usize)> {
    let index = View::u32_le_at(bytes, start.checked_add(header::INDEX)?)?;
    let count = usize::try_from(View::u32_le_at(bytes, start.checked_add(header::COUNT)?)?).ok()?;
    let mut at = start.checked_add(header::LEN)?;
    if count > bytes.len().checked_sub(at)? / MIN_RECORD_LEN {
        return None;
    }
    let mut properties = Vec::new();
    for _ in 0..count {
        let (property, end) = record(ctx, bytes, at)?;
        properties.push(property);
        at = end;
    }
    Some((PropertySet { index, properties }, at))
}

/// One record: `u32` name byte count `n`, `n` bytes of NUL-terminated
/// UTF-16LE name, `u32` identifier, `u32` flag, `u32` type, and the value.
fn record(ctx: &DecodeContext<'_>, bytes: &[u8], at: usize) -> Option<(Property, usize)> {
    let length = usize::try_from(View::u32_le_at(bytes, at)?).ok()?;
    if !(4..=MAX_NAME_BYTES).contains(&length) || length % 2 != 0 {
        return None;
    }
    let start = at.checked_add(4)?;
    let raw = bytes.get(start..start.checked_add(length)?)?;
    if raw[length - 2..] != [0, 0] {
        return None;
    }
    let name = ctx
        .utf16le_text(raw, length / 2 - 1, false, "Solid Edge property name")
        .ok()?;
    if name.is_empty() || name.chars().any(char::is_control) {
        return None;
    }
    let fields = start + length;
    let kind = View::u32_le_at(bytes, fields + 8)?;
    let value_at = fields + 12;
    let (value, width) = match kind {
        1 | 2 | 6 => (Value::Word(View::u32_le_at(bytes, value_at)?), 4),
        4 => (Value::Real(View::f64_le_at(bytes, value_at)?), 8),
        _ => return None,
    };
    Some((Property { name, value }, value_at + width))
}

/// The document unit system the property sets select, with a loss for each
/// unit the bytes do not settle.
pub(crate) fn unit_system(sets: &[PropertySet], losses: &mut Vec<LossNote>) -> Option<UnitSystem> {
    let Some(set) = sets.iter().find(|set| set.word(LENGTH_PROPERTY).is_some()) else {
        losses.push(SolidEdgeLossCode::UnitsUnread.note(format!(
            "no {CLUSTER_STREAM} property set stores {LENGTH_PROPERTY}; the document unit system is not transferred"
        )));
        return None;
    };
    let length = set.word(LENGTH_PROPERTY).and_then(|code| {
        let unit = length_unit(code);
        if unit.is_none() {
            losses.push(SolidEdgeLossCode::UnitCodeUnknown.note(format!(
                "{LENGTH_PROPERTY} code {code} names no known length unit"
            )));
        }
        unit
    });
    let angle = match set.word(ANGLE_PROPERTY) {
        Some(code) => {
            let unit = angle_unit(code);
            if unit.is_none() {
                losses.push(SolidEdgeLossCode::UnitCodeUnknown.note(format!(
                    "{ANGLE_PROPERTY} code {code} names no known angle unit"
                )));
            }
            unit
        }
        None => {
            losses.push(SolidEdgeLossCode::UnitsUnread.note(format!(
                "the property set storing {LENGTH_PROPERTY} stores no {ANGLE_PROPERTY}"
            )));
            None
        }
    };
    Some(UnitSystem {
        length: length?,
        angle: angle?,
        mass: None,
        time: None,
    })
}

/// `DefaultDistancePrimary` codes.
fn length_unit(code: u32) -> Option<UnitSymbol> {
    Some(match code {
        59 => UnitSymbol::M,
        61 => UnitSymbol::Mm,
        62 => UnitSymbol::Cm,
        64 => UnitSymbol::In,
        _ => return None,
    })
}

/// `DefaultAnglePrimary` codes.
fn angle_unit(code: u32) -> Option<UnitSymbol> {
    Some(match code {
        77 => UnitSymbol::Rad,
        357 => UnitSymbol::Deg,
        _ => return None,
    })
}

#[cfg(test)]
mod tests;
