// SPDX-License-Identifier: Apache-2.0
//! Text and binary sphere-stream fixtures.

/// Save-format word of a band no Spatial ACIS row verifies: major 700.
pub(crate) const UNVERIFIED_SAVE_FORMAT: u32 = 70_000;

/// A text stream carrying one loopless closed sphere face at header scale
/// `scale` millimetres per unit.
pub(crate) fn text_sphere_stream(scale: f64) -> Vec<u8> {
    text_sphere_stream_with(23_200, scale, "End-of-ASM-data")
}

/// The same solid under the `End-of-ACIS-data` terminator at
/// `save_format_version`, so a text stream outside the verified Spatial ACIS
/// band carries real geometry to recover.
pub(crate) fn acis_text_sphere_stream(save_format_version: u32) -> Vec<u8> {
    text_sphere_stream_with(save_format_version, 1.0, "End-of-ACIS-data")
}

/// The sphere text stream at one save format, scale, and terminator.
fn text_sphere_stream_with(save_format_version: u32, scale: f64, terminator: &str) -> Vec<u8> {
    let mut text = String::new();
    text.push_str(&save_format_version.to_string());
    text.push_str(" 0 2 2 \n");
    text.push_str("16 Autodesk Neutron 21 ASM 232.4.0.65535 OSX 9 Synthetic \n");
    text.push_str(&scale.to_string());
    text.push_str(" 9.999999999999999547e-07 1.000000000000000036e-10 \n");
    text.push_str("asmheader $-1 -1 @13 232.4.0.65535 #\n");
    text.push_str("body $-1 -1 $-1 $2 $-1 $-1 #\n");
    text.push_str("lump $-1 -1 $-1 $-1 $3 $1 #\n");
    text.push_str("shell $-1 -1 $-1 $-1 $-1 $4 $-1 $2 #\n");
    text.push_str("face $-1 -1 $-1 $-1 $-1 $3 $-1 $5 forward single #\n");
    text.push_str("sphere-surface $-1 -1 $-1 0 0 0 25 1 0 0 0 0 1 forward_v I I I I #\n");
    text.push_str(terminator);
    text.push('\n');
    text.into_bytes()
}

/// The same solid in the binary encoding, built token by token. The binary
/// unit is centimetres, so the same 25 mm radius is stored as 2.5.
#[derive(Clone, Copy)]
pub(crate) enum BinaryFixtureKind {
    Asm,
    Acis,
    /// `ACIS BinaryFile` declaring [`UNVERIFIED_SAVE_FORMAT`]: the same records
    /// under a band no row verifies.
    AcisUnverifiedBand,
}

pub(crate) fn binary_sphere_stream(kind: BinaryFixtureKind) -> Vec<u8> {
    let mut bytes = Vec::new();
    let width = match kind {
        BinaryFixtureKind::Asm => {
            bytes.extend_from_slice(b"ASM BinaryFile8");
            bytes.extend_from_slice(&23200u32.to_le_bytes());
            bytes.extend_from_slice(&[0u8; 12]);
            bytes.extend_from_slice(&2u64.to_le_bytes()); // entity count
            bytes.extend_from_slice(&2u64.to_le_bytes()); // revision 1, no history
            8
        }
        BinaryFixtureKind::Acis | BinaryFixtureKind::AcisUnverifiedBand => {
            bytes.extend_from_slice(b"ACIS BinaryFile");
            let save_format = match kind {
                BinaryFixtureKind::AcisUnverifiedBand => UNVERIFIED_SAVE_FORMAT,
                _ => 21_800,
            };
            for value in [save_format, 0, 2, 2] {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
            4
        }
    };
    for text in ["Autodesk Neutron", "ASM 232.4.0.65535 OSX", "Synthetic"] {
        bytes.push(0x07);
        bytes.push(u8::try_from(text.len()).unwrap());
        bytes.extend_from_slice(text.as_bytes());
    }
    for value in [10.0f64, 1.0e-6, 1.0e-10] {
        bytes.push(0x06);
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    let ident = |bytes: &mut Vec<u8>, tag: u8, name: &str| {
        bytes.push(tag);
        bytes.push(u8::try_from(name.len()).unwrap());
        bytes.extend_from_slice(name.as_bytes());
    };
    let reference = |bytes: &mut Vec<u8>, value: i64| {
        bytes.push(0x0c);
        if width == 8 {
            bytes.extend_from_slice(&value.to_le_bytes());
        } else {
            bytes.extend_from_slice(&i32::try_from(value).unwrap().to_le_bytes());
        }
    };
    let long = |bytes: &mut Vec<u8>, value: i64| {
        bytes.push(0x04);
        if width == 8 {
            bytes.extend_from_slice(&value.to_le_bytes());
        } else {
            bytes.extend_from_slice(&i32::try_from(value).unwrap().to_le_bytes());
        }
    };
    let double = |bytes: &mut Vec<u8>, value: f64| {
        bytes.push(0x06);
        bytes.extend_from_slice(&value.to_le_bytes());
    };
    // asmheader (index 0)
    ident(&mut bytes, 0x0d, "asmheader");
    reference(&mut bytes, -1);
    long(&mut bytes, -1);
    bytes.extend_from_slice(&[0x07, 13]);
    bytes.extend_from_slice(b"232.4.0.65535");
    bytes.push(0x11);
    // body (1) -> lump 2
    ident(&mut bytes, 0x0d, "body");
    reference(&mut bytes, -1);
    long(&mut bytes, -1);
    for value in [-1i64, 2, -1, -1] {
        reference(&mut bytes, value);
    }
    bytes.push(0x11);
    // lump (2) -> shell 3, owner 1
    ident(&mut bytes, 0x0d, "lump");
    reference(&mut bytes, -1);
    long(&mut bytes, -1);
    for value in [-1i64, -1, 3, 1] {
        reference(&mut bytes, value);
    }
    bytes.push(0x11);
    // shell (3) -> face 4, owner 2
    ident(&mut bytes, 0x0d, "shell");
    reference(&mut bytes, -1);
    long(&mut bytes, -1);
    for value in [-1i64, -1, -1, 4, -1, 2] {
        reference(&mut bytes, value);
    }
    bytes.push(0x11);
    // face (4) -> shell 3, surface 5, loopless
    ident(&mut bytes, 0x0d, "face");
    reference(&mut bytes, -1);
    long(&mut bytes, -1);
    for value in [-1i64, -1, -1, 3, -1, 5] {
        reference(&mut bytes, value);
    }
    bytes.extend_from_slice(&[0x0b, 0x0b, 0x11]);
    // sphere-surface (5): center, radius 2.5 cm, two axes, uv sense, bounds
    ident(&mut bytes, 0x0e, "sphere");
    ident(&mut bytes, 0x0d, "surface");
    reference(&mut bytes, -1);
    long(&mut bytes, -1);
    reference(&mut bytes, -1);
    bytes.push(0x13);
    for value in [0.0f64, 0.0, 0.0] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    double(&mut bytes, 2.5);
    for triple in [[1.0f64, 0.0, 0.0], [0.0, 0.0, 1.0]] {
        bytes.push(0x14);
        for value in triple {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    bytes.extend_from_slice(&[0x0b; 5]);
    bytes.push(0x11);
    bytes
}
