// SPDX-License-Identifier: Apache-2.0
//! Decode and transfer tests for text and binary ASM streams.

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{Codec, DecodeResult};
use cadmpeg_ir::geometry::SurfaceGeometry;
use std::io::Cursor;

use crate::loss::SatLossCode;
use crate::test_support::{binary_sphere_stream, text_sphere_stream, BinaryFixtureKind};
use crate::{SatCodec, FORMAT};

fn decode_bytes(bytes: &[u8]) -> DecodeResult {
    SatCodec
        .decode(
            &mut Cursor::new(bytes.to_vec()),
            &cadmpeg_ir::codec::DecodeOptions::default(),
        )
        .unwrap()
}

fn sphere_radius(result: &DecodeResult) -> f64 {
    let surface = &result.ir().model.surfaces[0];
    let SurfaceGeometry::Sphere { radius, .. } = &surface.geometry else {
        panic!("sphere carrier expected, got {:?}", surface.geometry);
    };
    *radius
}

#[test]
fn both_encodings_decode_the_same_solid() {
    let text = decode_bytes(&text_sphere_stream(1.0));
    let asm_binary = decode_bytes(&binary_sphere_stream(BinaryFixtureKind::Asm));
    let acis_binary = decode_bytes(&binary_sphere_stream(BinaryFixtureKind::Acis));
    for result in [&text, &asm_binary, &acis_binary] {
        assert_eq!(result.ir().model.bodies.len(), 1);
        assert_eq!(result.ir().model.shells.len(), 1);
        assert_eq!(result.ir().model.faces.len(), 1);
        assert_eq!(result.ir().model.surfaces.len(), 1);
        assert!(result.report().geometry_transferred);
    }
    // 25 stream units at scale 1 (mm) and 2.5 binary centimetres are both
    // 25 mm in the model.
    assert!((sphere_radius(&text) - 25.0).abs() < 1.0e-9);
    assert!((sphere_radius(&asm_binary) - 25.0).abs() < 1.0e-9);
    assert!((sphere_radius(&acis_binary) - 25.0).abs() < 1.0e-9);
}

#[test]
fn text_scale_selects_the_length_unit() {
    let inch = decode_bytes(&text_sphere_stream(25.4));
    assert!((sphere_radius(&inch) - 635.0).abs() < 1.0e-9);
}

#[test]
fn ids_use_the_sat_format_scheme() {
    let result = decode_bytes(&text_sphere_stream(1.0));
    let body_id = &result.ir().model.bodies[0].id;
    assert!(
        body_id.0.starts_with("sat:brep:entity#"),
        "unexpected id scheme: {body_id:?}"
    );
}

#[test]
fn native_arenas_live_under_the_sat_namespace() {
    let result = decode_bytes(&text_sphere_stream(1.0));
    let namespace = result.ir().native.namespace(FORMAT).expect("sat namespace");
    assert!(namespace.arenas.contains_key("face_sidedness"));
    assert_eq!(namespace.arenas["face_sidedness"].len(), 1);
}

#[test]
fn an_unadmitted_acis_binary_band_is_identified_and_reported() {
    let mut bytes = b"ACIS BinaryFile".to_vec();
    bytes.extend_from_slice(&100u32.to_le_bytes());
    bytes.extend_from_slice(&[0u8; 28]);
    let result = decode_bytes(&bytes);
    assert!(!result.report().geometry_transferred);
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("Spatial ACIS binary stream")));
    let source = result.ir().source.as_ref().expect("source metadata");
    assert_eq!(source.attributes["kernel_family"], "acis");
    assert_eq!(source.attributes["acis_save_format_version"], "100");
    assert!(result.ir().model.bodies.is_empty());
}

#[test]
fn a_geometry_less_text_stream_reports_uncovered_coverage() {
    let mut text = String::new();
    text.push_str("700 0 1 0 \n");
    text.push_str("16 Autodesk Neutron 21 ASM 232.4.0.65535 OSX 9 Synthetic \n");
    text.push_str("1 1e-06 1.0e-10 \n");
    text.push_str("mystery_record $-1 -1 42 #\n");
    text.push_str("End-of-ACIS-data \n");
    let result = decode_bytes(text.as_bytes());
    assert!(!result.report().geometry_transferred);
    let loss = result
        .report()
        .losses
        .iter()
        .find(|loss| loss.code == SatLossCode::GeometryFramedWithoutCarriers.kind())
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
