// SPDX-License-Identifier: Apache-2.0
//! Synthetic-stream tests for both encodings and the detection rules.

use super::*;
use cadmpeg_ir::codec::CodecEntry;
use cadmpeg_ir::geometry::SurfaceGeometry;
use std::io::Cursor;

fn decode_bytes(bytes: &[u8]) -> DecodeResult {
    SatCodec
        .decode(
            &mut Cursor::new(bytes.to_vec()),
            &cadmpeg_ir::codec::DecodeOptions::default(),
        )
        .unwrap()
}

/// A text stream carrying one loopless closed sphere face at header scale
/// `scale` millimetres per unit.
fn text_sphere_stream(scale: f64) -> Vec<u8> {
    let mut text = String::new();
    text.push_str("23200 0 2 2 \n");
    text.push_str("16 Autodesk Neutron 21 ASM 232.4.0.65535 OSX 9 Synthetic \n");
    text.push_str(&scale.to_string());
    text.push_str(" 9.999999999999999547e-07 1.000000000000000036e-10 \n");
    text.push_str("asmheader $-1 -1 @13 232.4.0.65535 #\n");
    text.push_str("body $-1 -1 $-1 $2 $-1 $-1 #\n");
    text.push_str("lump $-1 -1 $-1 $-1 $3 $1 #\n");
    text.push_str("shell $-1 -1 $-1 $-1 $-1 $4 $-1 $2 #\n");
    text.push_str("face $-1 -1 $-1 $-1 $-1 $3 $-1 $5 forward single #\n");
    text.push_str("sphere-surface $-1 -1 $-1 0 0 0 25 1 0 0 0 0 1 forward_v I I I I #\n");
    text.push_str("End-of-ASM-data\n");
    text.into_bytes()
}

