// SPDX-License-Identifier: Apache-2.0
//! Part records of the `Public Access` stream (DR-1067).

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

/// Value type tag of a string property.
const STRING_PROPERTY: u16 = 8;

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
        let transform = payload
            .get(after..next)
            .and_then(model_guid_end)
            .and_then(|end| matrix(payload, after + end + scene_record_matrix::MATRIX));
        out.push(SceneRecord {
            name,
            has_features: features,
            transform,
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

fn find(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    haystack
        .get(from..)?
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|offset| from + offset)
}

/// The printable UTF-16LE name after the marker at `at`.
fn name_at(payload: &[u8], at: usize) -> Option<String> {
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

/// The offset after the `ModelGUID` string value within `record`.
fn model_guid_end(record: &[u8]) -> Option<usize> {
    let key: Vec<u8> = "ModelGUID\0"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    let at = find(record, &key, 0)? + key.len();
    if View::u16_le_at(record, at)? != STRING_PROPERTY {
        return None;
    }
    let length = usize::try_from(View::u32_le_at(record, at + 2)?).ok()?;
    let end = at + 6 + length;
    (end <= record.len()).then_some(end)
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
