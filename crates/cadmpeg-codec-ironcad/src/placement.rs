// SPDX-License-Identifier: Apache-2.0
//! Part and assembly records of the `Public Access` stream (`ironcad.md` §5).

use cadmpeg_core::decode::View;
use cadmpeg_ir::transform::Transform;

use crate::layout::{scene_object_header, scene_record_matrix, scene_record_matrix_short};

/// Bytes after a record name that open a feature record.
pub(crate) const FEATURE: [u8; 6] = [0, 0, 0, 0, 1, 0];

/// The last 12 bytes of every scene object class GUID.
pub(crate) const CLASS_TAIL: [u8; 12] = [
    0x2d, 0x92, 0xcf, 0x11, 0xac, 0x2e, 0x00, 0xa0, 0x24, 0x58, 0x1e, 0x5c,
];

/// Class of a part or assembly record.
pub(crate) const PART_CLASS: u32 = 0x3de1_004a;

/// Class of a part copy record.
pub(crate) const PART_COPY_CLASS: u32 = 0x3de1_004b;

/// Version of a part copy record that stores a part.
pub(crate) const PART_COPY_VERSION: u32 = 2;

/// Version of a part copy record that places one more occurrence.
pub(crate) const COPY_VERSION: u32 = 1;

/// Bytes before the identifier of a scene object.
pub(crate) const OBJECT_MARKER: [u8; 6] = [0xff, 0xff, 0xff, 0xff, 0xe1, 0xff];

/// Zero bytes before a copy path.
const PATH_LEAD: usize = 12;

/// The word that opens a copy path.
const PATH_OPEN: u32 = 1;

/// Most copy paths kept for one record.
const MAX_PATHS: usize = 4;

/// Most identifiers in one copy path.
const MAX_PATH_DEPTH: usize = 32;

/// Versions of a part or assembly record.
pub(crate) const PART_VERSIONS: [u32; 2] = [2, 3];

/// Class of the body property set that a version 3 part record holds.
pub(crate) const BODY_SET_CLASS: u32 = 0x3de5_0020;

/// Marker before a record name: `ff ff fe` and a UTF-16 code-unit count.
pub(crate) const NAME_MARKER: [u8; 3] = [0xff, 0xff, 0xfe];

/// Marker before a property name.
pub(crate) const PROPERTY_MARKER: [u8; 2] = [0xf4, 0xff];

/// Value type tag of an f64 property.
pub(crate) const REAL_PROPERTY: u16 = 5;

/// Value type tag of a string property.
pub(crate) const STRING_PROPERTY: u16 = 8;

/// Value type tag of an i32 property.
pub(crate) const INTEGER_PROPERTY: u16 = 3;

/// Value type tag of a property without a value.
pub(crate) const EMPTY_PROPERTY: u16 = 0;

/// Bytes in a stored matrix.
const MATRIX_BYTES: usize = 128;

/// Largest deviation of a dot product of two rows of a searched rotation
/// from that of an orthonormal basis.
const ORTHONORMAL_TOLERANCE: f64 = 1.0e-9;

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
    pub(crate) role: Role,
    /// The identifier after the first object marker after the previous
    /// record.
    pub(crate) id: Option<u32>,
    /// A version 1 part copy record.
    pub(crate) copy: bool,
    /// The copy paths of a copy record in stored order: each lists the
    /// identifiers of a scene object and its ancestors from the root down.
    pub(crate) paths: Vec<Vec<u32>>,
    /// The stored matrix as an IR transform in millimetres.
    pub(crate) transform: Option<Transform>,
    /// The matrix lies at no fixed offset and was found by a search.
    pub(crate) matrix_searched: bool,
    /// A class `0x3de1004a` record stores a version outside
    /// [`PART_VERSIONS`].
    pub(crate) version_unrecognized: bool,
    /// The class and the properties of a property set that a part or
    /// assembly record holds between its `ModelGUID` text and its matrix.
    pub(crate) embedded: Option<(u32, Vec<(String, PropertyValue)>)>,
    /// The set properties, in stored order.
    pub(crate) properties: Vec<(String, PropertyValue)>,
    /// The name of a property whose value type the reader does not read; the
    /// properties after it are not read.
    pub(crate) unread_property: Option<String>,
}

/// What a scene record places.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Role {
    Part,
    Assembly,
}

/// The value of one property.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PropertyValue {
    Real(f64),
    Integer(i32),
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
    pub(crate) fn is_part(&self) -> bool {
        self.role == Role::Part
    }
}