/// The same solid in the binary encoding, built token by token. The binary
/// unit is centimetres, so the same 25 mm radius is stored as 2.5.
fn binary_sphere_stream() -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"ASM BinaryFile8");
    bytes.extend_from_slice(&23200u32.to_le_bytes());
    bytes.extend_from_slice(&[0u8; 12]);
    bytes.extend_from_slice(&2u64.to_le_bytes()); // entity count
    bytes.extend_from_slice(&2u64.to_le_bytes()); // flags: revision 1, no history
    for text in ["Autodesk Neutron", "ASM 232.4.0.65535 OSX", "Synthetic"] {
        bytes.push(0x07);
        bytes.push(u8::try_from(text.len()).unwrap());
        bytes.extend_from_slice(text.as_bytes());
    }
    for value in [10.0f64, 1e-6, 1e-10] {
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
        bytes.extend_from_slice(&value.to_le_bytes());
    };
    let long = |bytes: &mut Vec<u8>, value: i64| {
        bytes.push(0x04);
        bytes.extend_from_slice(&value.to_le_bytes());
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

fn sphere_radius(result: &DecodeResult) -> f64 {
    let surface = &result.ir.model.surfaces[0];
    let SurfaceGeometry::Sphere { radius, .. } = &surface.geometry else {
        panic!("sphere carrier expected, got {:?}", surface.geometry);
    };
    *radius
}

#[test]
fn detection_is_content_based() {
    assert_eq!(SatCodec.detect(b"ASM BinaryFile8\x00"), Confidence::High);
    assert_eq!(SatCodec.detect(b"ACIS BinaryFile\x00"), Confidence::High);
    assert_eq!(
        SatCodec.detect(b"23200 0 2 2 \n16 Autodesk Neutron"),
        Confidence::Medium
    );
    assert_eq!(
        SatCodec.detect(b"700 0 6 0           \n30 Autodesk"),
        Confidence::Medium
    );
    // Numeric text without the four-word first line is not a stream.
    assert_eq!(SatCodec.detect(b"123 456\n789"), Confidence::No);
    assert_eq!(SatCodec.detect(b"ISO-10303-21;\nHEADER;"), Confidence::No);
    assert_eq!(SatCodec.detect(b"{\"ir_version\":\"5\"}"), Confidence::No);
}

#[test]
fn both_encodings_decode_the_same_solid() {
    let text = decode_bytes(&text_sphere_stream(1.0));
    let binary = decode_bytes(&binary_sphere_stream());
    for result in [&text, &binary] {
        assert_eq!(result.ir.model.bodies.len(), 1);
        assert_eq!(result.ir.model.shells.len(), 1);
        assert_eq!(result.ir.model.faces.len(), 1);
        assert_eq!(result.ir.model.surfaces.len(), 1);
        assert!(result.report.geometry_transferred);
    }
    // 25 stream units at scale 1 (mm) and 2.5 binary centimetres are both
    // 25 mm in the model.
    assert!((sphere_radius(&text) - 25.0).abs() < 1e-9);
    assert!((sphere_radius(&binary) - 25.0).abs() < 1e-9);
}

#[test]
fn text_scale_selects_the_length_unit() {
    let inch = decode_bytes(&text_sphere_stream(25.4));
    assert!((sphere_radius(&inch) - 635.0).abs() < 1e-9);
}

#[test]
fn ids_use_the_sat_format_scheme() {
    let result = decode_bytes(&text_sphere_stream(1.0));
    let body_id = &result.ir.model.bodies[0].id;
    assert!(
        body_id.0.starts_with("sat:brep:entity#"),
        "unexpected id scheme: {body_id:?}"
    );
}

#[test]
fn native_arenas_live_under_the_sat_namespace() {
    let result = decode_bytes(&text_sphere_stream(1.0));
    let namespace = result.ir.native.namespace(FORMAT).expect("sat namespace");
    assert!(namespace.arenas.contains_key("face_sidedness"));
    assert_eq!(namespace.arenas["face_sidedness"].len(), 1);
}

#[test]
fn an_acis_binary_stream_is_identified_and_reported() {
    let mut bytes = b"ACIS BinaryFile".to_vec();
    bytes.extend_from_slice(&[0u8; 32]);
    let result = decode_bytes(&bytes);
    assert!(!result.report.geometry_transferred);
    assert!(result
        .report
        .losses
        .iter()
        .any(|loss| loss.message.contains("Spatial ACIS binary stream")));
    assert!(result.ir.model.bodies.is_empty());
}

#[test]
fn a_geometry_less_text_stream_reports_uncovered_coverage() {
    let mut text = String::new();
    text.push_str("700 0 1 0 \n");
    text.push_str("16 Autodesk Neutron 21 ASM 232.4.0.65535 OSX 9 Synthetic \n");
    text.push_str("1 1e-06 1e-10 \n");
    text.push_str("mystery_record $-1 -1 42 #\n");
    text.push_str("End-of-ACIS-data \n");
    let result = decode_bytes(text.as_bytes());
    assert!(!result.report.geometry_transferred);
    let loss = result
        .report
        .losses
        .iter()
        .find(|loss| loss.code == LossKind::GeometryNotTransferred)
        .expect("coverage loss");
    assert!(loss.message.contains("End-of-ACIS-data"));
}

#[test]
fn a_non_stream_input_is_refused() {
    let error = SatCodec
        .decode(
            &mut Cursor::new(b"not a stream at all".to_vec()),
            &cadmpeg_ir::codec::DecodeOptions::default(),
        )
        .unwrap_err();
    assert!(matches!(error, CodecError::Malformed(_)));
}

#[test]
fn inspect_reports_the_stream_kind_and_header_facts() {
    let summary = SatCodec
        .inspect(
            &mut Cursor::new(text_sphere_stream(1.0)),
            &cadmpeg_codec_core::decode::InspectOptions::default(),
        )
        .unwrap();
    assert_eq!(summary.format, "sat");
    assert_eq!(summary.entries.len(), 1);
    assert_eq!(summary.entries[0].role, "brep-text");
    assert_eq!(
        summary.entries[0]
            .attributes
            .get("acis_save_format_version"),
        Some(&"23200".to_string())
    );
    assert_eq!(
        summary.entries[0].attributes.get("terminator"),
        Some(&"End-of-ASM-data".to_string())
    );
}
