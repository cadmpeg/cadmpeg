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
fn binary_product_encoding_recovery_keeps_geometry_and_exact_header() {
    for (kind, first_string) in [(BinaryFixtureKind::Asm, 47), (BinaryFixtureKind::Acis, 31)] {
        let original = binary_sphere_stream(kind);
        let expected = decode_bytes(&original);
        let mut cursor = first_string;
        let mut positions = Vec::new();
        for _ in 0..3 {
            positions.push(cursor + 2);
            cursor += 2 + usize::from(original[cursor + 1]);
        }
        let header_end = cursor + 3 * 9;
        for position in positions {
            let mut bytes = original.clone();
            bytes[position] = 0xff;
            let recovered = cadmpeg_test_support::EditableDecodeResult::from(decode_bytes(&bytes));
            assert_eq!(recovered.ir().model, expected.ir().model);
            assert_eq!(recovered.ir().tolerances, expected.ir().tolerances);
            assert!(recovered
                .report()
                .losses
                .iter()
                .any(|loss| loss.code == SatLossCode::HeaderMetadataNoncanonical.kind()));
            assert_eq!(
                recovered
                    .source_fidelity()
                    .retained_record("sat:source:header#0")
                    .unwrap()
                    .data(),
                Some(&bytes[..header_end])
            );
            assert!(cadmpeg_ir::validate_neutral(recovered.ir(), Vec::new())
                .unwrap()
                .is_ok());
        }
    }
}

#[test]
fn text_record_count_metadata_recovery_keeps_geometry_and_exact_header() {
    for original in [text_sphere_stream(1.0), acis_text_sphere_stream(21_800)] {
        let original = String::from_utf8(original).unwrap();
        let expected = decode_bytes(original.as_bytes());
        for value in [
            "-1",
            "4294967296",
            "999999999999999999999999999999999",
            "invalid",
        ] {
            let source = original.replacen(" 0 2 2", &format!(" {value} 2 2"), 1);
            let recovered =
                cadmpeg_test_support::EditableDecodeResult::from(decode_bytes(source.as_bytes()));
            assert_eq!(recovered.ir().model, expected.ir().model);
            assert!(recovered
                .report()
                .losses
                .iter()
                .any(|loss| loss.code == SatLossCode::HeaderMetadataNoncanonical.kind()));
            let header_end = source.match_indices('\n').nth(2).unwrap().0 + 1;
            assert_eq!(
                recovered
                    .source_fidelity()
                    .retained_record("sat:source:header#0")
                    .unwrap()
                    .data(),
                Some(&source.as_bytes()[..header_end])
            );
            assert!(cadmpeg_ir::validate_neutral(recovered.ir(), Vec::new())
                .unwrap()
                .is_ok());
        }
    }
}

