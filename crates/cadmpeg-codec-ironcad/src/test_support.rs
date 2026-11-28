// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::io::Write as _;

use flate2::write::ZlibEncoder;
use flate2::Compression;

/// Inflated block size of a frame.
pub(crate) const BLOCK_SIZE: u32 = 0x40000;

/// A framed stream whose zlib stream inflates to `block` bytes.
pub(crate) fn framed_with(payload: &[u8], block: u32, declared: u64) -> Vec<u8> {
    let mut inflated = payload.to_vec();
    inflated.resize(usize::try_from(block).unwrap(), 0);
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(&inflated).unwrap();
    let compressed = encoder.finish().unwrap();
    let mut stream = 0x3de3_de00_u32.to_le_bytes().to_vec();
    stream.extend_from_slice(&block.to_le_bytes());
    stream.extend_from_slice(&declared.to_le_bytes());
    stream.extend_from_slice(&u32::try_from(compressed.len()).unwrap().to_le_bytes());
    stream.extend_from_slice(&compressed);
    stream
}

/// A canonical framed stream holding `payload`.
pub(crate) fn framed(payload: &[u8]) -> Vec<u8> {
    framed_with(payload, BLOCK_SIZE, u64::try_from(payload.len()).unwrap())
}

/// A tree node holding `items` in order.
pub(crate) fn node(items: &[Vec<u8>]) -> Vec<u8> {
    let content: Vec<u8> = items.concat();
    let mut bytes = u64::try_from(content.len() + 8)
        .unwrap()
        .to_le_bytes()
        .to_vec();
    bytes.extend_from_slice(&content);
    bytes
}