/// Reads the part and assembly records of a `Public Access` payload in
/// stored order.
pub(crate) fn records(payload: &[u8]) -> Vec<SceneRecord> {
    let markers = object_markers(payload);
    let mut out = Vec::new();
    let mut features = false;
    let mut at = 0;
    let mut previous = 0;
    while let Some(found) = find(payload, &NAME_MARKER, at) {
        at = found + NAME_MARKER.len();
        let Some(name) = record_name(payload, found) else {
            continue;
        };
        let after = found + NAME_MARKER.len() + 1 + 2 * name.encode_utf16().count();
        if payload.get(after..after + FEATURE.len()) == Some(FEATURE.as_slice()) {
            features = true;
            continue;
        }
        let Some((version, class)) = object_header(payload, after) else {
            continue;
        };
        let copy = class == PART_COPY_CLASS && version == COPY_VERSION;
        let role = match class {
            PART_CLASS if features => Role::Part,
            PART_CLASS => Role::Assembly,
            PART_COPY_CLASS if version == PART_COPY_VERSION || copy => Role::Part,
            _ => continue,
        };
        if class == PART_CLASS {
            features = false;
        }
        let opened = first_marker(&markers, previous, found);
        let next = next_name(payload, after).unwrap_or(payload.len());
        let record = payload.get(after..next).unwrap_or_default();
        let mut scene = scene_record(name, role, class, record);
        scene.version_unrecognized = class == PART_CLASS && !PART_VERSIONS.contains(&version);
        scene.id = opened.map(|(_, id)| id);
        scene.copy = copy;
        if copy {
            let from = opened.map_or(previous, |(position, _)| position);
            scene.paths = payload.get(from..found).map(copy_paths).unwrap_or_default();
        }
        out.push(scene);
        previous = after;
        at = after;
    }
    out
}

/// The name of the record whose name marker is at `at`: an empty name when
/// the count is `0` and the header of a version 1 or 2 part copy record
/// follows.
fn record_name(payload: &[u8], at: usize) -> Option<String> {
    if payload.get(at + NAME_MARKER.len()) == Some(&0) {
        return object_header(payload, at + NAME_MARKER.len() + 1)
            .filter(|(version, class)| {
                *class == PART_COPY_CLASS
                    && (*version == COPY_VERSION || *version == PART_COPY_VERSION)
            })
            .map(|_| String::new());
    }
    name_at(payload, at)
}

/// The position and identifier of every object marker, in stored order.
fn object_markers(payload: &[u8]) -> Vec<(usize, u32)> {
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(found) = find(payload, &OBJECT_MARKER, at) {
        at = found + 1;
        if let Some(id) = View::u32_le_at(payload, found + OBJECT_MARKER.len()) {
            out.push((found, id));
        }
    }
    out
}

/// The first object marker at or after `from` and before `to`.
fn first_marker(markers: &[(usize, u32)], from: usize, to: usize) -> Option<(usize, u32)> {
    let start = markers.partition_point(|(position, _)| *position < from);
    markers
        .get(start)
        .copied()
        .filter(|(position, _)| *position < to)
}

/// The copy paths of `bytes`: twelve zero bytes, the word `1`, and the
/// identifiers, each greater than `1`, up to a word of `0` or `1`.
fn copy_paths(bytes: &[u8]) -> Vec<Vec<u32>> {
    let mut out = Vec::new();
    let mut at = PATH_LEAD;
    while at < bytes.len() && out.len() < MAX_PATHS {
        let open = bytes[at - PATH_LEAD..at].iter().all(|byte| *byte == 0)
            && View::u32_le_at(bytes, at) == Some(PATH_OPEN);
        if !open {
            at += 1;
            continue;
        }
        let path: Vec<u32> = (1..=MAX_PATH_DEPTH)
            .map_while(|index| View::u32_le_at(bytes, at + 4 * index))
            .take_while(|id| *id > PATH_OPEN)
            .collect();
        at += 4 * (path.len() + 1);
        if !path.is_empty() {
            out.push(path);
        }
    }
    out
}

/// The version and class of the scene object header at `at`.
fn object_header(payload: &[u8], at: usize) -> Option<(u32, u32)> {
    let tail = at + scene_object_header::CLASS_TAIL;
    let tail_matches = payload.get(tail..tail + CLASS_TAIL.len()) == Some(CLASS_TAIL.as_slice());
    tail_matches.then_some(())?;
    Some((
        View::u32_le_at(payload, at + scene_object_header::VERSION)?,
        View::u32_le_at(payload, at + scene_object_header::CLASS)?,
    ))
}

/// Reads the properties and the matrix of the scene record of `class` whose
/// bytes after the name are `record`.
///
/// A part copy record stores its `ModelGUID` first, then a part property set
/// that ends at the matrix flag. A part or assembly record can hold a
/// property set of another class in the same place.
fn scene_record(name: String, role: Role, class: u32, record: &[u8]) -> SceneRecord {
    let (mut parsed, mut unread_property) = properties(record);
    let guid_end = parsed
        .iter()
        .find(|property| property.name == "ModelGUID")
        .map(|guid| guid.end);
    let located = guid_end.and_then(|end| locate_matrix(record, end));
    let between = guid_end
        .and_then(|start| record.get(start..located.map_or(record.len(), |(at, _, _)| at)))
        .unwrap_or_default();
    let mut embedded = None;
    if class == PART_COPY_CLASS {
        let (copied, unread) = properties(between);
        parsed.extend(copied);
        unread_property = unread_property.or(unread);
    } else if let Some((set_class, set)) = embedded_set(between) {
        let (found, unread) = properties(set);
        unread_property = unread_property.or(unread);
        embedded = Some((set_class, kept(found)));
    }
    SceneRecord {
        name,
        role,
        id: None,
        copy: false,
        paths: Vec::new(),
        transform: located.map(|(_, transform, _)| transform),
        matrix_searched: located.is_some_and(|(_, _, searched)| searched),
        version_unrecognized: false,
        embedded,
        properties: kept(parsed),
        unread_property,
    }
}