#[test]
fn text_header_metadata_recovery_preserves_geometry_and_exact_header() {
    for original in [text_sphere_stream(1.0), acis_text_sphere_stream(23_200)] {
        let original = String::from_utf8(original).unwrap();
        let expected = decode_bytes(original.as_bytes());
        for source in [
            original.replace("9 Synthetic", ""),
            original.replace(
                "16 Autodesk Neutron 21 ASM 232.4.0.65535 OSX 9 Synthetic",
                "bad producer metadata",
            ),
            original.replace("9.999999999999999547e-07", "invalid"),
            original.replace("1.000000000000000036e-10", "NaN"),
            original.replace("1.000000000000000036e-10", ""),
        ] {
            let recovered =
                cadmpeg_test_support::EditableDecodeResult::from(decode_bytes(source.as_bytes()));
            assert_eq!(recovered.ir().model, expected.ir().model);
            assert!(!recovered.report().losses.is_empty());
            let header_end = source.match_indices('\n').nth(2).unwrap().0 + 1;
            assert_eq!(
                recovered
                    .source_fidelity()
                    .retained_record("sat:source:header#0")
                    .unwrap()
                    .data(),
                Some(&source.as_bytes()[..header_end])
            );
            assert!(cadmpeg_ir::validate_neutral(recovered.ir(), Vec::new())
                .unwrap()
                .findings
                .iter()
                .all(|finding| finding.severity < cadmpeg_ir::report::Severity::Error));
        }
    }
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
    assert_eq!(
        provenance.offset,
        cadmpeg_core::decode::u64_from_index(expected_offset)
    );
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

#[test]
fn unknown_record_retention_preserves_its_resource_refusal() {
    use crate::test_support::with_context;
    use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};

    let source = text_sphere_stream(1.0);
    let header = with_context(&source, &DecodePolicy::service(), |ctx| {
        cadmpeg_asm::sat::parse(ctx, &source)
            .expect("text stream parses")
            .header
            .as_kernel_header(ctx)
            .expect("kernel header")
    });
    let mut brep = cadmpeg_asm::brep::AsmBrep::default();
    brep.unknowns.push(cadmpeg_ir::UnknownRecord::retained(
        cadmpeg_ir::ids::UnknownId::mint("sat:test:unknown#1").expect("unknown identity"),
        0,
        vec![1],
        vec!["sat:test:unknown#2".into()],
    ));
    let (matched, kernel) = crate::dialect::layers(&crate::dialect::StreamEvidence::Text(None));
    let mut policy = DecodePolicy::service();
    // One dialect layer, twelve native arenas and two coverage nodes precede the unknown links.
    policy.limits.max_collection_items = 15;
    let error = with_context(&[], &policy, |ctx| {
        super::build_result(
            ctx,
            Some(brep),
            std::collections::BTreeMap::new(),
            &header,
            None,
            matched,
            &kernel,
        )
    })
    .expect_err("unknown link exceeds the remaining collection allowance");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "native unknown product links"));
}

#[test]
fn sat_encodings_admit_declared_and_actual_entities_once() {
    let mut options = cadmpeg_ir::codec::DecodeOptions::default();
    // Six native records and five emitted neutral entities.
    options.policy.limits.max_entities = 11;
    for bytes in [
        text_sphere_stream(1.0),
        binary_sphere_stream(BinaryFixtureKind::Asm),
        binary_sphere_stream(BinaryFixtureKind::Acis),
    ] {
        let decoded = SatCodec
            .decode(&mut Cursor::new(bytes), &options)
            .expect("declared records share the actual population admission");
        assert_eq!(decoded.ir().model.bodies.len(), 1);
        assert_eq!(decoded.ir().model.surfaces.len(), 1);
    }
}

#[test]
fn sat_annotation_storage_uses_the_callers_collection_budget() {
    use crate::test_support::with_context;
    use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};

    let source = text_sphere_stream(1.0);
    let header = with_context(&source, &DecodePolicy::service(), |ctx| {
        cadmpeg_asm::sat::parse(ctx, &source)
            .expect("text stream parses")
            .header
            .as_kernel_header(ctx)
            .expect("kernel header")
    });
    let mut brep = cadmpeg_asm::brep::AsmBrep::default();
    brep.annotation_records
        .push(cadmpeg_asm::brep::annotations::AnnotationRecord {
            id: "sat:brep:entity#1".into(),
            stream: "stream".into(),
            offset: 0,
            tag: cadmpeg_asm::brep::annotations::AnnotationTag::Record("sphere-surface".into()),
            derived_fields: Vec::new(),
        });
    let (matched, kernel) = crate::dialect::layers(&crate::dialect::StreamEvidence::Text(None));
    let mut policy = DecodePolicy::service();
    // One dialect layer, twelve native arenas and two coverage nodes precede the handle.
    policy.limits.max_collection_items = 15;
    let error = with_context(&[], &policy, |ctx| {
        super::build_result(
            ctx,
            Some(brep),
            std::collections::BTreeMap::new(),
            &header,
            None,
            matched,
            &kernel,
        )
    })
    .expect_err("annotation stream handle exceeds the preceding collection slots");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "allocate annotation stream handle"));
}

