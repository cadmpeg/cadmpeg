// SPDX-License-Identifier: Apache-2.0
//! Synthetic MFC `CArchive` streams and a part that carries them.
#![allow(clippy::unwrap_used)]

use super::container::{make_block, outer_header};

/// The value of the synthetic parameter object.
pub(crate) const PARAMETER_VALUE: f64 = 0.025;

/// The map count the synthetic `Config-0` stream ends at.
const CONFIGURATION_MAP_COUNT: u32 = 9;

fn declaration(name: &str) -> Vec<u8> {
    let mut bytes = vec![0xff, 0xff, 0x01, 0x00];
    bytes.extend_from_slice(&u16::try_from(name.len()).unwrap().to_le_bytes());
    bytes.extend_from_slice(name.as_bytes());
    bytes
}

fn tag(index: u16) -> [u8; 2] {
    (0x8000 | index).to_le_bytes()
}

/// The name object and named-object header of a parameter object, through the
/// u16 `0` that precedes its value.
pub(crate) fn parameter_prefix() -> Vec<u8> {
    let mut body = vec![0x04, 0x80, 0xff, 0xfe, 0xff, 0x01, b'A', 0x00];
    body.extend_from_slice(&[0; 4]);
    body.extend_from_slice(&[0, 0, 0, 0x40]);
    body.extend_from_slice(&[0xff; 4]);
    body.extend_from_slice(&[0; 4]);
    body.extend_from_slice(&[0xff, 0xfe, 0xff, 0x00]);
    body.extend_from_slice(&[0, 0]);
    body
}

/// The 35-byte trailer of a driving scalar with no tolerance and no
/// operands: a zero byte, a null tolerance pointer and the object identifier
/// `1`, then the operand count `0` and the role `0` (§2.3).
pub(crate) fn parameter_trailer() -> [u8; 35] {
    let mut trailer = [0u8; 35];
    trailer[3..7].copy_from_slice(&1_u32.to_le_bytes());
    trailer
}

/// A real parameter object body: the prefix, the f64 value, the trailer.
pub(crate) fn real_parameter_body(value: [u8; 8]) -> Vec<u8> {
    let mut body = parameter_prefix();
    body.extend_from_slice(&value);
    body.extend_from_slice(&parameter_trailer());
    body
}

/// An integer parameter object body: the prefix, the u32 value, the trailer.
pub(crate) fn integer_parameter_body(value: u32) -> Vec<u8> {
    let mut body = parameter_prefix();
    body.extend_from_slice(&value.to_le_bytes());
    body.extend_from_slice(&parameter_trailer());
    body
}

/// A `moDimTolerance_c` body (§2.3) with empty strings.
fn tolerance_body() -> Vec<u8> {
    let mut body = 1_u32.to_le_bytes().to_vec();
    body.extend_from_slice(&[0; 4]);
    for limit in [-0.000_1_f64, 0.000_1] {
        body.extend_from_slice(&limit.to_le_bytes());
    }
    body.push(1);
    body.extend_from_slice(&[0; 24]);
    body.extend(cstring(""));
    body.extend(cstring(""));
    body.push(0);
    body.extend_from_slice(&1_u32.to_le_bytes());
    body.push(1);
    body.extend_from_slice(&[0; 24 + 6]);
    for _ in 0..3 {
        body.extend(cstring(""));
    }
    body.extend_from_slice(&[0; 4]);
    body
}

/// The bytes after a parameter's value (§2.3): a zero byte, the tolerance
/// pointer (a `moDimTolerance_c` declared in place, or null), the object
/// identifier `id`, then the operand count `0` at identifier +24 and the role
/// `0` at identifier +26, 32 bytes from the identifier.
fn parameter_tail(id: u32, toleranced: bool) -> Vec<u8> {
    let mut tail = vec![0];
    if toleranced {
        tail.extend(declaration("moDimTolerance_c"));
        tail.extend(tolerance_body());
    } else {
        tail.extend_from_slice(&[0, 0]);
    }
    tail.extend_from_slice(&id.to_le_bytes());
    tail.extend_from_slice(&[0; 28]);
    tail
}

/// A `Config-0-ResolvedFeatures` payload for [`config_object_stream`]: a
/// `moIntegerParameter_c`, declared in place, whose identifier is `integer`,
/// and a `moLengthParameter_c` with a tolerance, whose identifier is
/// `toleranced`.
pub(crate) fn toleranced_and_integer_parameters_stream(integer: u32, toleranced: u32) -> Vec<u8> {
    let mut stream = CONFIGURATION_MAP_COUNT.to_le_bytes().to_vec();
    stream.extend_from_slice(&[0, 0]);
    stream.extend(declaration("moIntegerParameter_c"));
    stream.extend(parameter_prefix());
    stream.extend_from_slice(&7_u32.to_le_bytes());
    stream.extend(parameter_tail(integer, false));
    stream.extend_from_slice(&tag(7));
    stream.extend(parameter_prefix());
    stream.extend_from_slice(&PARAMETER_VALUE.to_le_bytes());
    stream.extend(parameter_tail(toleranced, true));
    stream
}

/// `Contents/Config-0`: `moPart_c` and `moNodeName_c` take indices 2 and 4,
/// one name object takes 6, and `moLengthParameter_c` takes 7, so the map ends
/// at 9.
pub(crate) fn config_object_stream() -> Vec<u8> {
    config_object_stream_declaring("moLengthParameter_c")
}

