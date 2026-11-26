// SPDX-License-Identifier: Apache-2.0
//! Part and assembly records of the `Public Access` stream (`ironcad.md` §5).

use cadmpeg_core::decode::View;
use cadmpeg_ir::transform::Transform;

use crate::layout::scene_record_matrix;

/// Bytes after a record name that open the property set of a part or
/// assembly.
pub(crate) const PROPERTY_SET: [u8; 20] = [
    0x02, 0x00, 0x00, 0x00, 0x4a, 0x00, 0xe1, 0x3d, 0x2d, 0x92, 0xcf, 0x11, 0xac, 0x2e, 0x00, 0xa0,
    0x24, 0x58, 0x1e, 0x5c,
];

/// Marker before a record name: `ff ff fe` and a UTF-16 code-unit count.
pub(crate) const NAME_MARKER: [u8; 3] = [0xff, 0xff, 0xfe];

/// Marker before a property name.
pub(crate) const PROPERTY_MARKER: [u8; 2] = [0xf4, 0xff];

/// Value type tag of an f64 property.
pub(crate) const REAL_PROPERTY: u16 = 5;

/// Value type tag of a string property.
pub(crate) const STRING_PROPERTY: u16 = 8;

/// The value of a real property that the source leaves unset.
const UNSET_REAL: f64 = -1.0e300;

/// The longest property name, in bytes.
const MAX_PROPERTY_NAME: u32 = 256;

/// Metres to millimetres.
const M_TO_MM: f64 = 1000.0;

/// One part or assembly record.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SceneRecord {
    pub(crate) name: String,
    /// The record follows at least one feature record.
    pub(crate) has_features: bool,
    /// The stored matrix as an IR transform in millimetres.
    pub(crate) transform: Option<Transform>,
    /// The set properties, in stored order.
    pub(crate) properties: Vec<(String, PropertyValue)>,
}

/// The value of one property.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PropertyValue {
    Real(f64),
    Text(String),
}

impl SceneRecord {
    /// The value of the property `name`.
    pub(crate) fn property(&self, name: &str) -> Option<&PropertyValue> {
        self.properties
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value)
    }
}

impl SceneRecord {
    /// A part record holds its features; an assembly record holds none.
    pub(crate) const fn is_part(&self) -> bool {
        self.has_features
    }
}

/// Reads the part and assembly records of a `Public Access` payload in
/// stored order.
pub(crate) fn records(payload: &[u8]) -> Vec<SceneRecord> {
    let mut out = Vec::new();
    let mut features = false;
    let mut at = 0;
    while let Some(found) = find(payload, &NAME_MARKER, at) {
        at = found + NAME_MARKER.len();
        let Some(name) = name_at(payload, found) else {
            continue;
        };
        let after = found + NAME_MARKER.len() + 1 + 2 * name.encode_utf16().count();
        let property_set = payload
            .get(after..after + PROPERTY_SET.len())
            .is_some_and(|bytes| bytes == PROPERTY_SET);
        if !property_set {
            features = true;
            continue;
        }
        let next = next_name(payload, after).unwrap_or(payload.len());
        let parsed = payload.get(after..next).map(properties).unwrap_or_default();
        let transform = parsed
            .iter()
            .find(|property| property.name == "ModelGUID")
            .and_then(|guid| matrix(payload, after + guid.end + scene_record_matrix::MATRIX));
        let properties = parsed
            .into_iter()
            .filter(|property| property.value != PropertyValue::Real(UNSET_REAL))
            .filter(
                |property| !matches!(&property.value, PropertyValue::Text(text) if text.is_empty()),
            )
            .map(|property| (property.name, property.value))
            .collect();
        out.push(SceneRecord {
            name,
            has_features: features,
            transform,
            properties,
        });
        features = false;
        at = after;
    }
    out
}

/// The offset of the next marker at or after `from` that opens a non-empty
/// name.
fn next_name(payload: &[u8], mut from: usize) -> Option<usize> {
    loop {
        let found = find(payload, &NAME_MARKER, from)?;
        if name_at(payload, found).is_some() {
            return Some(found);
        }
        from = found + NAME_MARKER.len();
    }
}

