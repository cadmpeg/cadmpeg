// SPDX-License-Identifier: Apache-2.0
//! Decode and transfer tests for text and binary ASM streams.

use cadmpeg_test_support::wire;

use cadmpeg_asm::dialect::DECLARED_SAVE_FORMAT_MAJOR;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{Codec, DecodeResult};
use cadmpeg_ir::geometry::SolvedSurfaceGeometry;
use std::io::Cursor;

use crate::loss::SatLossCode;
use crate::test_support::test_streams::{
    acis_text_sphere_stream, binary_sphere_stream, text_sphere_stream, BinaryFixtureKind,
    UNVERIFIED_SAVE_FORMAT,
};
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
    let Some(SolvedSurfaceGeometry::Sphere(sphere_surface)) = surface.geometry.solved() else {
        panic!("sphere carrier expected, got {:?}", surface.geometry);
    };

    sphere_surface.radius().get()
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
        assert!(result.report().geometry_transferred());
    }
    // 25 stream units at scale 1 (mm) and 2.5 binary centimetres are both
    // 25 mm in the model.
    assert!((sphere_radius(&text) - 25.0).abs() < 1.0e-9);
    assert!((sphere_radius(&asm_binary) - 25.0).abs() < 1.0e-9);
    assert!((sphere_radius(&acis_binary) - 25.0).abs() < 1.0e-9);
    let text_resabs_mm = 0.000_001;
    let binary_resabs_mm = 0.000_01;
    assert!((text.ir().tolerances.linear.get() - text_resabs_mm).abs() < f64::EPSILON);
    for result in [&asm_binary, &acis_binary] {
        assert!((result.ir().tolerances.linear.get() - binary_resabs_mm).abs() < f64::EPSILON);
    }
}

#[test]
fn text_scale_selects_the_length_unit() {
    let inch = decode_bytes(&text_sphere_stream(25.4));
    assert!((sphere_radius(&inch) - 635.0).abs() < 1.0e-9);
    let expected_resabs_mm = 0.000_025_4;
    assert!((inch.ir().tolerances.linear.get() - expected_resabs_mm).abs() < f64::EPSILON);
}

#[test]
fn unrepresentable_text_length_is_not_implemented() {
    let source = String::from_utf8(text_sphere_stream(1.0))
        .expect("ASCII sphere stream")
        .replacen(
            "1 9.999999999999999547e-07 1.000000000000000036e-10",
            "5e-324 0 0",
            1,
        )
        .replacen("0 0 0 25 1 0 0", "0 0 0 1 1 0 0", 1);
    let error = SatCodec
        .decode(
            &mut Cursor::new(source.into_bytes()),
            &cadmpeg_ir::codec::DecodeOptions::default(),
        )
        .expect_err("nonzero radius cannot collapse to zero");
    assert!(matches!(
        error,
        cadmpeg_ir::DecodeFailure::Codec(CodecError::NotImplemented(_))
    ));
}