#[test]
fn sat_container_only_stops_before_entity_decode_for_all_encodings() {
    let mut options = cadmpeg_ir::codec::DecodeOptions {
        container_only: true,
        ..Default::default()
    };
    options.policy.limits.max_entities = 0;
    for bytes in [
        text_sphere_stream(1.0),
        binary_sphere_stream(BinaryFixtureKind::Asm),
        binary_sphere_stream(BinaryFixtureKind::Acis),
        acis_text_sphere_stream(21_800),
    ] {
        let result = SatCodec
            .decode(&mut Cursor::new(bytes), &options)
            .expect("container facts require no entity admission");
        assert!(result.report().container_only());
        assert!(!result.report().geometry_transferred());
        assert!(result.ir().model.surfaces.is_empty());
        assert!(result.ir().model.faces.is_empty());
        assert!(result.ir().model.shells.is_empty());
        assert!(result.ir().model.bodies.is_empty());
        assert!(result.ir().model.points.is_empty());
        assert!(result.report().losses.is_empty());
    }
}

#[test]
fn sat_container_only_ignores_malformed_entity_payload() {
    let source = String::from_utf8(text_sphere_stream(1.0)).expect("text fixture");
    let source = source.replacen(
        "asmheader $-1 -1 @13 232.4.0.65535 #",
        "asmheader @broken #",
        1,
    );
    let mut options = cadmpeg_ir::codec::DecodeOptions {
        container_only: true,
        ..Default::default()
    };
    let result = SatCodec
        .decode(&mut Cursor::new(source.as_bytes()), &options)
        .expect("container scope does not parse entities");
    assert!(result.ir().model.surfaces.is_empty());
    options.container_only = false;
    assert!(SatCodec
        .decode(&mut Cursor::new(source.as_bytes()), &options)
        .is_err());
}

#[test]
fn sat_header_conversion_keeps_geometry_with_default_tolerance() {
    for line in ["20 1.7976931348623157e308 0", "1 5e-324 0"] {
        let source = String::from_utf8(text_sphere_stream(1.0)).expect("text fixture");
        let bytes = source.replacen(
            "1 9.999999999999999547e-07 1.000000000000000036e-10",
            line,
            1,
        );
        for container_only in [false, true] {
            let options = cadmpeg_ir::codec::DecodeOptions {
                container_only,
                ..Default::default()
            };
            let result = SatCodec
                .decode(&mut Cursor::new(bytes.as_bytes()), &options)
                .expect("unrepresentable tolerance does not control record decode");
            assert_eq!(
                result.ir().tolerances.linear,
                cadmpeg_ir::CadIr::empty().tolerances.linear
            );
            assert!(result
                .report()
                .losses
                .iter()
                .any(|loss| loss.code == SatLossCode::HeaderToleranceUnresolved.kind()));
            assert_eq!(result.ir().model.faces.len(), usize::from(!container_only));
        }
    }
}

#[test]
fn sat_unusable_header_values_keep_independent_records() {
    for line in [
        "0 1 0", "-1 1 0", "NaN 1 0", "inf 1 0", "bad 1 0", "1 -1 0", "1 NaN 0", "1 inf 0",
        "1 bad 0", "1 1 -1", "1 1 NaN", "1 1 inf", "1 1 bad",
    ] {
        let source = String::from_utf8(text_sphere_stream(1.0)).expect("text fixture");
        let bytes = source.replacen(
            "1 9.999999999999999547e-07 1.000000000000000036e-10",
            line,
            1,
        );
        let expected = if line.starts_with("1 ") {
            SatLossCode::HeaderToleranceUnresolved
        } else {
            SatLossCode::HeaderLengthUnitUnresolved
        };
        for container_only in [false, true] {
            let options = cadmpeg_ir::codec::DecodeOptions {
                container_only,
                ..Default::default()
            };
            let recovered = SatCodec
                .decode(&mut Cursor::new(bytes.as_bytes()), &options)
                .expect("an unusable header value does not control record framing");
            assert!(
                recovered
                    .report()
                    .losses
                    .iter()
                    .any(|loss| loss.code == expected.kind()),
                "{line}"
            );
            assert_eq!(
                recovered.ir().model.faces.len(),
                usize::from(!container_only)
            );
        }
    }
}

