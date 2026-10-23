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

/// The 35-byte trailer of a driving scalar with no operands.
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

/// `Contents/Config-0`: `moPart_c` and `moNodeName_c` take indices 2 and 4,
/// one name object takes 6, and `moLengthParameter_c` takes 7, so the map ends
/// at 9.
pub(crate) fn config_object_stream() -> Vec<u8> {
    let mut stream = declaration("moPart_c");
    stream.extend_from_slice(&[0, 0]);
    stream.extend(declaration("moNodeName_c"));
    stream.extend_from_slice(&[0xff, 0xfe, 0xff, 0x01, b'A', 0]);
    stream.extend_from_slice(&tag(4));
    stream.extend_from_slice(&[0xff, 0xfe, 0xff, 0x01, b'B', 0]);
    stream.extend(declaration("moLengthParameter_c"));
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

/// A part whose `Config-0` class map types its lane scalar's archive class.
pub(crate) fn sldprt_with_archive_parameter() -> Vec<u8> {
    sldprt_with_resolved_features(&resolved_features_stream())
}

/// A part whose archive walk stops at an unknown inline class.
pub(crate) fn sldprt_with_stopped_archive_walk() -> Vec<u8> {
    sldprt_with_resolved_features(&stopped_resolved_features_stream())
}