/// A kernel record with the given kernel tag, header words, and kernel bytes.
pub(crate) fn record_with(kernel: u32, words: [u32; 4], body: &[u8]) -> Vec<u8> {
    let mut bytes = u32::try_from(24 + body.len())
        .unwrap()
        .to_le_bytes()
        .to_vec();
    bytes.extend_from_slice(&0x0009_b401_u32.to_le_bytes());
    bytes.extend_from_slice(&kernel.to_le_bytes());
    for word in words {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    bytes.extend_from_slice(body);
    bytes
}

/// A kernel record with canonical header words.
pub(crate) fn record(kernel: u32, body: &[u8]) -> Vec<u8> {
    record_with(kernel, [1, 1, 1, 0], body)
}

/// The prologue of a little-endian Parasolid transmit.
pub(crate) fn parasolid_prologue(description: &str, schema: &str) -> Vec<u8> {
    let mut bytes = vec![0x41];
    bytes.extend_from_slice(&u16::try_from(description.len()).unwrap().to_le_bytes());
    bytes.extend_from_slice(description.as_bytes());
    bytes.extend_from_slice(&u32::try_from(schema.len()).unwrap().to_le_bytes());
    bytes.extend_from_slice(schema.as_bytes());
    bytes
}

/// Bytes of a committed `IronCAD` specimen.
pub(crate) fn corpus(name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/ironcad")
        .join(name);
    std::fs::read(path).unwrap()
}

/// One item of an interned ACIS binary stream.
#[derive(Clone, Copy)]
pub(crate) enum Sab {
    /// A record name token that defines the next name id.
    Name(&'static str),
    /// A sub-identifier name token that defines the next name id.
    SubName(&'static str),
    Ref(i32),
    Long(i32),
    Position([f64; 3]),
    Vector([f64; 3]),
    True,
    False,
    End,
}

/// An ACIS 32.0 binary save stream with interned names.
pub(crate) fn acis_stream(items: &[Sab]) -> Vec<u8> {
    let mut bytes = b"ACIS BinaryFile".to_vec();
    for word in [3200_u32, 0, 1, 0] {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    for text in ["IronCAD - 32.0", "ACIS 32.0 NT", "date"] {
        bytes.push(0x07);
        bytes.push(u8::try_from(text.len()).unwrap());
        bytes.extend_from_slice(text.as_bytes());
    }
    for value in [10.0_f64, 1.0e-6, 0.0] {
        bytes.push(0x06);
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&[0x0a, 0x07, 3]);
    bytes.extend_from_slice(b"key");
    let mut defined = 0_u32;
    let name = |bytes: &mut Vec<u8>, tag: u8, text: &str, id: u32| {
        bytes.push(tag);
        bytes.push(u8::try_from(text.len() + 5).unwrap());
        bytes.extend_from_slice(text.as_bytes());
        bytes.push(b'%');
        bytes.extend_from_slice(&id.to_le_bytes());
    };
    for item in items {
        match item {
            Sab::Name(text) | Sab::SubName(text) => {
                defined += 1;
                let tag = if matches!(item, Sab::Name(_)) {
                    0x0d
                } else {
                    0x0e
                };
                name(&mut bytes, tag, text, defined);
            }
            Sab::Ref(value) => {
                bytes.push(0x0c);
                bytes.extend_from_slice(&value.to_le_bytes());
            }
            Sab::Long(value) => {
                bytes.push(0x04);
                bytes.extend_from_slice(&value.to_le_bytes());
            }
            Sab::Position(value) | Sab::Vector(value) => {
                bytes.push(if matches!(item, Sab::Position(_)) {
                    0x13
                } else {
                    0x14
                });
                for component in value {
                    bytes.extend_from_slice(&component.to_le_bytes());
                }
            }
            Sab::True => bytes.push(0x0a),
            Sab::False => bytes.push(0x0b),
            Sab::End => bytes.push(0x11),
        }
    }
    bytes.extend_from_slice(&[0x0d, 16]);
    bytes.extend_from_slice(b"End-of-ACIS-data");
    bytes
}

/// A solid made of one unbounded plane face, in interned ACIS 32.0 form.
pub(crate) fn plane_face_body() -> Vec<u8> {
    face_body("plane")
}

/// A solid made of one face on a surface record of class `surface`, in
/// interned ACIS 32.0 form.
pub(crate) fn face_body(surface: &'static str) -> Vec<u8> {
    face_body_on(surface, 4)
}

/// The solid of [`face_body`], whose face names the record `surface_ref` as
/// its surface.
pub(crate) fn face_body_on(surface: &'static str, surface_ref: i32) -> Vec<u8> {
    use Sab::{End, False, Long, Name, Position, Ref, SubName, True, Vector};
    let low = Position([0.0, 0.0, 0.0]);
    let high = Position([1.0, 1.0, 0.0]);
    let prefix = [Ref(-1), Long(-1), Long(-1), Ref(-1)];
    let mut items = vec![Name("body")];
    items.extend(prefix);
    items.extend([
        Long(0),
        Ref(1),
        Ref(-1),
        Ref(-1),
        True,
        low,
        high,
        End,
        Name("lump"),
    ]);
    items.extend(prefix);
    items.extend([Ref(-1), Ref(2), Ref(0), True, low, high, End, Name("shell")]);
    items.extend(prefix);
    items.extend([
        Ref(-1),
        Ref(-1),
        Ref(3),
        Ref(-1),
        Ref(1),
        True,
        low,
        high,
        End,
        Name("face"),
    ]);
    items.extend(prefix);
    items.extend([
        Ref(-1),
        Ref(-1),
        Ref(2),
        Ref(-1),
        Ref(surface_ref),
        False,
        False,
        True,
        low,
        high,
        False,
        End,
    ]);
    items.extend([SubName(surface), Name("surface")]);
    items.extend(prefix);
    items.extend([
        Position([0.0, 0.0, 0.0]),
        Vector([0.0, 0.0, 1.0]),
        Vector([1.0, 0.0, 0.0]),
        False,
        False,
        False,
        False,
        False,
        End,
    ]);
    acis_stream(&items)
}

/// A native binary Parasolid transmit with no nodes: the prologue, the
/// maximum node type, `user_fields`, and the terminator.
pub(crate) fn parasolid_record(description: &str, user_fields: u32) -> Vec<u8> {
    let mut bytes = parasolid_prologue(description, "SCH_3401176_34101_13006");
    bytes.extend_from_slice(&231_u16.to_le_bytes());
    bytes.extend_from_slice(&user_fields.to_le_bytes());
    bytes.extend_from_slice(&[1, 0, 0, 0, 0, 0]);
    bytes
}