#[test]
fn zero_vertex_tolerance_keeps_topology_and_retains_its_source_record() {
    let source = b"700 0 1 0\n1 T 4 ACIS 1 D\n1 0.01 0.001\n\
body $-1 -1 $-1 $1 $-1 $-1 #\n\
lump $-1 -1 $-1 $-1 $2 $0 #\n\
shell $-1 -1 $-1 $-1 $-1 $3 $-1 $1 #\n\
face $-1 -1 $-1 $-1 $4 $2 $-1 $5 forward single #\n\
loop $-1 -1 $-1 $-1 $6 $3 #\n\
plane-surface $-1 -1 $-1 0 0 0 0 0 1 1 0 0 forward_v I I I I #\n\
coedge $-1 -1 $-1 $6 $6 $-1 $7 forward $4 $-1 #\n\
edge $-1 -1 $-1 $8 0 $8 6.283185307179586 $6 $9 forward @7 unknown #\n\
tvertex $-1 -1 $-1 $7 $10 0 #\n\
ellipse-curve $-1 -1 $-1 0 0 0 0 0 1 10 0 0 1 I I #\n\
point $-1 -1 $-1 10 0 0 #\nEnd-of-ACIS-data\n";
    let result = cadmpeg_test_support::EditableDecodeResult::from(decode_bytes(source));
    assert_eq!(result.ir().model.vertices.len(), 1);
    assert!(result.ir().model.vertices[0].tolerance.is_none());
    assert_eq!(result.ir().model.edges.len(), 1);
    assert_eq!(result.ir().model.faces.len(), 1);
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == SatLossCode::VertexToleranceUnresolved.kind()));
    assert_eq!(
        result
            .source_fidelity()
            .retained_record("sat:brep:tvertex#8")
            .unwrap()
            .data(),
        Some(b"tvertex $-1 -1 $-1 $7 $10 0 #".as_slice())
    );
}

#[test]
fn attachment_headers_keep_record_offsets_and_source_bytes() {
    let prefix = b"X-Sun-Data-Type: default\nX-Sun-Charset: us-ascii\n\n";
    let mut source = prefix.to_vec();
    source.extend(text_sphere_stream(1.0));
    let expected = decode_bytes(&text_sphere_stream(1.0));
    let result = cadmpeg_test_support::EditableDecodeResult::from(decode_bytes(&source));
    assert_eq!(result.ir().model, expected.ir().model);
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == SatLossCode::HeaderMetadataNoncanonical.kind()));
    let retained = result
        .source_fidelity()
        .retained_record("sat:source:header#0")
        .unwrap()
        .data()
        .unwrap();
    assert!(retained.starts_with(prefix));
}

#[test]
fn empty_leading_record_name_uses_its_table_index_and_keeps_the_source() {
    let source = String::from_utf8(text_sphere_stream(1.0))
        .unwrap()
        .replace("End-of-ASM-data", "-opaque $-1 #\nEnd-of-ASM-data");
    let result = cadmpeg_test_support::EditableDecodeResult::from(decode_bytes(source.as_bytes()));
    assert_eq!(result.ir().model.faces.len(), 1);
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == SatLossCode::SourceRecordNameUnresolved.kind()));
    assert_eq!(
        result
            .source_fidelity()
            .retained_record("sat:brep:untyped-record#6")
            .unwrap()
            .data(),
        Some(b"-opaque $-1 #".as_slice())
    );
}