pub(crate) fn find(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    haystack
        .get(from..)?
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|offset| from + offset)
}

/// The printable UTF-16LE name after the marker at `at`.
pub(crate) fn name_at(payload: &[u8], at: usize) -> Option<String> {
    let count = usize::from(*payload.get(at + NAME_MARKER.len())?);
    if count == 0 {
        return None;
    }
    let start = at + NAME_MARKER.len() + 1;
    let units: Vec<u16> = (0..count)
        .map(|index| View::u16_le_at(payload, start + 2 * index))
        .collect::<Option<_>>()?;
    let name = String::from_utf16(&units).ok()?;
    (name.chars().all(|c| !c.is_control())).then_some(name)
}

/// One stored property and the offset after its value.
struct Property {
    name: String,
    value: PropertyValue,
    end: usize,
}

/// Reads the properties of one property set.
///
/// A property is `f4 ff`, a `u32` name length in bytes, the NUL-terminated
/// UTF-16LE name, a `u16` value type, and the value. Properties of other
/// value types end the read.
fn properties(record: &[u8]) -> Vec<Property> {
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(found) = find(record, &PROPERTY_MARKER, at) {
        at = found + PROPERTY_MARKER.len();
        let Some(length) = View::u32_le_at(record, at)
            .filter(|length| *length >= 2 && *length <= MAX_PROPERTY_NAME && length % 2 == 0)
        else {
            continue;
        };
        let Some(name) = utf16(record, at + 4, length) else {
            continue;
        };
        let Ok(name_bytes) = usize::try_from(length) else {
            continue;
        };
        let value_at = at + 4 + name_bytes;
        let Some(kind) = View::u16_le_at(record, value_at) else {
            break;
        };
        let (value, end) = match kind {
            REAL_PROPERTY => {
                let Some(value) = View::f64_le_at(record, value_at + 2) else {
                    break;
                };
                (PropertyValue::Real(value), value_at + 10)
            }
            STRING_PROPERTY => {
                let Some(length) = View::u32_le_at(record, value_at + 2) else {
                    break;
                };
                let Some(text) = utf16(record, value_at + 6, length) else {
                    break;
                };
                let Ok(text_bytes) = usize::try_from(length) else {
                    break;
                };
                (PropertyValue::Text(text), value_at + 6 + text_bytes)
            }
            _ => continue,
        };
        out.push(Property { name, value, end });
        at = end;
    }
    out
}

/// The NUL-terminated UTF-16LE text of `length` bytes at `at`, without its
/// NUL.
fn utf16(bytes: &[u8], at: usize, length: u32) -> Option<String> {
    let units = usize::try_from(length).ok()? / 2;
    let units: Vec<u16> = (0..units)
        .map(|index| View::u16_le_at(bytes, at + 2 * index))
        .collect::<Option<_>>()?;
    let (&last, text) = units.split_last()?;
    (last == 0).then_some(())?;
    let text = String::from_utf16(text).ok()?;
    text.chars().all(|c| !c.is_control()).then_some(text)
}

/// The row-vector 4 × 4 matrix at `at` as a column-vector IR transform.
///
/// A point maps as `p · R + t`, where rows 1 to 3 hold `R` and row 4 holds
/// `t` in metres.
fn matrix(payload: &[u8], at: usize) -> Option<Transform> {
    let value = |index: usize| View::f64_le_at(payload, at + 8 * index);
    let m: Vec<f64> = (0..16).map(value).collect::<Option<_>>()?;
    let affine = m[3] == 0.0 && m[7] == 0.0 && m[11] == 0.0 && m[15] == 1.0;
    if !affine {
        return None;
    }
    let rows = std::array::from_fn(|row| [m[row], m[4 + row], m[8 + row], m[12 + row] * M_TO_MM]);
    Transform::affine(rows)
}

#[cfg(test)]
mod tests;