/// [`config_object_stream`] with `class` in place of `moLengthParameter_c`
/// at index 7.
pub(crate) fn config_object_stream_declaring(class: &str) -> Vec<u8> {
    // The walk reads the `swVersion` fallback from offset 22.
    let mut stream = vec![0; 22];
    stream.extend_from_slice(&17_000_u32.to_le_bytes());
    stream.extend(declaration("moPart_c"));
    stream.extend_from_slice(&[0, 0]);
    stream.extend(declaration("moNodeName_c"));
    stream.extend_from_slice(&[0xff, 0xfe, 0xff, 0x01, b'A', 0]);
    stream.extend_from_slice(&tag(4));
    stream.extend_from_slice(&[0xff, 0xfe, 0xff, 0x01, b'B', 0]);
    stream.extend(declaration(class));
    stream
}

/// A framed UTF-16 `CString`.
fn cstring(text: &str) -> Vec<u8> {
    let mut bytes = vec![0xff, 0xfe, 0xff, u8::try_from(text.len()).unwrap()];
    for unit in text.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    bytes
}

/// A `Config-0-ResolvedFeatures` payload for [`config_object_stream_declaring`]
/// with `moPMarkRecord_c`: the map count, a u16, and a `moBaseBody_c` named
/// `feature` (its `moNodeName_c` header at class index 4) that holds one
/// record in its flag-1 form naming `mark`.
pub(crate) fn held_mark_record_stream(mark: u32, feature: &str) -> Vec<u8> {
    let mut stream = CONFIGURATION_MAP_COUNT.to_le_bytes().to_vec();
    stream.extend_from_slice(&[0, 0]);
    stream.extend(declaration("moBaseBody_c"));
    stream.extend_from_slice(&tag(4));
    stream.extend(cstring(feature));
    stream.extend_from_slice(&[0; 8]);
    stream.extend_from_slice(&42_u32.to_le_bytes());
    stream.extend_from_slice(&[0; 4]);
    stream.extend(cstring(""));
    stream.extend_from_slice(&[0; 8 + 12 + 35]);
    stream.extend(cstring(""));
    stream.extend_from_slice(&[0x00, 0xff, 0xff, 0x00, 0x00]);
    stream.extend_from_slice(&[0; 49]);
    stream.extend_from_slice(&[0x02, 0x00]);
    stream.extend_from_slice(&[0; 8 + 4]);
    stream.extend_from_slice(&[0; 32]);
    stream.extend_from_slice(&[0x01, 0x00]);
    stream.extend_from_slice(&tag(7));
    for word in [mark, 1, 0] {
        stream.extend_from_slice(&word.to_le_bytes());
    }
    stream.extend_from_slice(&[0; 8]);
    for word in [20_u32, 0, 20] {
        stream.extend_from_slice(&word.to_le_bytes());
    }
    stream.extend_from_slice(&[0; 16]);
    stream
}

/// The `Config-0-ResolvedFeatures` payload: the map count, a u16, and one
/// `moLengthParameter_c` object whose value is [`PARAMETER_VALUE`].
pub(crate) fn resolved_features_stream() -> Vec<u8> {
    let mut stream = CONFIGURATION_MAP_COUNT.to_le_bytes().to_vec();
    stream.extend_from_slice(&[0, 0]);
    stream.extend_from_slice(&tag(7));
    stream.extend(real_parameter_body(PARAMETER_VALUE.to_le_bytes()));
    stream
}

/// The same payload with an unknown inline class after the parameter object,
/// which stops the walk.
pub(crate) fn stopped_resolved_features_stream() -> Vec<u8> {
    let mut stream = resolved_features_stream();
    stream.extend_from_slice(&[0xff, 0xff, 0, 0]);
    stream.extend_from_slice(&3_u16.to_le_bytes());
    stream.extend_from_slice(b"xyz");
    stream
}

fn sldprt_with_resolved_features(resolved: &[u8]) -> Vec<u8> {
    let mut file = outer_header();
    file.extend_from_slice(&make_block(
        0x20,
        "Contents/Config-0",
        &config_object_stream(),
    ));
    file.extend_from_slice(&make_block(
        0x21,
        "Contents/Config-0-ResolvedFeatures",
        resolved,
    ));
    file
}

/// A part whose `Config-0` stream also declares a sketch and a sketch
/// entity class, so it holds a config-objects lane beside the
/// `Config-0-ResolvedFeatures` lane.
pub(crate) fn sldprt_with_sketch_config_objects() -> Vec<u8> {
    let mut config = config_object_stream_declaring("sgSketch");
    config.extend(declaration("sgPointHandle"));
    let mut file = outer_header();
    file.extend_from_slice(&make_block(0x20, "Contents/Config-0", &config));
    file.extend_from_slice(&make_block(
        0x21,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_features_stream(),
    ));
    file
}

/// A part whose `Config-0` class map types its lane scalar's archive class.
pub(crate) fn sldprt_with_archive_parameter() -> Vec<u8> {
    sldprt_with_resolved_features(&resolved_features_stream())
}

/// A part whose archive walk stops at an unknown inline class.
pub(crate) fn sldprt_with_stopped_archive_walk() -> Vec<u8> {
    sldprt_with_resolved_features(&stopped_resolved_features_stream())
}