#[test]
fn acis_base_extensions_do_not_shift_shared_entity_fields() {
    let source = String::from_utf8(text_sphere_stream(1.0))
        .unwrap()
        .replacen("23200", "2200", 1)
        .replace(" $-1 -1 $-1 ", " $-1 -1 -1 $-1 ")
        .replace("forward single #", "forward single F T 0 0 0 0 #")
        .replace("End-of-ASM-data", "End-of-ACIS-data");
    let expected = decode_bytes(&text_sphere_stream(1.0));
    let result = cadmpeg_test_support::EditableDecodeResult::from(decode_bytes(source.as_bytes()));
    assert_eq!(result.ir().model, expected.ir().model);
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == SatLossCode::SourceRecordExtensionsUnprojected.kind()));
    assert_eq!(
        result
            .source_fidelity()
            .retained_record("sat:source:record-extensions#0")
            .unwrap()
            .data(),
        Some(source.as_bytes())
    );
}

#[test]
fn legacy_spline_context_is_reported_and_retained_with_its_solved_surface() {
    let source = b"105 0 1 0\n\
body $-1 $1 $-1 $-1 #\n\
lump $-1 $-1 $2 $0 #\n\
shell $-1 $-1 $-1 $3 $1 #\n\
face $-1 $-1 $-1 $2 $-1 $4 0 0 #\n\
spline-surface $-1 0 { exactsur nubs 1 1 open open none none 2 2 0 1 1 1 0 1 1 1 0 0 0 10 0 0 0 10 0 10 10 0 0 } #\n\
End-of-ACIS-data\n";
    let result = cadmpeg_test_support::EditableDecodeResult::from(decode_bytes(source));
    assert_eq!(result.ir().model.faces.len(), 1);
    assert!(matches!(
        result.ir().model.surfaces[0].geometry.solved(),
        Some(SolvedSurfaceGeometry::Nurbs(_))
    ));
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == SatLossCode::SourceRecordExtensionsUnprojected.kind()));
    assert_eq!(
        result
            .source_fidelity()
            .retained_record("sat:source:legacy-context#0")
            .unwrap()
            .data(),
        Some(source.as_slice())
    );
}

#[test]
fn standalone_face_is_retained_when_the_ir_requires_a_shell_owner() {
    let source = b"201 0 1 0\n1 T 4 ACIS 1 D\n1 0.01 0.001\n\
face $-1 $-1 $-1 $-1 $-1 $1 forward single #\n\
plane-surface $-1 0 0 0 0 0 1 1 0 0 forward_v I I I I #\nEnd-of-ACIS-data\n";
    let result = cadmpeg_test_support::EditableDecodeResult::from(decode_bytes(source));
    assert!(result.ir().model.faces.is_empty());
    assert!(result.ir().model.surfaces.is_empty());
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == SatLossCode::TopologyFaceOwnerUnprojected.kind()));
    assert_eq!(
        result
            .source_fidelity()
            .retained_record("sat:brep:face#0")
            .unwrap()
            .data(),
        Some(b"face $-1 $-1 $-1 $-1 $-1 $1 forward single #".as_slice())
    );
    assert_eq!(
        result
            .source_fidelity()
            .retained_record("sat:source:standalone-faces#0")
            .unwrap()
            .data(),
        Some(source.as_slice())
    );
    assert!(cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
        .unwrap()
        .is_ok());
}

#[test]
fn empty_shell_is_retained_without_dangling_region_references() {
    let source = b"105 0 1 0\n\
body $-1 $1 $-1 $-1 #\n\
lump $-1 $-1 $2 $0 #\n\
shell $-1 $3 $-1 $-1 $1 #\n\
shell $-1 $-1 $-1 $4 $1 #\n\
face $-1 $-1 $-1 $3 $-1 $5 0 0 #\n\
plane-surface $-1 0 0 0 0 0 1 1 0 0 0 #\nEnd-of-ACIS-data\n";
    let result = cadmpeg_test_support::EditableDecodeResult::from(decode_bytes(source));
    let model = &result.ir().model;
    assert_eq!(model.faces.len(), 1);
    assert_eq!(model.shells.len(), 1);
    assert_eq!(model.shells[0].id.as_str(), "sat:brep:entity#3");
    assert_eq!(model.regions[0].shells, [model.shells[0].id.clone()]);
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == SatLossCode::TopologyShellUnprojected.kind()));
    assert_eq!(
        result
            .source_fidelity()
            .retained_record("sat:brep:shell#2")
            .unwrap()
            .data(),
        Some(b"shell $-1 $3 $-1 $-1 $1 #".as_slice())
    );
}