#[test]
fn text_decode_preflights_header_entities_and_counts_actual_records() {
    use cadmpeg_core::decode::ResourceDimension;

    let bytes = text_sphere_stream(1.0);
    let mut options = cadmpeg_ir::codec::DecodeOptions::default();
    options.policy.limits.max_entities = 1;
    let error = SatCodec
        .decode(&mut Cursor::new(&bytes), &options)
        .expect_err("header entity count exceeds one");
    assert!(matches!(
        error,
        cadmpeg_ir::DecodeFailure::Codec(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::Entities
                && limit.operation == "preflight SAT header entities"
    ));

    options.policy.limits.max_entities = 5;
    let error = SatCodec
        .decode(&mut Cursor::new(&bytes), &options)
        .expect_err("six native records exceed five");
    assert!(matches!(
        error,
        cadmpeg_ir::DecodeFailure::Codec(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::Entities
                && limit.operation == "admit SAT native records"
    ));
    assert_eq!(decode_bytes(&bytes).ir().model.bodies.len(), 1);
}

#[test]
fn known_sphere_record_retains_source_offset_tag_and_derived_fields() {
    let bytes = text_sphere_stream(1.0);
    let result = decode_bytes(&bytes);
    let surface_id = result.ir().model.surfaces[0].id.to_string();
    let (_, _, fidelity) = result.into_parts();
    let provenance = fidelity
        .annotations
        .provenance
        .get(surface_id.as_str())
        .expect("sphere source record");
    let expected_offset = bytes
        .windows(b"sphere-surface".len())
        .position(|window| window == b"sphere-surface")
        .expect("sphere record in source");
    assert_eq!(provenance.stream(), "sat:stream");
    assert_eq!(provenance.offset, expected_offset as u64);
    assert_eq!(provenance.tag.as_deref(), Some("sphere-surface"));
    let fields = fidelity.annotations.exactness()[surface_id.as_str()].fields();
    assert_eq!(
        fields.get("geometry.axis"),
        Some(&cadmpeg_ir::Exactness::Derived)
    );
    assert_eq!(
        fields.get("geometry.ref_direction"),
        Some(&cadmpeg_ir::Exactness::Derived)
    );
}

#[test]
fn ids_use_the_sat_format_scheme() {
    let result = decode_bytes(&text_sphere_stream(1.0));
    let body_id = &result.ir().model.bodies[0].id;
    assert!(
        body_id.as_str().starts_with("sat:brep:entity#"),
        "unexpected id scheme: {body_id:?}"
    );
}

#[test]
fn native_arenas_live_under_the_sat_namespace() {
    let result = decode_bytes(&text_sphere_stream(1.0));
    let namespace = result.ir().native.namespace(FORMAT).expect("sat namespace");
    assert!(namespace.arenas().contains_key("face_sidedness"));
    assert_eq!(namespace.arenas()["face_sidedness"].len(), 1);
}

#[test]
fn an_unverified_acis_binary_band_is_decoded_and_marked() {
    // A band no row verifies takes the same framing and record decode as a
    // verified one; only the mark on the result differs.
    let result = decode_bytes(&binary_sphere_stream(BinaryFixtureKind::AcisUnverifiedBand));
    assert!(result.report().geometry_transferred());
    assert_eq!(result.ir().model.bodies.len(), 1);
    assert!((sphere_radius(&result) - 25.0).abs() < 1.0e-9);
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == SatLossCode::SourceDialectUnverified.kind()));
    let source = result.ir().source.as_ref().expect("source metadata");
    assert_eq!(source.attributes["kernel_family"], "acis");
    assert_eq!(
        source.attributes["acis_save_format_version"],
        UNVERIFIED_SAVE_FORMAT.to_string()
    );
}

#[test]
fn an_unverified_acis_text_band_is_decoded_and_marked() {
    let result = decode_bytes(&acis_text_sphere_stream(UNVERIFIED_SAVE_FORMAT));
    assert!(result.report().geometry_transferred());
    assert_eq!(result.ir().model.bodies.len(), 1);
    assert!((sphere_radius(&result) - 25.0).abs() < 1.0e-9);
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == SatLossCode::SourceDialectUnverified.kind()));
    let source = result.ir().source.as_ref().expect("source metadata");
    assert_eq!(source.attributes["kernel_family"], "acis");
    assert_eq!(source.dialect().unwrap().declared()["encoding"], "text");
    assert!(!source.attributes.contains_key("encoding"));
    assert!(!source.attributes.contains_key("terminator"));
}

#[test]
fn an_unverified_band_that_decodes_nothing_reports_honest_coverage() {
    // Recovery is not a promise of content: an unverified band whose records
    // this codec does not type keeps both marks and reports what it did not
    // read.
    let mut text = String::new();
    text.push_str("70000 0 1 0 \n");
    text.push_str("16 Autodesk Neutron 21 ASM 232.4.0.65535 OSX 9 Synthetic \n");
    text.push_str("1 1e-06 1.0e-10 \n");
    text.push_str("mystery_record $-1 -1 42 #\n");
    text.push_str("End-of-ACIS-data \n");
    let result = decode_bytes(text.as_bytes());
    assert!(!result.report().geometry_transferred());
    assert!(wire::coverage(result.report()).contains_key("unknown_records"));
    let codes = result
        .report()
        .losses
        .iter()
        .map(|loss| loss.code.clone())
        .collect::<Vec<_>>();
    assert!(codes.contains(&SatLossCode::SourceDialectUnverified.kind()));
    assert!(codes.contains(&SatLossCode::GeometryFramedWithoutCarriers.kind()));
}