/// The class of the first property set that `bytes` holds and the bytes
/// after its class GUID.
fn embedded_set(bytes: &[u8]) -> Option<(u32, &[u8])> {
    let tail = find(bytes, &CLASS_TAIL, scene_object_header::CLASS_TAIL)?;
    let class = View::u32_le_at(
        bytes,
        tail - scene_object_header::CLASS_TAIL + scene_object_header::CLASS,
    )?;
    Some((class, bytes.get(tail + CLASS_TAIL.len()..)?))
}

/// The set properties of `parsed` as name and value pairs.
fn kept(parsed: Vec<Property>) -> Vec<(String, PropertyValue)> {
    parsed
        .into_iter()
        .filter(|property| property.value != PropertyValue::Real(UNSET_REAL))
        .filter(|property| !matches!(&property.value, PropertyValue::Text(text) if text.is_empty()))
        .map(|property| (property.name, property.value))
        .collect()
}

/// The offset of the matrix of `record`, the matrix, and whether a search
/// found it, given the end of the `ModelGUID` text at `guid_end`.
///
/// The short and long forms place the matrix at a fixed offset. Otherwise
/// the matrix is the first rigid matrix after a flag byte.
fn locate_matrix(record: &[u8], guid_end: usize) -> Option<(usize, Transform, bool)> {
    let flag = |offset: usize, value: u8| record.get(guid_end + offset) == Some(&value);
    let fixed = if flag(
        scene_record_matrix_short::FLAG,
        scene_record_matrix_short::FLAG_VALUE,
    ) {
        Some(guid_end + scene_record_matrix_short::MATRIX)
    } else if flag(scene_record_matrix::FLAG, scene_record_matrix::FLAG_VALUE) {
        Some(guid_end + scene_record_matrix::MATRIX)
    } else {
        None
    };
    if let Some(found) =
        fixed.and_then(|at| matrix(record, at).map(|transform| (at, transform, false)))
    {
        return Some(found);
    }
    let last = record.len().checked_sub(MATRIX_BYTES)?;
    (guid_end + 1..=last)
        .filter(|&at| record[at - 1] == scene_record_matrix::FLAG_VALUE && rigid(record, at))
        .find_map(|at| matrix(record, at).map(|transform| (at, transform, true)))
}

/// The 4 × 4 matrix at `at` holds an orthonormal 3 × 3 block and an affine
/// column.
fn rigid(bytes: &[u8], at: usize) -> bool {
    let value = |index: usize| View::f64_le_at(bytes, at + 8 * index);
    if value(15) != Some(1.0) {
        return false;
    }
    let Some(m) = (0..16).map(value).collect::<Option<Vec<f64>>>() else {
        return false;
    };
    let row = |index: usize| [m[4 * index], m[4 * index + 1], m[4 * index + 2]];
    (0..3).all(|a| {
        (0..3).all(|b| {
            let dot: f64 = (0..3).map(|k| row(a)[k] * row(b)[k]).sum();
            let expected = if a == b { 1.0 } else { 0.0 };
            (dot - expected).abs() <= ORTHONORMAL_TOLERANCE
        })
    })
}

/// The offset of the next marker at or after `from` that opens a record.
fn next_name(payload: &[u8], mut from: usize) -> Option<usize> {
    loop {
        let found = find(payload, &NAME_MARKER, from)?;
        if record_name(payload, found).is_some() {
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
/// UTF-16LE name, a `u16` value type, and the value. `ModelGUID` is the last
/// property read. The name of the first property of another value type is
/// returned, and the read continues at the next property marker.
fn properties(record: &[u8]) -> (Vec<Property>, Option<String>) {
    let mut out = Vec::new();
    let mut unread = None;
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
            INTEGER_PROPERTY => {
                let Some(value) = View::i32_le_at(record, value_at + 2) else {
                    break;
                };
                (PropertyValue::Integer(value), value_at + 6)
            }
            EMPTY_PROPERTY => {
                at = value_at + 2;
                continue;
            }
            _ => {
                unread.get_or_insert(name);
                at = value_at + 2;
                continue;
            }
        };
        let last = name == "ModelGUID";
        out.push(Property { name, value, end });
        if last {
            break;
        }
        at = end;
    }
    (out, unread)
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