#[test]
fn body_owned_wire_reports_the_missing_ir_ownership_and_retains_its_source() {
    let source = b"105 0 1 0\n\
body $-1 $-1 $1 $-1#\n\
wire $-1 $-1 $2 $0 $-1 0#\n\
coedge $-1 $2 $2 $-1 $3 0 $1 $-1#\n\
edge $-1 $4 $5 $2 $6 0#\n\
vertex $-1 $3 $7#\nvertex $-1 $3 $8#\n\
straight-curve $-1 0 0 0 1 0 0#\n\
point $-1 0 0 0#\npoint $-1 5 0 0#\nEnd-of-ACIS-data\n";
    let result = cadmpeg_test_support::EditableDecodeResult::from(decode_bytes(source));
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == SatLossCode::TopologyWireOwnerUnprojected.kind()));
    assert_eq!(
        result
            .source_fidelity()
            .retained_record("sat:source:body-wire#0")
            .unwrap()
            .data(),
        Some(source.as_slice())
    );
}

#[test]
fn omitted_use_curve_interval_has_a_sat_loss_code() {
    use cadmpeg_asm::brep::AsmBrep;
    use cadmpeg_asm::kernel_header::KernelHeader;
    use std::collections::BTreeMap;
    let bytes = text_sphere_stream(1.0);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &bytes,
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .unwrap();
    let stream = cadmpeg_asm::sat::parse(&ctx, &bytes).unwrap();
    let header: KernelHeader = stream.header.as_kernel_header(&ctx).unwrap();
    let evidence = super::StreamEvidence::Text(Some(super::TextEvidence {
        branch: stream.terminator,
        header: &header,
    }));
    let (matched, kernel) = super::layers(&evidence);
    let mut graph = AsmBrep::default();
    graph
        .stats
        .other_record_kinds
        .insert("tcoedge-use-curve-invalid-interval".into(), 1);
    let result = super::build_result(
        &ctx,
        Some(graph),
        BTreeMap::new(),
        &header,
        Some(stream.terminator),
        matched,
        &kernel,
    )
    .unwrap();
    assert!(result
        .body
        .losses
        .iter()
        .any(|loss| loss.code == SatLossCode::GeometryUseCurveIntervalInvalid.kind()));
}

#[test]
fn invalid_edge_tolerance_has_a_loss_and_retains_its_source_record() {
    for tolerance in [0, -1] {
        let record = format!(
            "tedge-edge $-1 -1 $-1 $-1 0 $-1 1 $-1 $-1 forward @7 unknown {tolerance} 0 0 #"
        );
        let source = String::from_utf8(text_sphere_stream(1.0))
            .unwrap()
            .replace("End-of-ASM-data", &format!("{record}\nEnd-of-ASM-data"));
        let result =
            cadmpeg_test_support::EditableDecodeResult::from(decode_bytes(source.as_bytes()));
        assert_eq!(result.ir().model.faces.len(), 1);
        assert!(
            matches!(result.ir().model.surfaces[0].geometry.solved(), Some(SolvedSurfaceGeometry::Sphere(sphere)) if sphere.radius().get() == 25.0)
        );
        assert!(result
            .report()
            .losses
            .iter()
            .any(|loss| loss.code == SatLossCode::EdgeToleranceUnresolved.kind()));
        assert_eq!(
            result
                .source_fidelity()
                .retained_record("sat:brep:tedge#6")
                .unwrap()
                .data(),
            Some(record.as_bytes())
        );
    }
}