#[test]
fn unframed_binary_header_has_the_same_refused_match_at_inspect_and_decode() {
    let mut bytes = b"ACIS BinaryFile".to_vec();
    bytes.extend_from_slice(&UNVERIFIED_SAVE_FORMAT.to_le_bytes());
    bytes.extend_from_slice(&[0u8; 4]);
    let summary = SatCodec
        .inspect(
            &mut Cursor::new(&bytes),
            &cadmpeg_core::decode::InspectOptions::default(),
        )
        .expect("the recognized stream kind inspects at refusal depth");
    let layers = summary
        .dialects()
        .expect("inspection classifies the host and kernel layers");
    let inspected = layers.primary();
    assert_eq!(inspected.dialect().as_str(), "sat:acis-binary");
    assert_eq!(
        inspected.admission(),
        &cadmpeg_core::dialect::Admission::Refused
    );
    assert_eq!(layers.iter().count(), 2, "inspect retains both layers");
    assert_eq!(inspected.declared()["encoding"], "binary");
    assert!(!inspected
        .declared()
        .contains_key(DECLARED_SAVE_FORMAT_MAJOR));
    let kernel = layers
        .iter()
        .nth(1)
        .expect("inspect retains the kernel layer");
    assert_eq!(kernel.dialect().as_str(), "acis:save-format-binary-other");
    assert_eq!(
        kernel.declared()[DECLARED_SAVE_FORMAT_MAJOR],
        (UNVERIFIED_SAVE_FORMAT / 100).to_string()
    );

    let error = SatCodec
        .decode(
            &mut Cursor::new(bytes),
            &cadmpeg_ir::codec::DecodeOptions::default(),
        )
        .unwrap_err();
    let cadmpeg_ir::DecodeFailure::Codec(CodecError::UnsupportedDialect {
        dialects: refused_layers,
        ..
    }) = error
    else {
        panic!("expected identified dialect refusal, got {error:?}");
    };
    assert_eq!(refused_layers.as_ref(), layers);
}

#[test]
fn unframed_discriminant_has_the_same_refused_match_at_inspect_and_decode() {
    let bytes = b"21800 0 1 0 \n1";
    let summary = SatCodec
        .inspect(
            &mut Cursor::new(bytes),
            &cadmpeg_core::decode::InspectOptions::default(),
        )
        .expect("the recognized stream kind inspects at refusal depth");
    let layers = summary
        .dialects()
        .expect("inspection classifies the host and kernel layers");
    let inspected = layers.primary();
    assert_eq!(inspected.dialect().as_str(), "sat:text");
    assert_eq!(
        inspected.admission(),
        &cadmpeg_core::dialect::Admission::Refused
    );
    assert_eq!(
        layers.iter().count(),
        2,
        "the full inspect layer list is retained"
    );

    let error = SatCodec
        .decode(
            &mut Cursor::new(bytes),
            &cadmpeg_ir::codec::DecodeOptions::default(),
        )
        .expect_err("the unframed identified stream is refused");
    let cadmpeg_ir::DecodeFailure::Codec(CodecError::UnsupportedDialect {
        dialects: refused_layers,
        ..
    }) = error
    else {
        panic!("expected identified dialect refusal, got {error:?}");
    };
    assert_eq!(refused_layers.as_ref(), layers);
}

#[test]
fn a_geometry_less_text_stream_reports_uncovered_coverage() {
    let mut text = String::new();
    text.push_str("21800 0 1 0 \n");
    text.push_str("16 Autodesk Neutron 21 ASM 232.4.0.65535 OSX 9 Synthetic \n");
    text.push_str("1 1e-06 1.0e-10 \n");
    text.push_str("mystery_record $-1 -1 42 #\n");
    text.push_str("End-of-ACIS-data \n");
    let result = decode_bytes(text.as_bytes());
    assert!(!result.report().geometry_transferred());
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
    assert!(matches!(
        error,
        cadmpeg_ir::DecodeFailure::Codec(CodecError::WrongFormat(_))
    ));
}

#[test]
fn zero_header_resabs_preserves_default_and_records_loss() {
    let source = String::from_utf8(text_sphere_stream(1.0)).unwrap();
    let source = source.replacen("9.999999999999999547e-07", "0", 1);
    let result = decode_bytes(source.as_bytes());
    assert_eq!(result.ir().model.bodies.len(), 1);
    assert_eq!(
        result.ir().tolerances.linear,
        cadmpeg_ir::units::Tolerances::default().linear
    );
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| { loss.code.to_string() == "sat/header.tolerance-unresolved" }));
}
