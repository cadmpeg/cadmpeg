// SPDX-License-Identifier: Apache-2.0
//! Self-contained tests: IR documents are built in code (via the IR crate's
//! fixtures or inline), and expected STEP fragments are asserted inline. No test
//! depends on an external STEP consumer.
#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]
#![allow(unused_imports)]

use cadmpeg_ir::codec::{Codec, CodecBackend, Confidence, DecodeOptions};
use cadmpeg_ir::eval::{
    model_curve_point_by_id, model_surface_partials_by_id, model_surface_point_by_id,
};
use cadmpeg_ir::index::ModelIndex;

use crate::ids::StepIdentity;
use cadmpeg_core::decode::{DecodeMode, InspectOptions};
use cadmpeg_ir::examples::unit_cube;
use cadmpeg_ir::geometry::{Curve, CurveGeometry, Surface, SurfaceGeometry};
use cadmpeg_ir::ids::{CurveId, SurfaceId};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::units::Units;
use cadmpeg_ir::CadIr;
use std::fmt::Write as _;
use std::io::Cursor;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::test_support::{decode_inline, export};
use crate::{write_step, StepCodec, StepSchema, StepUnsupportedPolicy, StepWriteOptions};

fn step_zip(entries: &[(&str, &[u8], CompressionMethod)]) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for &(name, bytes, method) in entries {
        writer
            .start_file(
                name,
                SimpleFileOptions::default().compression_method(method),
            )
            .unwrap();
        std::io::Write::write_all(&mut writer, bytes).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

#[test]
fn semantic_decode_uses_the_decode_session_work_budget() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('test','2026-07-14T00:00:00',('cadmpeg'),('cadmpeg'),'cadmpeg-step','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
    let mut semantic_operation = None;
    for max_work_units in 1..=2048 {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        policy.limits.max_work_units = max_work_units;
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(source, &arena, &policy)
                .expect("root fits the test policy");
        let error = crate::reader::decode(source, DecodeOptions::default(), &ctx)
            .expect_err("a small work budget must refuse one decode stage");
        let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
            continue;
        };
        if !matches!(
            limit.context.operation,
            "step_lex_token"
                | "step_parse_record"
                | "step_parse_parameter"
                | "step_anchor_materialization"
                | "step_reference_materialization"
        ) {
            semantic_operation = Some(limit.context.operation);
            break;
        }
    }
    assert_eq!(semantic_operation, Some("step_geometry_decode"));
}

#[test]
fn semantic_decode_admits_ir_entities_at_stage_boundaries() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('test','2026-07-14T00:00:00',('cadmpeg'),('cadmpeg'),'cadmpeg-step','','');FILE_SCHEMA(('AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF'));ENDSEC;DATA;#1=CARTESIAN_POINT('',(1.,2.,3.));#2=VERTEX_POINT('',#1);ENDSEC;END-ISO-10303-21;";
    let mut entity_limit = None;
    for max_entities in 1..=64 {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        policy.limits.max_entities = max_entities;
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(source, &arena, &policy)
                .expect("root fits the test policy");
        let error = crate::reader::decode(source, DecodeOptions::default(), &ctx)
            .expect_err("a model entity must be admitted before the next semantic stage");
        let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
            continue;
        };
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::Entities
            && limit.context.operation == "step_dependency_decode"
        {
            entity_limit = Some(limit);
            break;
        }
    }
    let limit = entity_limit.expect("IR entities must be charged at a semantic boundary");
    assert_eq!(limit.additional, 1);
    assert!(limit.used <= limit.limit);
}

#[test]
fn implicit_face_plane_work_is_charged_before_plane_inference() {
    let point_references = (2..=17)
        .map(|id| format!("#{id}"))
        .collect::<Vec<_>>()
        .join(",");
    let point_records = (2..=17).fold(String::new(), |mut records, id| {
        writeln!(records, "#{id}=CARTESIAN_POINT('',({id}.,0.,0.));").expect("write point fixture");
        records
    });
    let source = format!(
        "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('test','2026-07-14T00:00:00',('cadmpeg'),('cadmpeg'),'cadmpeg-step','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=POLY_LOOP('',({point_references}));{point_records}ENDSEC;END-ISO-10303-21;"
    );
    let mut plane_limit = None;
    for max_work_units in 1..=65_536 {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        policy.limits.max_work_units = max_work_units;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            source.as_bytes(),
            &arena,
            &policy,
        )
        .expect("root fits the test policy");
        let error = crate::reader::decode(source.as_bytes(), DecodeOptions::default(), &ctx)
            .expect_err("bounded implicit-plane work must be refused at some budget");
        let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
            continue;
        };
        if limit.context.operation == "step_implicit_face_plane" {
            plane_limit = Some(limit);
            break;
        }
    }
    let limit = plane_limit.expect("implicit face-plane work must have a stable budget gate");
    assert_eq!(
        limit.dimension,
        cadmpeg_core::decode::ResourceDimension::WorkUnits
    );
    assert_eq!(limit.additional, 16);
    assert!(limit.used <= limit.limit);
}

#[test]
pub(crate) fn codec_detects_and_inspects_ap242_exchange_structure() {
    let bytes = include_bytes!("../tests/fixtures/ap242_minimal.p21");
    let codec = StepCodec::default();

    assert_eq!(codec.detect(bytes), Confidence::High);
    assert_eq!(codec.detect(b"PK\x03\x04"), Confidence::No);

    let summary = codec
        .inspect(&mut Cursor::new(bytes), &InspectOptions::default())
        .expect("inspect minimal AP242");
    assert_eq!(summary.format, "step");
    assert_eq!(summary.container_kind, "iso-10303-21-clear-text");
    assert_eq!(summary.entries.len(), 2);
    assert_eq!(summary.entries[0].name, "HEADER");
    assert_eq!(summary.entries[1].name, "DATA[0]");
    assert_eq!(summary.entries[1].attributes["entity_count"], "2");
    assert_eq!(
        summary.entries[1].attributes["unknown_entities"],
        "EXAMPLE_RECORD:1,OPAQUE_TARGET:1"
    );
    assert!(summary
        .notes
        .iter()
        .any(|note| note.contains("AP242") && note.contains("edition 2")));
}

#[test]
fn codec_detection_matches_part21_trivia_and_keyword_rules() {
    let source = b"/* preamble */\n  iso-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;ENDSEC;END-ISO-10303-21;";
    let codec = StepCodec::default();

    assert_eq!(codec.detect(source), Confidence::High);
    codec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("Part 21 leading trivia and case-insensitive magic");
}

#[test]
fn codec_decodes_step_zip_root_and_reports_archive_members() {
    let root = include_bytes!("../tests/fixtures/ap242_minimal.p21");
    let bytes = step_zip(&[
        ("ISO-10303.p21", root, CompressionMethod::Deflated),
        ("parts/child.p21", b"subsidiary", CompressionMethod::Stored),
        ("preview.bin", b"ancillary", CompressionMethod::Stored),
    ]);
    let codec = StepCodec::default();

    assert_eq!(codec.detect(&bytes), Confidence::Medium);
    let summary = codec
        .inspect(&mut Cursor::new(&bytes), &InspectOptions::default())
        .expect("inspect STEP ZIP");
    assert_eq!(summary.container_kind, "iso-10303-21-zip");
    assert_eq!(
        summary
            .entries
            .iter()
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>(),
        ["ISO-10303.p21", "parts/child.p21", "preview.bin"]
    );
    assert_eq!(summary.entries[0].role, "root-exchange");
    assert_eq!(summary.entries[0].compression, "deflate");
    assert_eq!(summary.entries[1].role, "subsidiary-exchange");
    assert!(summary.entries[0].attributes["logical_sections"].contains("HEADER"));

    let result = codec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .expect("decode STEP ZIP root");
    let source = result.ir().source.as_ref().expect("STEP source metadata");
    assert_eq!(source.format, "step");
    assert_eq!(source.attributes["container_kind"], "iso-10303-21-zip");
    assert_eq!(source.attributes["archive_root"], "ISO-10303.p21");
    assert_eq!(source.attributes["archive_entries"], "3");
    assert!(result
        .report()
        .notes
        .iter()
        .any(|note| note == "container root ISO-10303.p21; archive entries=3"));
}

#[test]
fn codec_resolves_root_references_relative_to_the_archive_directory() {
    let root = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('zip references'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;REFERENCE;#10=<parts/child.p21#target>;ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
    let bytes = step_zip(&[
        ("ISO-10303.p21", root, CompressionMethod::Stored),
        ("parts/child.p21", b"child", CompressionMethod::Stored),
    ]);
    let codec = StepCodec::default();
    let summary = codec
        .inspect(&mut Cursor::new(&bytes), &InspectOptions::default())
        .expect("inspect relative ZIP reference");
    assert!(summary
        .notes
        .iter()
        .any(|note| note == "internal resource #10 -> parts/child.p21#target"));

    let missing = step_zip(&[("ISO-10303.p21", root, CompressionMethod::Stored)]);
    assert!(matches!(
        codec.inspect(&mut Cursor::new(missing), &InspectOptions::default()),
        Err(cadmpeg_core::CodecError::Malformed(_))
    ));

    let escaping = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('zip references'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;REFERENCE;#10=<../outside.p21#target>;ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
    let escaping = step_zip(&[("ISO-10303.p21", escaping, CompressionMethod::Stored)]);
    assert!(matches!(
        codec.inspect(&mut Cursor::new(escaping), &InspectOptions::default()),
        Err(cadmpeg_core::CodecError::Malformed(_))
    ));
}

#[test]
fn codec_rejects_step_zip_without_root_or_with_unsupported_layout() {
    let root = include_bytes!("../tests/fixtures/ap242_minimal.p21");
    let codec = StepCodec::default();

    let missing_root = step_zip(&[("part.p21", root, CompressionMethod::Stored)]);
    assert!(matches!(
        codec.inspect(&mut Cursor::new(missing_root), &InspectOptions::default()),
        Err(cadmpeg_core::CodecError::WrongFormat(_))
    ));

    let unsafe_path = step_zip(&[
        ("ISO-10303.p21", root, CompressionMethod::Stored),
        ("../outside.bin", b"unsafe", CompressionMethod::Stored),
    ]);
    assert!(matches!(
        codec.inspect(&mut Cursor::new(unsafe_path), &InspectOptions::default()),
        Err(cadmpeg_core::CodecError::Malformed(_))
    ));

    let zstd = step_zip(&[("ISO-10303.p21", root, CompressionMethod::Zstd)]);
    assert!(matches!(
        codec.inspect(&mut Cursor::new(zstd), &InspectOptions::default()),
        Err(cadmpeg_core::CodecError::NotImplemented(_))
    ));
}

#[test]
pub(crate) fn codec_inspects_edition3_sections_and_external_references() {
    let bytes = include_bytes!("../tests/fixtures/ap242_ed3_sections.p21");
    let summary = StepCodec::default()
        .inspect(&mut Cursor::new(bytes), &InspectOptions::default())
        .expect("inspect edition 3 sections");

    assert_eq!(
        summary
            .entries
            .iter()
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>(),
        [
            "HEADER",
            "ANCHOR",
            "REFERENCE",
            "DATA[0]",
            "DATA[1]",
            "SIGNATURE"
        ]
    );
    let references = summary
        .entries
        .iter()
        .find(|entry| entry.name == "REFERENCE")
        .unwrap();
    assert_eq!(references.attributes["external_count"], "1");
    assert_eq!(
        references.attributes["external_uris"],
        "https://example.invalid/external-part"
    );
    assert_eq!(summary.entries[3].attributes["unknown_entities"], "");
    assert_eq!(
        summary.entries[4].attributes["unknown_entities"],
        "EXAMPLE_RECORD:1"
    );
    let (exchange, diagnostics) =
        crate::parse::parse(bytes).expect("parse opaque signature payload");
    assert!(diagnostics.is_empty());
    assert_eq!(exchange.signatures.len(), 1);
    let signature = exchange.signatures[0].clone();
    assert!(bytes[signature.clone()]
        .windows(b"MFoGCSqGSIb3DQEHAqBNMEsCAQExDTALBglghkgBZQMEAgEwCwYJKoZIhvcNAQcBMSowKAIBATAFMAACAQEwCwYJYIZIAWUDBAIBMA0GCSqGSIb3DQEBAQUABAA=".len())
        .any(|bytes| bytes == b"MFoGCSqGSIb3DQEHAqBNMEsCAQExDTALBglghkgBZQMEAgEwCwYJKoZIhvcNAQcBMSowKAIBATAFMAACAQEwCwYJYIZIAWUDBAIBMA0GCSqGSIb3DQEBAQUABAA="));
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode signature fixture");
    let unknowns = decoded
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown arena");
    let signature_unknown = unknowns
        .iter()
        .find(|record| record.id.0 == "step:file:signature#0")
        .expect("retained signature");
    assert_eq!(
        decoded
            .source_fidelity()
            .retained_record(&signature_unknown.id.0)
            .and_then(|record| record.data.as_deref()),
        Some(&bytes[signature.clone()])
    );
    assert_eq!(
        exchange.records[&2].partials[0].parameters,
        vec![crate::parse::Value::Reference(1)]
    );
}

#[test]
fn parser_retains_multiple_signature_sections_after_exchange_terminator() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('signatures'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;SIGNATURE;MFoGCSqGSIb3DQEHAqBNMEsCAQExDTALBglghkgBZQMEAgEwCwYJKoZIhvcNAQcBMSowKAIBATAFMAACAQEwCwYJYIZIAWUDBAIBMA0GCSqGSIb3DQEBAQUABAA=\nENDSEC;SIGNATURE;MFoGCSqGSIb3DQEHAqBNMEsCAQExDTALBglghkgBZQMEAgEwCwYJKoZIhvcNAQcBMSowKAIBATAFMAACAQEwCwYJYIZIAWUDBAIBMA0GCSqGSIb3DQEBAQUABAA=\nENDSEC;";
    let (exchange, diagnostics) = crate::parse::parse(source).expect("multiple signatures");

    assert!(diagnostics.is_empty());
    assert_eq!(exchange.signatures.len(), 2);
    assert!(source[exchange.signatures[0].clone()]
        .windows(b"MFoGCSqGSIb3DQEHAqBNMEsCAQExDTALBglghkgBZQMEAgEwCwYJKoZIhvcNAQcBMSowKAIBATAFMAACAQEwCwYJYIZIAWUDBAIBMA0GCSqGSIb3DQEBAQUABAA=".len())
        .any(|bytes| bytes == b"MFoGCSqGSIb3DQEHAqBNMEsCAQExDTALBglghkgBZQMEAgEwCwYJKoZIhvcNAQcBMSowKAIBATAFMAACAQEwCwYJYIZIAWUDBAIBMA0GCSqGSIb3DQEBAQUABAA="));
    assert!(source[exchange.signatures[1].clone()]
        .windows(b"MFoGCSqGSIb3DQEHAqBNMEsCAQExDTALBglghkgBZQMEAgEwCwYJKoZIhvcNAQcBMSowKAIBATAFMAACAQEwCwYJYIZIAWUDBAIBMA0GCSqGSIb3DQEBAQUABAA=".len())
        .any(|bytes| bytes == b"MFoGCSqGSIb3DQEHAqBNMEsCAQExDTALBglghkgBZQMEAgEwCwYJKoZIhvcNAQcBMSowKAIBATAFMAACAQEwCwYJYIZIAWUDBAIBMA0GCSqGSIb3DQEBAQUABAA="));
}

#[test]
fn parser_exposes_the_detached_signature_contract() {
    let source = b" /* leading trivia */ ISO-10303-21;HEADER;FILE_DESCRIPTION(('signature'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;SIGNATURE;MFoGCSqGSIb3DQEHAqBNMEsCAQExDTALBglghkgBZQMEAgEwCwYJKoZIhvcNAQcBMSowKAIBATAFMAACAQEwCwYJYIZIAWUDBAIBMA0GCSqGSIb3DQEBAQUABAA=\nENDSEC;";
    let (exchange, diagnostics) = crate::parse::parse(source).expect("signature contract");

    assert!(diagnostics.is_empty());
    let section = &exchange.signature_sections[0];
    let signed = &source[section.signed.clone()];
    assert!(signed.starts_with(b"ISO-10303-21;"));
    assert!(signed.ends_with(b"END-ISO-10303-21;"));
    assert_eq!(
        &source[section.payload.clone()],
        b"MFoGCSqGSIb3DQEHAqBNMEsCAQExDTALBglghkgBZQMEAgEwCwYJKoZIhvcNAQcBMSowKAIBATAFMAACAQEwCwYJYIZIAWUDBAIBMA0GCSqGSIb3DQEBAQUABAA=\n"
    );
    assert_eq!(section.cms.len(), 92);
    let signed_alphabet = section
        .signed_alphabet_bytes(source)
        .expect("signed source range");
    assert!(!signed_alphabet.contains(&b'\n'));
    assert!(signed_alphabet.ends_with(b"END-ISO-10303-21;"));
    assert_eq!(
        &source[section.span.clone()],
        b"SIGNATURE;MFoGCSqGSIb3DQEHAqBNMEsCAQExDTALBglghkgBZQMEAgEwCwYJKoZIhvcNAQcBMSowKAIBATAFMAACAQEwCwYJYIZIAWUDBAIBMA0GCSqGSIb3DQEBAQUABAA=\nENDSEC;"
    );
}

#[test]
fn parser_ignores_controls_inside_signature_terminators() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('signature'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;SIGNATURE;MFoGCSqGSIb3DQEHAqBNMEsCAQExDTALBglghkgBZQMEAgEwCwYJKoZIhvcNAQcBMSowKAIBATAFMAACAQEwCwYJYIZIAWUDBAIBMA0GCSqGSIb3DQEBAQUABAA=\nEN\nDSEC;";
    let (exchange, _) = crate::parse::parse(source).expect("split signature terminator");
    assert_eq!(exchange.signatures.len(), 1);
}

#[test]
fn parser_rejects_invalid_signature_base64() {
    for (payload, expected_message) in [
        ("YWJjZA==!", "invalid SIGNATURE base64 padding"),
        ("YWJjZA==AAAA", "invalid SIGNATURE base64 padding"),
        ("YWJjZA=", "SIGNATURE base64 content has incomplete quantum"),
    ] {
        let source = format!(
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('signature'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;SIGNATURE;{payload}\nENDSEC;"
        );
        let error = crate::parse::parse(source.as_bytes()).expect_err("invalid signature");
        assert!(matches!(
            error,
            crate::parse::ParseError::Lex(crate::lex::LexError { message, .. })
                if message == expected_message
        ));
    }
}

#[test]
pub(crate) fn decode_preserves_named_opaque_records_with_exact_byte_spans() {
    let bytes = include_bytes!("../tests/fixtures/ap242_minimal.p21");
    let result = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode parsed STEP document");

    assert_eq!(result.ir().source.as_ref().unwrap().format, "step");
    let unknowns = result.ir().native_unknowns("step").unwrap();
    assert_eq!(unknowns.len(), 2);
    assert_eq!(unknowns[0].id.0, "step:data:example_record#1");
    let retained = result
        .source_fidelity()
        .retained_record(&unknowns[0].id.0)
        .expect("opaque payload is retained in source fidelity");
    assert_eq!(
        retained.data.as_deref(),
        Some(&bytes[retained.offset as usize..(retained.offset + retained.byte_len) as usize])
    );
    assert!(unknowns[0]
        .links
        .contains(&"step:data:opaque_target#2".to_string()));
    assert!(!result.report().geometry_transferred);
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("EXAMPLE_RECORD")));
}

#[test]
fn opaque_links_retain_typed_step_targets() {
    let result = decode_inline(
        "#1=EXAMPLE_RECORD('',#2);
        #2=LINE('typed target',#3,#5);
        #3=CARTESIAN_POINT('',(0.,0.,0.));
        #4=DIRECTION('',(1.,0.,0.));
        #5=VECTOR('',#4,1.);",
    );
    let unknowns = result
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown records");
    assert_eq!(unknowns.len(), 1);
    assert_eq!(unknowns[0].links, vec!["step:data:curve#2".to_string()]);
}

#[test]
fn opaque_links_retain_fallback_carrier_targets() {
    let result = decode_inline(
        "#1=TRIMMED_CURVE('',#99,(0.),(1.),.T.,.PARAMETER.);
         #2=EXAMPLE_RECORD('',#1);
         #99=EXAMPLE_RECORD('missing basis');",
    );
    assert!(result
        .ir()
        .model
        .curves
        .iter()
        .any(|curve| curve.id.0 == "step:data:curve#1"));

    let unknowns = result
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown records");
    let example = unknowns
        .iter()
        .find(|record| record.id.0 == "step:data:example_record#2")
        .expect("opaque record referencing fallback carrier");
    assert!(example.links.contains(&"step:data:curve#1".to_string()));

    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(!validation.findings.iter().any(|finding| {
        finding.check == cadmpeg_ir::Check::CarrierReachability
            && finding.entity.as_deref() == Some("step:data:curve#1")
    }));
}

#[test]
pub(crate) fn decode_accounts_for_every_part21_byte() {
    let bytes = include_bytes!("../tests/fixtures/ap242_semantic_pmi.p21");
    let result = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode byte-accounting fixture");
    let attributes = &result.ir().source.as_ref().unwrap().attributes;
    let count = |name: &str| attributes[name].parse::<usize>().unwrap();

    assert!(count("bytes_structural") > 0);
    assert!(count("bytes_typed") > 0);
    assert_eq!(count("bytes_named_opaque"), 0);
    assert_eq!(count("bytes_unclassified"), 0);
    assert_eq!(
        count("bytes_structural")
            + count("bytes_typed")
            + count("bytes_named_opaque")
            + count("bytes_unclassified"),
        bytes.len()
    );
}

#[test]
fn every_repository_step_fixture_has_complete_byte_accounting() {
    let fixtures: &[(&str, &[u8])] = &[
        (
            "ap203_sheet",
            include_bytes!("../tests/fixtures/ap203_sheet.p21"),
        ),
        (
            "ap214_sheet",
            include_bytes!("../tests/fixtures/ap214_sheet.p21"),
        ),
        (
            "ap242_assembly",
            include_bytes!("../tests/fixtures/ap242_assembly.p21"),
        ),
        (
            "ap242_conversion_units",
            include_bytes!("../tests/fixtures/ap242_conversion_units.p21"),
        ),
        (
            "ap242_ed3_sections",
            include_bytes!("../tests/fixtures/ap242_ed3_sections.p21"),
        ),
        (
            "ap242_degree_cone",
            include_bytes!("../tests/fixtures/ap242_degree_cone.p21"),
        ),
        (
            "ap242_external_documents",
            include_bytes!("../tests/fixtures/ap242_external_documents.p21"),
        ),
        (
            "ap242_geometry",
            include_bytes!("../tests/fixtures/ap242_geometry.p21"),
        ),
        (
            "ap242_geometric_set",
            include_bytes!("../tests/fixtures/ap242_geometric_set.p21"),
        ),
        (
            "ap242_mapped_assembly",
            include_bytes!("../tests/fixtures/ap242_mapped_assembly.p21"),
        ),
        (
            "ap242_minimal",
            include_bytes!("../tests/fixtures/ap242_minimal.p21"),
        ),
        (
            "ap242_presentation_pmi",
            include_bytes!("../tests/fixtures/ap242_presentation_pmi.p21"),
        ),
        (
            "ap242_semantic_pmi",
            include_bytes!("../tests/fixtures/ap242_semantic_pmi.p21"),
        ),
        (
            "ap242_tessellation",
            include_bytes!("../tests/fixtures/ap242_tessellation.p21"),
        ),
        (
            "ap242_vertex_loop",
            include_bytes!("../tests/fixtures/ap242_vertex_loop.p21"),
        ),
        (
            "complex_instance",
            include_bytes!("../tests/fixtures/complex_instance.p21"),
        ),
        ("strings", include_bytes!("../tests/fixtures/strings.p21")),
    ];
    for &(name, bytes) in fixtures {
        let result = StepCodec::default()
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap_or_else(|error| panic!("{name}: {error}"));
        let attributes = &result.ir().source.as_ref().unwrap().attributes;
        let count = |key: &str| attributes[key].parse::<usize>().unwrap();
        assert_eq!(count("bytes_unclassified"), 0, "{name}");
        assert_eq!(
            count("bytes_structural")
                + count("bytes_typed")
                + count("bytes_named_opaque")
                + count("bytes_unclassified"),
            bytes.len(),
            "{name}"
        );
    }
}

#[test]
pub(crate) fn decode_and_write_singular_vertex_loops() {
    let bytes = include_bytes!("../tests/fixtures/ap242_vertex_loop.p21");
    let result = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode vertex loops");
    assert_eq!(result.ir().model.loops.len(), 2);
    assert!(result
        .ir()
        .model
        .loops
        .iter()
        .all(|loop_| loop_.coedges.is_empty() && loop_.vertex_uses.len() == 1));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
    let mut encoded = Vec::new();
    write_step(result.ir(), &mut encoded, &StepWriteOptions::default())
        .expect("write vertex loops");
    assert_eq!(
        String::from_utf8(encoded)
            .unwrap()
            .matches("VERTEX_LOOP")
            .count(),
        2
    );
}

#[test]
pub(crate) fn decode_builds_a_valid_connected_sheet_brep() {
    use cadmpeg_ir::topology::{BodyKind, Sense};

    let bytes = include_bytes!("../tests/fixtures/ap214_sheet.p21");
    let result = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode AP214 sheet");

    assert_eq!(result.ir().model.bodies.len(), 1);
    assert_eq!(result.ir().model.bodies[0].kind, BodyKind::Sheet);
    assert_eq!(result.ir().model.regions.len(), 1);
    assert_eq!(result.ir().model.shells.len(), 1);
    assert_eq!(result.ir().model.faces.len(), 1);
    assert_eq!(result.ir().model.loops.len(), 1);
    assert_eq!(result.ir().model.coedges.len(), 3);
    assert_eq!(result.ir().model.edges.len(), 3);
    assert!(result.ir().model.edges.iter().all(|edge| {
        edge.param_range
            .is_some_and(|[start, end]| start.is_finite() && end.is_finite() && start < end)
    }));
    assert_eq!(result.ir().model.vertices.len(), 3);
    assert_eq!(result.ir().model.pcurves.len(), 1);
    assert_eq!(
        result
            .ir()
            .model
            .coedges
            .iter()
            .filter(|coedge| !coedge.pcurves.is_empty())
            .count(),
        1
    );
    assert!(matches!(
        result.ir().model.pcurves[0].geometry,
        cadmpeg_ir::geometry::PcurveGeometry::Line { origin, direction }
            if origin == cadmpeg_ir::math::Point2::new(0.0, 0.0)
                && direction == cadmpeg_ir::math::Point2::new(1.0, 0.0)
    ));
    assert!(result
        .ir()
        .model
        .coedges
        .iter()
        .all(|coedge| coedge.sense == Sense::Forward));
    assert_eq!(result.ir().model.faces[0].sense, Sense::Reversed);
    assert!(result
        .ir()
        .model
        .appearance_bindings
        .iter()
        .any(|binding| matches!(
            binding.target,
            cadmpeg_ir::appearance::AppearanceTarget::Edge(_)
        )));
    assert_eq!(
        result.ir().model.faces[0].color,
        Some(cadmpeg_ir::topology::Color {
            r: 0.9,
            g: 0.1,
            b: 0.1,
            a: 1.0,
        })
    );
    assert_eq!(result.ir().model.presentation_layers.len(), 1);
    assert_eq!(
        result.ir().model.presentation_layers[0].name,
        "machined faces"
    );
    assert!(matches!(
        result.ir().model.presentation_layers[0].items.as_slice(),
        [cadmpeg_ir::PresentationItem::Face { .. }]
    ));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);

    let mut output = Vec::new();
    let report = write_step(result.ir(), &mut output, &StepWriteOptions::default())
        .expect("write sheet pcurve");
    assert!(!report
        .losses
        .iter()
        .any(|loss| loss.message.contains("coedge pcurve(s) use unsupported")));
    let roundtrip = StepCodec::default()
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("decode written pcurve");
    assert_eq!(roundtrip.ir().model.pcurves.len(), 1);
    assert_eq!(roundtrip.ir().model.bodies[0].kind, BodyKind::Sheet);
    assert_eq!(roundtrip.ir().model.presentation_layers.len(), 1);
    assert_eq!(
        roundtrip.ir().model.presentation_layers[0].name,
        "machined faces"
    );
    assert!(roundtrip
        .ir()
        .model
        .appearance_bindings
        .iter()
        .any(|binding| matches!(
            binding.target,
            cadmpeg_ir::appearance::AppearanceTarget::Edge(_)
        )));
    assert_eq!(
        roundtrip
            .ir()
            .model
            .coedges
            .iter()
            .filter(|coedge| !coedge.pcurves.is_empty())
            .count(),
        1
    );
}

#[test]
fn disconnected_source_shell_is_partitioned_into_connected_ir_shells() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#30=OPEN_SHELL('',(#29));",
            "#30=OPEN_SHELL('',(#29,#92));",
        )
        .replace(
            "ENDSEC;\nEND-ISO-10303-21;",
            "#70=CARTESIAN_POINT('',(20.,0.,0.));\n#71=CARTESIAN_POINT('',(30.,0.,0.));\n#72=CARTESIAN_POINT('',(20.,10.,0.));\n#73=VERTEX_POINT('',#70);\n#74=VERTEX_POINT('',#71);\n#75=VERTEX_POINT('',#72);\n#76=DIRECTION('',(0.,0.,1.));\n#77=DIRECTION('',(1.,0.,0.));\n#78=DIRECTION('',(-1.,1.,0.));\n#79=DIRECTION('',(0.,-1.,0.));\n#80=VECTOR('',#77,10.);\n#81=VECTOR('',#78,14.142135623730951);\n#82=VECTOR('',#79,10.);\n#83=LINE('',#70,#80);\n#84=LINE('',#71,#81);\n#85=LINE('',#72,#82);\n#86=EDGE_CURVE('',#73,#74,#83,.T.);\n#87=EDGE_CURVE('',#74,#75,#84,.T.);\n#88=EDGE_CURVE('',#75,#73,#85,.T.);\n#89=ORIENTED_EDGE('',*,*,#86,.T.);\n#90=ORIENTED_EDGE('',*,*,#87,.T.);\n#91=ORIENTED_EDGE('',*,*,#88,.T.);\n#93=EDGE_LOOP('',(#89,#90,#91));\n#94=FACE_OUTER_BOUND('',#93,.T.);\n#95=AXIS2_PLACEMENT_3D('',#70,#76,#77);\n#96=PLANE('',#95);\n#92=ADVANCED_FACE('',(#94),#96,.T.);\nENDSEC;\nEND-ISO-10303-21;",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode disconnected source shell");

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.regions.len(), 1);
    assert_eq!(decoded.ir().model.shells.len(), 2);
    assert_eq!(decoded.ir().model.faces.len(), 2);
    let source_loss = decoded
        .report()
        .losses
        .iter()
        .find(|loss| {
            loss.code
                == cadmpeg_ir::LossKind::shared(cadmpeg_ir::LossTaxonomy::SourceTopologyInvalid)
        })
        .expect("source topology loss");
    assert!(source_loss.message.contains("OPEN_SHELL #30"));
    assert!(source_loss
        .message
        .contains("2 disconnected face components"));
    assert!(source_loss.provenance.is_some());
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn disconnected_brep_outer_shell_is_rejected_without_role_corruption() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#30=OPEN_SHELL('',(#29));",
            "#30=CLOSED_SHELL('',(#29,#92));",
        )
        .replace(
            "#31=SHELL_BASED_SURFACE_MODEL('',(#33));",
            "#31=BREP_WITH_VOIDS('',#30,(#34));\n#34=CLOSED_SHELL('',(#29));",
        )
        .replace(
            "ENDSEC;\nEND-ISO-10303-21;",
            "#70=CARTESIAN_POINT('',(20.,0.,0.));\n#71=CARTESIAN_POINT('',(30.,0.,0.));\n#72=CARTESIAN_POINT('',(20.,10.,0.));\n#73=VERTEX_POINT('',#70);\n#74=VERTEX_POINT('',#71);\n#75=VERTEX_POINT('',#72);\n#76=DIRECTION('',(0.,0.,1.));\n#77=DIRECTION('',(1.,0.,0.));\n#78=DIRECTION('',(-1.,1.,0.));\n#79=DIRECTION('',(0.,-1.,0.));\n#80=VECTOR('',#77,10.);\n#81=VECTOR('',#78,14.142135623730951);\n#82=VECTOR('',#79,10.);\n#83=LINE('',#70,#80);\n#84=LINE('',#71,#81);\n#85=LINE('',#72,#82);\n#86=EDGE_CURVE('',#73,#74,#83,.T.);\n#87=EDGE_CURVE('',#74,#75,#84,.T.);\n#88=EDGE_CURVE('',#75,#73,#85,.T.);\n#89=ORIENTED_EDGE('',*,*,#86,.T.);\n#90=ORIENTED_EDGE('',*,*,#87,.T.);\n#91=ORIENTED_EDGE('',*,*,#88,.T.);\n#93=EDGE_LOOP('',(#89,#90,#91));\n#94=FACE_OUTER_BOUND('',#93,.T.);\n#95=AXIS2_PLACEMENT_3D('',#70,#76,#77);\n#96=PLANE('',#95);\n#92=ADVANCED_FACE('',(#94),#96,.T.);\nENDSEC;\nEND-ISO-10303-21;",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode disconnected BREP outer shell");

    assert!(decoded.ir().model.bodies.is_empty());
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.message
            .contains("STEP topology root #31 rejected: connected outer shell #30")
    }));
}

#[test]
pub(crate) fn decode_builds_a_valid_ap203_sheet_brep() {
    use cadmpeg_ir::topology::BodyKind;

    let bytes = include_bytes!("../tests/fixtures/ap203_sheet.p21");
    let result = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode AP203 sheet");

    assert_eq!(
        result.ir().source.as_ref().unwrap().attributes["schema"],
        "CONFIG_CONTROL_DESIGN"
    );
    assert_eq!(result.ir().model.bodies.len(), 1);
    assert_eq!(result.ir().model.bodies[0].kind, BodyKind::Sheet);
    assert_eq!(result.ir().model.faces.len(), 1);
    assert_eq!(result.ir().model.edges.len(), 3);
    assert_eq!(result.ir().model.vertices.len(), 3);
    let composite = result
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id.as_str() == "step:data:curve#34")
        .expect("outer composite curve");
    assert!(matches!(
        &composite.geometry,
        cadmpeg_ir::geometry::CurveGeometry::Composite {
            segments,
            self_intersect: Some(false)
        } if segments.len() == 1
            && segments[0].curve.as_str() == "step:data:curve#36"
            && segments[0].same_sense
            && segments[0].transition
                == cadmpeg_ir::geometry::CompositeCurveTransition::ContSameGradient
    ));
    assert!(result
        .ir()
        .model
        .procedural_surfaces
        .iter()
        .any(|surface| matches!(
            &surface.definition,
            cadmpeg_ir::geometry::ProceduralSurfaceDefinition::CurveBounded {
                support,
                boundaries,
                implicit_outer: false,
                ..
            } if support.as_str() == "step:data:surface#28"
                && boundaries.as_slice() == [cadmpeg_ir::ids::CurveId("step:data:curve#34".into())]
        )));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);

    let mut encoded = Vec::new();
    write_step(result.ir(), &mut encoded, &StepWriteOptions::default())
        .expect("write composite curve graph");
    let roundtrip = StepCodec::default()
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("decode written composite curve graph");
    assert!(roundtrip
        .ir()
        .model
        .curves
        .iter()
        .any(|curve| matches!(curve.geometry, CurveGeometry::Composite { .. })));
}

#[test]
fn decode_builds_a_face_based_surface_model() {
    use cadmpeg_ir::topology::BodyKind;

    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#31=SHELL_BASED_SURFACE_MODEL('',(#33));",
            "#31=FACE_BASED_SURFACE_MODEL('',(#30));",
        )
        .replace(
            "#30=OPEN_SHELL('',(#29));",
            "#30=CONNECTED_FACE_SET('',(#29));",
        );
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode face-based surface model");

    assert_eq!(
        result.ir().model.bodies.len(),
        1,
        "{:#?}",
        result.report().losses
    );
    assert_eq!(result.ir().model.bodies[0].kind, BodyKind::Sheet);
    assert_eq!(result.ir().model.faces.len(), 1);
    assert!(result
        .report()
        .losses
        .iter()
        .all(|loss| !loss.message.contains("does not resolve to a complete")));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_builds_faceted_brep_polygon_loops() {
    use cadmpeg_ir::topology::BodyKind;

    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#25=EDGE_LOOP('',(#22,#23,#24));",
            "#25=POLY_LOOP('',(#3,#4,#5,#3));",
        )
        .replace("#30=OPEN_SHELL('',(#29));", "#30=CLOSED_SHELL('',(#29));")
        .replace(
            "#31=SHELL_BASED_SURFACE_MODEL('',(#33));",
            "#31=FACETED_BREP('',#30);",
        );
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode faceted brep");

    assert_eq!(
        result.ir().model.bodies.len(),
        1,
        "{:#?}",
        result.report().losses
    );
    assert_eq!(result.ir().model.bodies[0].kind, BodyKind::Solid);
    assert_eq!(result.ir().model.faces.len(), 1);
    assert_eq!(result.ir().model.coedges.len(), 3);
    assert!(result
        .ir()
        .model
        .edges
        .iter()
        .all(|edge| edge.curve.is_none()));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn base_face_with_polygon_loop_gets_an_inferred_plane() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#25=EDGE_LOOP('',(#22,#23,#24));",
            "#25=POLY_LOOP('',(#3,#4,#5));",
        )
        .replace(
            "#29=ADVANCED_FACE('',(#26),#28,.T.);",
            "#29=FACE('',(#26));",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode base face");

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.faces.len(), 1);
    let surface = decoded
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id.as_str() == "step:data:surface#implicit-face-29")
        .expect("implicit face plane");
    let SurfaceGeometry::Plane {
        origin,
        normal,
        u_axis,
    } = &surface.geometry
    else {
        panic!("implicit face did not produce a plane");
    };
    assert_eq!(*normal, Vector3::new(0.0, 0.0, 1.0));
    assert_eq!(*origin, Point3::new(10.0 / 3.0, 10.0 / 3.0, 0.0));
    assert_eq!(*u_axis, Vector3::new(1.0, 0.0, 0.0));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn implicit_face_plane_is_invariant_under_edge_ring_rotation() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#29=ADVANCED_FACE('',(#26),#28,.T.);",
            "#29=FACE('',(#26));",
        );
    let rotated = source.replace(
        "#25=EDGE_LOOP('',(#22,#23,#24));",
        "#25=EDGE_LOOP('',(#23,#24,#22));",
    );
    let first = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode base face");
    let second = StepCodec::default()
        .decode(&mut Cursor::new(rotated), &DecodeOptions::default())
        .expect("decode rotated base face");
    let first_surface = first
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id.as_str() == "step:data:surface#implicit-face-29")
        .expect("first implicit face plane");
    let second_surface = second
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id.as_str() == "step:data:surface#implicit-face-29")
        .expect("rotated implicit face plane");
    assert_eq!(first_surface.geometry, second_surface.geometry);
}

#[test]
fn non_planar_base_face_is_rejected_without_an_inferred_surface() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#25=EDGE_LOOP('',(#22,#23,#24));",
            "#25=POLY_LOOP('',(#3,#4,#5));",
        )
        .replace(
            "#29=ADVANCED_FACE('',(#26),#28,.T.);",
            "#29=FACE('',(#26));",
        )
        .replace(
            "#25=POLY_LOOP('',(#3,#4,#5));",
            "#70=CARTESIAN_POINT('',(5.,5.,1.));\n#25=POLY_LOOP('',(#3,#4,#5,#70));",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode non-planar base face");

    assert!(decoded.ir().model.bodies.is_empty());
    assert!(!decoded
        .ir()
        .model
        .surfaces
        .iter()
        .any(|surface| surface.id.as_str() == "step:data:surface#implicit-face-29"));
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code == cadmpeg_ir::LossKind::shared(cadmpeg_ir::LossTaxonomy::TopologyNotTransferred)
            && loss.severity == cadmpeg_ir::Severity::Error
    }));
}

#[test]
fn complex_outer_face_bound_uses_inherited_attributes() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#26=FACE_OUTER_BOUND('',#25,.T.);",
            "#26=(FACE_BOUND('',#25,.T.) FACE_OUTER_BOUND());",
        )
        .replace(
            "#29=ADVANCED_FACE('',(#26),#28,.T.);",
            "#29=FACE('',(#26));",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode complex face bound");

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.faces.len(), 1);
    let surface = decoded
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id.as_str() == "step:data:surface#implicit-face-29")
        .expect("implicit face plane");
    assert!(matches!(surface.geometry, SurfaceGeometry::Plane { .. }));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn implicit_face_plane_uses_the_outer_loop_only() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#25=EDGE_LOOP('',(#22,#23,#24));",
            "#25=POLY_LOOP('',(#3,#4,#5));",
        )
        .replace(
            "#29=ADVANCED_FACE('',(#26),#28,.T.);",
            "#70=CARTESIAN_POINT('',(2.,2.,0.));\n#71=CARTESIAN_POINT('',(2.,3.,0.));\n#72=CARTESIAN_POINT('',(3.,2.,0.));\n#73=POLY_LOOP('',(#70,#71,#72));\n#74=FACE_BOUND('',#73,.F.);\n#29=FACE('',(#74,#26));",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode base face with a hole");

    let surface = decoded
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id.as_str() == "step:data:surface#implicit-face-29")
        .expect("implicit face plane");
    let SurfaceGeometry::Plane { normal, .. } = surface.geometry else {
        panic!("implicit face did not produce a plane");
    };
    assert_eq!(normal, Vector3::new(0.0, 0.0, 1.0));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn nearly_collinear_implicit_face_is_rejected_without_a_fabricated_plane() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#5=CARTESIAN_POINT('',(0.,10.,0.));",
            "#5=CARTESIAN_POINT('',(20.,0.0000000000002,0.));",
        )
        .replace(
            "#29=ADVANCED_FACE('',(#26),#28,.T.);",
            "#29=FACE('',(#26));",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode nearly collinear base face");

    assert!(decoded.ir().model.bodies.is_empty());
    assert!(decoded.ir().model.surfaces.is_empty());
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code == cadmpeg_ir::LossKind::shared(cadmpeg_ir::LossTaxonomy::TopologyNotTransferred)
            && loss.message.contains("implicit face plane")
    }));
}

#[test]
fn implicit_face_plane_keeps_base_orientation_across_oriented_face() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#25=EDGE_LOOP('',(#22,#23,#24));",
            "#25=POLY_LOOP('',(#3,#4,#5));",
        )
        .replace(
            "#29=ADVANCED_FACE('',(#26),#28,.T.);",
            "#29=FACE('',(#26));",
        )
        .replace("#30=OPEN_SHELL('',(#29));", "#30=OPEN_SHELL('',(#34));")
        .replace(
            "#31=SHELL_BASED_SURFACE_MODEL('',(#33));",
            "#31=SHELL_BASED_SURFACE_MODEL('',(#33));\n#34=ORIENTED_FACE('',#29,.F.);",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode oriented base face");

    let surface = decoded
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id.as_str() == "step:data:surface#implicit-face-34")
        .expect("implicit face plane");
    let SurfaceGeometry::Plane { normal, .. } = surface.geometry else {
        panic!("implicit face did not produce a plane");
    };
    assert_eq!(normal, Vector3::new(0.0, 0.0, 1.0));
    assert_eq!(
        decoded.ir().model.faces[0].sense,
        cadmpeg_ir::topology::Sense::Forward
    );
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn base_edges_without_curve_carriers_remain_topological_edges() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace("#19=EDGE_CURVE('',#6,#7,#57,.T.);", "#19=EDGE('',#6,#7);")
        .replace("#20=EDGE_CURVE('',#7,#8,#17,.T.);", "#20=EDGE('',#7,#8);")
        .replace("#21=EDGE_CURVE('',#8,#6,#18,.T.);", "#21=EDGE('',#8,#6);")
        .replace("#16=LINE('',#3,#13);", "#16=UNSUPPORTED_CURVE('',());")
        .replace("#17=LINE('',#4,#14);", "#17=UNSUPPORTED_CURVE('',());")
        .replace("#18=LINE('',#5,#15);", "#18=UNSUPPORTED_CURVE('',());")
        .replace(
            "#57=SURFACE_CURVE('',#16,(#56),.PCURVE_S1.);",
            "#57=UNSUPPORTED_CURVE('',());",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode base edges");

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.edges.len(), 3);
    assert!(decoded
        .ir()
        .model
        .edges
        .iter()
        .all(|edge| edge.curve.is_none()));
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code == cadmpeg_ir::LossKind::shared(cadmpeg_ir::LossTaxonomy::ReferenceGraphNotClosed)
            && loss
                .message
                .contains("edge #19 has no decoded surface or curve carrier")
    }));
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code == cadmpeg_ir::LossKind::shared(cadmpeg_ir::LossTaxonomy::DecodeDiagnostic)
            && loss
                .message
                .contains("STEP edge #19 has no 3D curve carrier")
    }));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn unresolved_vertex_point_does_not_enter_a_topology_draft() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#3=CARTESIAN_POINT('',(0.,0.,0.));",
            "#3=UNSUPPORTED_POINT('',());",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode missing vertex point carrier");

    assert!(decoded.ir().model.bodies.is_empty());
    assert!(decoded.ir().model.vertices.is_empty());
    assert!(decoded.report().losses.iter().any(|loss| loss
        .message
        .contains("VERTEX_POINT #6 has unresolved point carrier #3")));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn rejected_solid_root_reports_an_error_severity_loss() {
    let source =
        String::from_utf8(include_bytes!("../tests/fixtures/ap242_vertex_loop.p21").to_vec())
            .expect("fixture is UTF-8")
            .replace("#10=VERTEX_POINT('',#8);", "#10=VERTEX_POINT('',#4);");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("salvage mode accepts a destroyed solid");

    assert!(decoded.ir().model.bodies.is_empty());
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code == cadmpeg_ir::LossKind::shared(cadmpeg_ir::LossTaxonomy::TopologyNotTransferred)
            && loss.severity == cadmpeg_ir::Severity::Error
    }));
}

#[test]
fn strict_decode_rejects_a_destroyed_solid() {
    let source =
        String::from_utf8(include_bytes!("../tests/fixtures/ap242_vertex_loop.p21").to_vec())
            .expect("fixture is UTF-8")
            .replace("#10=VERTEX_POINT('',#8);", "#10=VERTEX_POINT('',#4);");
    let mut options = DecodeOptions::default();
    options.policy.mode = DecodeMode::Strict;

    let error = StepCodec::default()
        .decode(&mut Cursor::new(source), &options)
        .expect_err("strict mode rejects a destroyed solid");
    assert!(matches!(error, cadmpeg_core::CodecError::Malformed(_)));
}

#[test]
fn sheet_root_salvages_independent_shells() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#31=SHELL_BASED_SURFACE_MODEL('',(#33));",
            "#31=SHELL_BASED_SURFACE_MODEL('',(#33,#34));",
        )
        .replace(
            "#33=ORIENTED_OPEN_SHELL('',*,#30,.F.);",
            "#33=ORIENTED_OPEN_SHELL('',*,#30,.F.);\n#34=ORIENTED_OPEN_SHELL('',*,#99,.T.);\n#99=UNSUPPORTED_SHELL('',());",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode sheet with one invalid shell");

    assert_eq!(
        decoded.ir().model.bodies.len(),
        1,
        "{:#?}",
        decoded.report().losses
    );
    assert!(decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("omitted 1 unresolved shell")));
    assert!(decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("shell carrier #34")));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn shared_source_face_gets_one_owner_scoped_face_per_shell() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#31=SHELL_BASED_SURFACE_MODEL('',(#33));",
            "#31=SHELL_BASED_SURFACE_MODEL('',(#33,#34));",
        )
        .replace(
            "#33=ORIENTED_OPEN_SHELL('',*,#30,.F.);",
            "#33=ORIENTED_OPEN_SHELL('',*,#30,.F.);\n#34=OPEN_SHELL('',(#29));",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode shared face shells");

    assert_eq!(
        decoded.ir().model.bodies.len(),
        2,
        "{:#?}",
        decoded.report().losses
    );
    assert_eq!(decoded.ir().model.faces.len(), 2);
    assert_eq!(
        decoded
            .ir()
            .model
            .faces
            .iter()
            .map(|face| face.id.as_str())
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        2
    );
    assert!(decoded
        .ir()
        .model
        .faces
        .iter()
        .all(|face| face.color.is_some()));
    assert_eq!(decoded.ir().model.presentation_layers[0].items.len(), 2);
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn brep_with_voids_scopes_edges_and_vertices_per_shell_after_shared_shell_use() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace("#30=OPEN_SHELL('',(#29));", "#30=CLOSED_SHELL('',(#29));")
        .replace(
            "#33=ORIENTED_OPEN_SHELL('',*,#30,.F.);",
            "#33=ORIENTED_OPEN_SHELL('',*,#30,.F.);\n#34=CLOSED_SHELL('',(#29));\n#70=BREP_WITH_VOIDS('',#30,(#34));",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode scoped BREP with voids");

    assert!(decoded.ir().model.bodies.iter().any(|body| {
        body.id.as_str() == "step:data:body#70"
            && body.kind == cadmpeg_ir::topology::BodyKind::Solid
    }));
    assert!(!decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("root #70 rejected")));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn first_brep_with_voids_scopes_all_shell_carriers() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace("#30=OPEN_SHELL('',(#29));", "#30=CLOSED_SHELL('',(#29));")
        .replace(
            "#31=SHELL_BASED_SURFACE_MODEL('',(#33));",
            "#31=BREP_WITH_VOIDS('',#30,(#34));\n#34=CLOSED_SHELL('',(#29));",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode first BREP with voids");

    assert!(decoded
        .ir()
        .model
        .bodies
        .iter()
        .any(|body| body.id.as_str() == "step:data:body#31"));
    assert!(decoded
        .report()
        .losses
        .iter()
        .all(|loss| !loss.message.contains("root #31 rejected")));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn oriented_shell_reads_the_derived_cfs_faces_slot() {
    let decoded = StepCodec::default()
        .decode(
            &mut Cursor::new(oriented_closed_shell_source(true)),
            &DecodeOptions::default(),
        )
        .expect("decode specification-form oriented closed shell");

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.faces.len(), 1);
    assert!(decoded
        .ir()
        .model
        .bodies
        .iter()
        .any(|body| body.kind == cadmpeg_ir::topology::BodyKind::Solid));
    assert!(!decoded.report().losses.iter().any(|loss| loss.code
        == cadmpeg_ir::LossKind::shared(cadmpeg_ir::LossTaxonomy::NoncanonicalSourceSyntax)));
}

#[test]
fn oriented_shell_without_the_derived_slot_is_read_and_reported() {
    let source = oriented_closed_shell_source(false);
    let record_offset = source
        .find("#33=ORIENTED_CLOSED_SHELL")
        .expect("oriented shell record");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode noncanonical oriented closed shell");

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.faces.len(), 1);
    let losses = decoded
        .report()
        .losses
        .iter()
        .filter(|loss| {
            loss.code
                == cadmpeg_ir::LossKind::shared(cadmpeg_ir::LossTaxonomy::NoncanonicalSourceSyntax)
        })
        .collect::<Vec<_>>();
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("ORIENTED_CLOSED_SHELL #33"));
    assert_eq!(
        losses[0]
            .provenance
            .as_ref()
            .expect("oriented shell provenance")
            .offset,
        record_offset as u64
    );
}

#[test]
fn strict_decode_rejects_an_oriented_shell_missing_its_derived_slot() {
    let mut options = DecodeOptions::default();
    options.policy.mode = DecodeMode::Strict;
    let error = StepCodec::default()
        .decode(
            &mut Cursor::new(oriented_closed_shell_source(false)),
            &options,
        )
        .expect_err("strict mode rejects a noncanonical oriented shell");

    assert!(matches!(error, cadmpeg_core::CodecError::Malformed(_)));
}

#[test]
fn shell_wire_edge_applies_edge_and_occurrence_sense() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#31=SHELL_BASED_SURFACE_MODEL('',(#33));",
            "#31=SHELL_BASED_WIREFRAME_MODEL('',(#33));",
        )
        .replace(
            "#33=ORIENTED_OPEN_SHELL('',*,#30,.F.);",
            "#33=WIRE_SHELL('',(#25));",
        )
        .replace(
            "#19=EDGE_CURVE('',#6,#7,#57,.T.);",
            "#19=EDGE_CURVE('',#6,#7,#57,.F.);",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode oriented wire edge");

    let first = decoded
        .ir()
        .model
        .edges
        .iter()
        .find(|edge| edge.id.as_str().contains("19-wire-31-33-22-0"))
        .expect("first wire edge");
    assert_eq!(first.start.as_str(), "step:data:vertex#7-wire-31-shell-33");
    assert_eq!(first.end.as_str(), "step:data:vertex#6-wire-31-shell-33");
}

#[test]
fn unowned_pcurve_dependencies_are_retained_as_one_opaque_closure() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "ENDSEC;\nEND-ISO-10303-21;",
            "#69=PCURVE('',#28,#70);\n#70=DEFINITIONAL_REPRESENTATION('',(#71),#50);\n#71=LINE('',#51,#53);\nENDSEC;\nEND-ISO-10303-21;",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode unowned pcurve");
    let unknowns = decoded
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown arena");
    assert!(unknowns
        .iter()
        .any(|record| record.id.0 == "step:data:pcurve#69"));
    assert!(unknowns
        .iter()
        .any(|record| record.id.0 == "step:data:definitional_representation#70"));
    let line = unknowns
        .iter()
        .find(|record| record.id.0 == "step:data:line#71")
        .expect("unowned pcurve line is retained");
    assert!(decoded
        .source_fidelity()
        .retained_record(&line.id.0)
        .expect("unowned pcurve line payload is retained")
        .data
        .as_deref()
        .is_some_and(|data| data.starts_with(b"#71=LINE")));
    assert!(decoded
        .ir()
        .model
        .pcurves
        .iter()
        .all(|pcurve| pcurve.id.as_str() != "step:data:pcurve#69"));
}

#[test]
fn a_protected_unowned_pcurve_stays_opaque() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#19=EDGE_CURVE('',#6,#7,#57,.T.);",
            "#19=EDGE_CURVE('',#6,#7,#72,.T.);",
        )
        .replace(
            "ENDSEC;\nEND-ISO-10303-21;",
            "#69=PCURVE('',#28,#55);\n#72=TRIMMED_CURVE('',#16,(#69,0.),(#69,10.),.T.,.PARAMETER.);\nENDSEC;\nEND-ISO-10303-21;",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode protected unowned pcurve");
    assert!(decoded
        .ir()
        .model
        .pcurves
        .iter()
        .all(|pcurve| { pcurve.id.as_str() != "step:data:pcurve#69" }));
    assert!(decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("protected_pcurves=1")));
    let unknowns = decoded
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown arena");
    assert!(unknowns
        .iter()
        .any(|record| record.id.0 == "step:data:pcurve#69"));
}

#[test]
fn retention_reports_every_deleted_carrier_category() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "ENDSEC;\nEND-ISO-10303-21;",
            "#69=PCURVE('',#78,#70);\n#70=DEFINITIONAL_REPRESENTATION('',(#71,#84),#50);\n#71=LINE('',#51,#53);\n#74=CARTESIAN_POINT('',(20.,20.,0.));\n#75=DIRECTION('',(0.,0.,1.));\n#76=DIRECTION('',(1.,0.,0.));\n#77=AXIS2_PLACEMENT_3D('',#74,#75,#76);\n#78=PLANE('',#77);\n#79=DIRECTION('',(1.,0.,0.));\n#80=VECTOR('',#79,1.);\n#83=LINE('',#74,#80);\n#84=OPAQUE_REFERENCE(#83,#86);\n#86=POLY_LOOP('',(#74));\nENDSEC;\nEND-ISO-10303-21;",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode carrier retention fixture");
    let message = decoded
        .report()
        .losses
        .iter()
        .find(|loss| loss.message.contains("unowned STEP carrier retention"))
        .map(|loss| loss.message.as_str())
        .expect("carrier retention report");
    for category in ["deleted pcurves=1", "points=1", "curves=1", "surfaces=1"] {
        assert!(message.contains(category), "missing {category}: {message}");
    }
    assert!(decoded
        .ir()
        .model
        .pcurves
        .iter()
        .all(|pcurve| pcurve.id.as_str() != "step:data:pcurve#69"));
    assert!(decoded
        .ir()
        .model
        .curves
        .iter()
        .all(|curve| curve.id.as_str() != "step:data:curve#83"));
    assert!(decoded
        .ir()
        .model
        .surfaces
        .iter()
        .all(|surface| surface.id.as_str() != "step:data:surface#78"));
    assert!(decoded
        .ir()
        .model
        .points
        .iter()
        .all(|point| point.id.as_str() != "step:data:point#74"));
}

#[test]
fn unsupported_mandatory_carriers_preserve_topology_as_unknown() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace("#16=LINE('',#3,#13);", "#16=UNSUPPORTED_CURVE('',#3);")
        .replace("#28=PLANE('',#27);", "#28=UNSUPPORTED_SURFACE('',#27);");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode sheet with unknown mandatory carriers");

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.faces.len(), 1);
    assert_eq!(decoded.ir().model.edges.len(), 3);
    assert!(matches!(
        decoded
            .ir()
            .model
            .curves
            .iter()
            .find(|curve| curve.id.as_str() == "step:data:curve#16")
            .map(|curve| &curve.geometry),
        Some(CurveGeometry::Unknown { record: Some(record) })
            if record.as_str() == "step:data:unsupported_curve#16"
    ));
    assert!(matches!(
        decoded
            .ir()
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id.as_str() == "step:data:surface#28")
            .map(|surface| &surface.geometry),
        Some(SurfaceGeometry::Unknown { record: Some(record) })
            if record.as_str() == "step:data:unsupported_surface#28"
    ));
    assert!(decoded
        .report()
        .losses
        .iter()
        .all(|loss| !loss.message.contains("conflicts with decoded topology")));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn unsupported_surface_carrier_on_face_surface_preserves_topology_as_unknown() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace("#28=PLANE('',#27);", "#28=UNSUPPORTED_SURFACE('',#27);")
        .replace(
            "#29=ADVANCED_FACE('',(#26),#28,.T.);",
            "#29=FACE_SURFACE('',(#26),#28,.T.);",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode FACE_SURFACE with unknown carrier");

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert!(matches!(
        decoded
            .ir()
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id.as_str() == "step:data:surface#28")
            .map(|surface| &surface.geometry),
        Some(SurfaceGeometry::Unknown { record: Some(record) })
            if record.as_str() == "step:data:unsupported_surface#28"
    ));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn failed_mandatory_point_root_remains_opaque_and_unbound() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#3=CARTESIAN_POINT('',(0.,0.,0.));",
            "#3=UNSUPPORTED_POINT('',(0.,0.,0.));",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode source with unsupported mandatory vertex point");

    assert!(decoded.ir().model.bodies.is_empty());
    let unknowns = decoded
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown arena");
    assert!(unknowns
        .iter()
        .any(|record| record.id.0 == "step:data:unsupported_point#3"));
    assert!(unknowns
        .iter()
        .any(|record| record.id.0 == "step:data:shell_based_surface_model#31"));
    assert!(decoded.report().losses.iter().any(|loss| loss
        .message
        .contains("STEP topology root #31 rejected: vertex point #3")));
}

#[test]
fn failed_void_shell_does_not_commit_the_outer_brep() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#31=SHELL_BASED_SURFACE_MODEL('',(#33));",
            "#31=BREP_WITH_VOIDS('',#30,(#34));",
        )
        .replace(
            "#33=ORIENTED_OPEN_SHELL('',*,#30,.F.);",
            "#33=OPEN_SHELL('',(#29));\n#34=OPEN_SHELL('',(#99));\n#99=UNSUPPORTED_FACE('',());",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode BREP with invalid void shell");

    assert!(decoded.ir().model.bodies.is_empty());
    assert!(decoded
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown arena")
        .iter()
        .any(|record| record.id.0 == "step:data:brep_with_voids#31"));
    assert!(decoded.report().losses.iter().any(|loss| loss
        .message
        .contains("STEP topology root #31 rejected: face carrier #99")));
}

#[test]
fn disconnected_edge_loop_is_not_committed() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#19=EDGE_CURVE('',#6,#7,#57,.T.);",
            "#19=EDGE_CURVE('',#6,#8,#57,.T.);",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode disconnected edge loop");

    assert!(decoded.ir().model.bodies.is_empty());
    assert!(decoded
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown arena")
        .iter()
        .any(|record| record.id.0 == "step:data:shell_based_surface_model#31"));
    assert!(decoded
        .report()
        .losses
        .iter()
        .all(|loss| !loss.message.contains("conflicts with decoded topology")));
}

#[test]
fn single_edge_loop_must_close_at_its_endpoint() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#25=EDGE_LOOP('',(#22,#23,#24));",
            "#25=EDGE_LOOP('',(#22));",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode open single-edge loop");

    assert!(decoded.ir().model.bodies.is_empty());
    assert!(decoded.report().losses.iter().any(|loss| loss
        .message
        .contains("STEP topology root #31 rejected: edge loop continuity #25")));
}

#[test]
fn seam_edge_preserves_its_explicit_pcurve_reference() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#22=ORIENTED_EDGE('',*,*,#19,.T.);",
            "#22=SEAM_EDGE('',*,*,#19,.T.,#56);",
        )
        .replace(
            "#57=SURFACE_CURVE('',#16,(#56),.PCURVE_S1.);",
            "#57=SEAM_CURVE('',#16,(#56),.PCURVE_S1.);",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode seam edge");

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert!(decoded.ir().model.coedges.iter().any(|coedge| {
        coedge
            .pcurves
            .iter()
            .any(|use_| use_.pcurve.as_str() == "step:data:pcurve#56")
    }));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn seam_edge_does_not_guess_an_unlisted_pcurve_reference() {
    let source = equivalent_seam_source()
        .replace(
            "#22=ORIENTED_EDGE('',*,*,#19,.T.);",
            "#22=SEAM_EDGE('',*,*,#19,.T.,#75);",
        )
        .replace(
            "ENDSEC;\nEND-ISO-10303-21;",
            "#72=CARTESIAN_POINT('',(0.,0.));\n#73=LINE('',#72,#53);\n#74=DEFINITIONAL_REPRESENTATION('',(#73),#50);\n#75=PCURVE('',#28,#74);\nENDSEC;\nEND-ISO-10303-21;",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode seam edge with an unlisted pcurve");

    assert!(decoded.ir().model.coedges.iter().all(|coedge| {
        coedge
            .pcurves
            .iter()
            .all(|use_| use_.pcurve.as_str() != "step:data:pcurve#75")
    }));
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code == cadmpeg_ir::LossKind::shared(cadmpeg_ir::LossTaxonomy::ReferenceGraphNotClosed)
            && loss.severity == cadmpeg_ir::Severity::Warning
    }));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn seam_edge_rejects_an_explicit_pcurve_outside_its_curve() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#22=ORIENTED_EDGE('',*,*,#19,.T.);",
            "#22=SEAM_EDGE('',*,*,#19,.T.,#75);",
        )
        .replace(
            "#57=SURFACE_CURVE('',#16,(#56),.PCURVE_S1.);",
            "#57=SEAM_CURVE('',#16,(#56),.PCURVE_S1.);",
        )
        .replace(
            "ENDSEC;\nEND-ISO-10303-21;",
            "#72=CARTESIAN_POINT('',(0.,0.));\n#73=LINE('',#72,#53);\n#74=DEFINITIONAL_REPRESENTATION('',(#73),#50);\n#75=PCURVE('',#28,#74);\nENDSEC;\nEND-ISO-10303-21;",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode seam edge with an unlisted pcurve");

    assert!(decoded.ir().model.coedges.iter().all(|coedge| {
        coedge
            .pcurves
            .iter()
            .all(|use_| use_.pcurve.as_str() != "step:data:pcurve#75")
    }));
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code == cadmpeg_ir::LossKind::shared(cadmpeg_ir::LossTaxonomy::ReferenceGraphNotClosed)
            && loss.message.contains("SEAM_EDGE #22")
            && loss.message.contains("belongs to its edge curve")
    }));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

fn equivalent_seam_source() -> String {
    String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#57=SURFACE_CURVE('',#16,(#56),.PCURVE_S1.);",
            "#57=SEAM_CURVE('',#16,(#56,#69),.PCURVE_S1.);",
        )
        .replace(
            "ENDSEC;\nEND-ISO-10303-21;",
            "#69=PCURVE('',#28,#70);\n#70=DEFINITIONAL_REPRESENTATION('',(#71),#50);\n#71=LINE('',#51,#53);\nENDSEC;\nEND-ISO-10303-21;",
        )
}

#[test]
fn surface_curve_without_a_basis_keeps_a_curve_less_edge_and_reports_loss() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace("#16=LINE('',#3,#13);", "#16=UNSUPPORTED_CURVE('',());")
        .replace(
            "#57=SURFACE_CURVE('',#16,(#56),.PCURVE_S1.);",
            "#57=SURFACE_CURVE('',*,(#56),.PCURVE_S1.);",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode surface curve without basis");

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert!(decoded
        .ir()
        .model
        .edges
        .iter()
        .find(|edge| edge.id.as_str() == "step:data:edge#19")
        .is_some_and(|edge| edge.curve.is_none()));
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.message
            .contains("STEP edge curve #19: surface-curve #57 has no resolvable basis")
    }));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn subedge_inherits_parent_edge_geometry_without_losing_topology() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#19=EDGE_CURVE('',#6,#7,#57,.T.);",
            "#19=(EDGE('',#6,#7) SUBEDGE('',#58));",
        )
        .replace(
            "#57=SURFACE_CURVE('',#16,(#56),.PCURVE_S1.);",
            "#57=SURFACE_CURVE('',#18,(#56),.PCURVE_S1.);\n#58=EDGE_CURVE('',#6,#7,#18,.F.);",
        )
        .replace(
            "#20=EDGE_CURVE('',#7,#8,#17,.T.);",
            "#20=EDGE_CURVE('',#7,#8,#16,.T.);",
        )
        .replace(
            "#21=EDGE_CURVE('',#8,#6,#18,.T.);",
            "#21=EDGE_CURVE('',#8,#6,#17,.T.);",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode subedge");

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert!(decoded.ir().model.edges.iter().any(|edge| {
        edge.id.as_str() == "step:data:edge#19"
            && edge
                .curve
                .as_ref()
                .is_some_and(|curve| curve.as_str() == "step:data:curve#18")
    }));
    assert!(decoded
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown arena")
        .iter()
        .all(|record| record.id.0 != "step:data:subedge#19"));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn oriented_face_subtype_composes_face_orientation() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace("#30=OPEN_SHELL('',(#29));", "#30=OPEN_SHELL('',(#34));")
        .replace(
            "#31=SHELL_BASED_SURFACE_MODEL('',(#33));",
            "#31=SHELL_BASED_SURFACE_MODEL('',(#33));\n#34=ORIENTED_FACE('',#29,.F.);",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode oriented face");

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.faces.len(), 1);
    assert!(decoded
        .ir()
        .model
        .coedges
        .iter()
        .all(|coedge| coedge.sense == cadmpeg_ir::topology::Sense::Reversed));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn nested_oriented_faces_compose_back_to_the_base_orientation() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace("#30=OPEN_SHELL('',(#29));", "#30=OPEN_SHELL('',(#35));")
        .replace(
            "#31=SHELL_BASED_SURFACE_MODEL('',(#33));",
            "#31=SHELL_BASED_SURFACE_MODEL('',(#33));\n#34=ORIENTED_FACE('',#29,.F.);\n#35=ORIENTED_FACE('',#34,.F.);",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode nested oriented faces");

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.faces.len(), 1);
    assert_eq!(
        decoded.ir().model.faces[0].sense,
        cadmpeg_ir::topology::Sense::Reversed
    );
    assert!(decoded
        .ir()
        .model
        .coedges
        .iter()
        .all(|coedge| coedge.sense == cadmpeg_ir::topology::Sense::Forward));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn complex_oriented_open_shell_preserves_shell_sense() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#33=ORIENTED_OPEN_SHELL('',*,#30,.F.);",
            "#33=(OPEN_SHELL('',(#29)) ORIENTED_OPEN_SHELL('',*,#30,.F.));",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode complex oriented shell");

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(
        decoded.ir().model.faces[0].sense,
        cadmpeg_ir::topology::Sense::Reversed
    );
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn subface_subtype_reuses_parent_surface_and_own_bounds() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace("#30=OPEN_SHELL('',(#29));", "#30=OPEN_SHELL('',(#34));")
        .replace(
            "#31=SHELL_BASED_SURFACE_MODEL('',(#33));",
            "#31=SHELL_BASED_SURFACE_MODEL('',(#33));\n#34=SUBFACE('',(#26),#29);",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode subface");

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.faces.len(), 1);
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn shell_based_wireframe_model_owns_wire_shell_edges() {
    use cadmpeg_ir::topology::BodyKind;

    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#31=SHELL_BASED_SURFACE_MODEL('',(#33));",
            "#31=SHELL_BASED_WIREFRAME_MODEL('',(#33));",
        )
        .replace(
            "#33=ORIENTED_OPEN_SHELL('',*,#30,.F.);",
            "#33=WIRE_SHELL('',(#25));",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode shell-based wireframe model");

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.bodies[0].kind, BodyKind::Wire);
    assert_eq!(decoded.ir().model.edges.len(), 3);
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn shell_based_wireframe_model_retains_vertex_shells() {
    use cadmpeg_ir::topology::BodyKind;

    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#31=SHELL_BASED_SURFACE_MODEL('',(#33));",
            "#31=SHELL_BASED_WIREFRAME_MODEL('',(#33));",
        )
        .replace(
            "#33=ORIENTED_OPEN_SHELL('',*,#30,.F.);",
            "#33=VERTEX_SHELL('',#34);\n#34=VERTEX_LOOP('',#6);",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode vertex shell");

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.bodies[0].kind, BodyKind::Wire);
    assert_eq!(decoded.ir().model.shells[0].free_vertices.len(), 1);
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn connected_edge_sub_set_is_accepted_as_a_wire_boundary() {
    use cadmpeg_ir::topology::BodyKind;

    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#31=SHELL_BASED_SURFACE_MODEL('',(#33));",
            "#31=EDGE_BASED_WIREFRAME_MODEL('',(#33));",
        )
        .replace(
            "#33=ORIENTED_OPEN_SHELL('',*,#30,.F.);",
            "#33=CONNECTED_EDGE_SUB_SET('',(#19,#20,#21),#34);\n#34=CONNECTED_EDGE_SET('',(#19,#20,#21));",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode connected edge sub set");

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.bodies[0].kind, BodyKind::Wire);
    assert_eq!(decoded.ir().model.edges.len(), 3);
    assert!(!decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("has no resolvable parent")));
    assert!(decoded
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown arena")
        .iter()
        .any(|record| record.id.0 == "step:data:connected_edge_set#34"));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn connected_edge_sub_set_keeps_topology_when_parent_is_invalid() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#31=SHELL_BASED_SURFACE_MODEL('',(#33));",
            "#31=EDGE_BASED_WIREFRAME_MODEL('',(#33));",
        )
        .replace(
            "#33=ORIENTED_OPEN_SHELL('',*,#30,.F.);",
            "#33=CONNECTED_EDGE_SUB_SET('',(#19,#20,#21),#34);\n#34=UNSUPPORTED_SET('',());",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode subset with invalid parent");

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert!(decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("parent #34 does not resolve")));
    assert!(decoded
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown arena")
        .iter()
        .any(|record| record.id.0 == "step:data:connected_edge_sub_set#33"));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn connected_face_sub_set_validates_and_uses_its_own_members() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#31=SHELL_BASED_SURFACE_MODEL('',(#33));",
            "#31=FACE_BASED_SURFACE_MODEL('',(#34));",
        )
        .replace(
            "#30=OPEN_SHELL('',(#29));",
            "#30=CONNECTED_FACE_SET('',(#29));",
        )
        .replace(
            "#33=ORIENTED_OPEN_SHELL('',*,#30,.F.);",
            "#34=CONNECTED_FACE_SUB_SET('',(#29),#30);",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode connected face subset");

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.faces.len(), 1);
    assert!(!decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("CONNECTED_FACE_SUB_SET #34")));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn connected_edge_set_resolves_direct_oriented_and_seam_members() {
    use cadmpeg_ir::topology::BodyKind;

    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#31=SHELL_BASED_SURFACE_MODEL('',(#33));",
            "#31=EDGE_BASED_WIREFRAME_MODEL('',(#70));",
        )
        .replace(
            "#33=ORIENTED_OPEN_SHELL('',*,#30,.F.);",
            "#70=CONNECTED_EDGE_SET('',(#71,#72,#73));\n#71=ORIENTED_EDGE('',*,*,#19,.F.);\n#72=SEAM_EDGE('',*,*,#20,.T.,#56);\n#73=ORIENTED_EDGE('',*,*,#21,.T.);",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode direct oriented and seam edge members");

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.bodies[0].kind, BodyKind::Wire);
    assert_eq!(decoded.ir().model.edges.len(), 3);
    let reversed = decoded
        .ir()
        .model
        .edges
        .iter()
        .find(|edge| edge.id.as_str().starts_with("step:data:edge#71-"))
        .expect("oriented edge carrier");
    assert!(reversed.start.as_str().contains("vertex#7"));
    assert!(reversed.end.as_str().contains("vertex#6"));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn shared_edge_wire_model_marks_every_representation_typed() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#31=SHELL_BASED_SURFACE_MODEL('',(#33));",
            "#31=EDGE_BASED_WIREFRAME_MODEL('',(#70));\n#70=CONNECTED_EDGE_SET('',(#19,#20,#21));\n#71=MANIFOLD_SURFACE_SHAPE_REPRESENTATION('',(#31),#2);",
        )
        .replace("#33=ORIENTED_OPEN_SHELL('',*,#30,.F.);", "#33=OPEN_SHELL('',(#29));");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode shared wire model representations");

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert!(!decoded
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown arena")
        .iter()
        .any(|record| { record.id.0 == "step:data:manifold_surface_shape_representation#71" }));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn complex_representation_items_reach_edge_based_wire_models() {
    use cadmpeg_ir::topology::BodyKind;

    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#31=SHELL_BASED_SURFACE_MODEL('',(#33));",
            "#31=EDGE_BASED_WIREFRAME_MODEL('',(#70));\n#70=CONNECTED_EDGE_SET('',(#19,#20,#21));\n#71=(MANIFOLD_SURFACE_SHAPE_REPRESENTATION() REPRESENTATION('',(#31),#2) SHAPE_REPRESENTATION());",
        )
        .replace("#33=ORIENTED_OPEN_SHELL('',*,#30,.F.);", "#33=OPEN_SHELL('',(#29));");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode complex wire representation");

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.bodies[0].kind, BodyKind::Wire);
    assert_eq!(decoded.ir().model.edges.len(), 3);
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn complex_edge_and_oriented_edge_instances_use_named_attributes() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#19=EDGE_CURVE('',#6,#7,#57,.T.);",
            "#19=(EDGE('',#6,#7) EDGE_CURVE('',#57,.T.));",
        )
        .replace(
            "#22=ORIENTED_EDGE('',*,*,#19,.T.);",
            "#22=(EDGE('',*,*) ORIENTED_EDGE('',#19,.T.));",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode complex edge instances");

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.edges.len(), 3);
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn complex_advanced_face_uses_its_explicit_surface_carrier() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace("#28=PLANE('',#27);", "#28=CYLINDRICAL_SURFACE('',#27,5.);")
        .replace(
            "#29=ADVANCED_FACE('',(#26),#28,.T.);",
            "#29=(FACE('',(#26)) FACE_SURFACE('',#28,.T.) ADVANCED_FACE());",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode complex advanced face");

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert!(decoded.ir().model.surfaces.iter().any(|surface| {
        surface.id.as_str() == "step:data:surface#28"
            && matches!(surface.geometry, SurfaceGeometry::Cylinder { .. })
    }));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn advanced_face_name_transfers_through_inherited_representation_item() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#29=ADVANCED_FACE('',(#26),#28,.T.);",
            "#29=ADVANCED_FACE('named face',(#26),#28,.T.);",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode named face");
    assert_eq!(
        decoded.ir().model.faces[0].name.as_deref(),
        Some("named face")
    );

    let mut output = Vec::new();
    write_step(decoded.ir(), &mut output, &StepWriteOptions::default()).expect("write named face");
    let roundtrip = StepCodec::default()
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("decode written named face");
    assert_eq!(
        roundtrip.ir().model.faces[0].name.as_deref(),
        Some("named face")
    );
}

#[test]
fn complex_advanced_face_name_uses_representation_item_partial() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#29=ADVANCED_FACE('',(#26),#28,.T.);",
            "#29=(FACE('',(#26)) FACE_SURFACE('',#28,.T.) ADVANCED_FACE() REPRESENTATION_ITEM('complex named face'));",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode complex named face");
    assert_eq!(
        decoded.ir().model.faces[0].name.as_deref(),
        Some("complex named face")
    );
}

#[test]
fn complex_vertex_point_instances_retain_their_point_carriers() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#6=VERTEX_POINT('',#3);",
            "#6=(VERTEX('',*) VERTEX_POINT('',#3));",
        )
        .replace(
            "#7=VERTEX_POINT('',#4);",
            "#7=(VERTEX('',*) VERTEX_POINT('',#4));",
        )
        .replace(
            "#8=VERTEX_POINT('',#5);",
            "#8=(VERTEX('',*) VERTEX_POINT('',#5));",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode complex vertex points");

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.vertices.len(), 3);
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_builds_a_sheet_from_a_geometric_surface_set() {
    use cadmpeg_ir::topology::BodyKind;

    let bytes = include_bytes!("../tests/fixtures/ap242_geometric_set.p21");
    let result = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode geometric surface set");

    assert_eq!(result.ir().model.bodies.len(), 1);
    assert_eq!(result.ir().model.bodies[0].kind, BodyKind::Sheet);
    assert_eq!(result.ir().model.faces.len(), 1);
    assert!(result.ir().model.faces[0].loops.is_empty());
    assert_eq!(
        result.ir().model.faces[0].surface.as_str(),
        "step:data:surface#11"
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn complex_geometric_set_representation_uses_its_named_items() {
    use cadmpeg_ir::topology::BodyKind;

    let mut source = String::from_utf8(include_bytes!(
        "../tests/fixtures/ap242_geometric_set.p21"
    )
    .to_vec())
    .expect("fixture is UTF-8")
    .replace(
        "#12=GEOMETRIC_SET('',(#11));",
        "#12=GEOMETRIC_SET('',(#11,#14,#15));",
    )
    .replace(
        "#13=GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION('',(#12),#2);",
        "#13=(GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION() REPRESENTATION('',(#12),#2) SHAPE_REPRESENTATION());",
    );
    let end = source.rfind("ENDSEC;").expect("STEP data section end");
    source.insert_str(
        end,
        "#14=(CIRCLE('',#6,5.) CURVE() GEOMETRIC_REPRESENTATION_ITEM() REPRESENTATION_ITEM('free circle'));\n#15=UNSUPPORTED_ITEM('');\n",
    );
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode complex geometric surface set");

    assert_eq!(result.ir().model.bodies.len(), 1);
    assert_eq!(result.ir().model.bodies[0].kind, BodyKind::Sheet);
    assert_eq!(result.ir().model.faces.len(), 1);
    let free_circle = result
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id.as_str() == "step:data:curve#14")
        .expect("free complex representation member");
    assert_eq!(
        free_circle
            .source_object
            .as_ref()
            .and_then(|source| source.name.as_deref()),
        Some("free circle")
    );
    assert!(result.report().losses.iter().any(|loss| {
        loss.message.contains(
            "GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION #13 omitted unsupported or unresolved member(s): #15",
        )
    }));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn presentation_layer_expands_all_product_definition_views() {
    use cadmpeg_ir::presentation::PresentationItem;

    let result = decode_inline(
        "#1=APPLICATION_CONTEXT('mechanical design');
#2=PRODUCT_CONTEXT('',#1,'mechanical');
#3=PRODUCT('P','Part','',(#2));
#4=PRODUCT_DEFINITION_FORMATION('v1','',#3);
#5=PRODUCT_DEFINITION_CONTEXT('part definition',#1,'design');
#6=PRODUCT_DEFINITION('design view','',#4,#5);
#7=PRODUCT_DEFINITION_FORMATION('v2','',#3);
#8=PRODUCT_DEFINITION('manufacturing view','',#7,#5);
#9=PRESENTATION_LAYER_ASSIGNMENT('definition views','',(#3));",
    );

    let layer = result
        .ir()
        .model
        .presentation_layers
        .first()
        .expect("product presentation layer");
    assert!(matches!(
        layer.items.as_slice(),
        [
            PresentationItem::Product { product: first },
            PresentationItem::Product { product: second },
        ] if first.as_str() == "step:product:product#3-definition-6"
            && second.as_str() == "step:product:product#3-definition-8"
    ));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn aliased_topology_root_reuses_the_committed_body_identity() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "ENDSEC;\nEND-ISO-10303-21;",
            "#70=SHELL_BASED_SURFACE_MODEL('',(#33));\n#71=MANIFOLD_SURFACE_SHAPE_REPRESENTATION('',(#70),#2);\n#72=PRODUCT('P','alias part','',());\n#73=PRODUCT_DEFINITION_FORMATION('','',#72);\n#74=APPLICATION_CONTEXT('mechanical design');\n#75=PRODUCT_DEFINITION_CONTEXT('part definition',#74,'design');\n#76=PRODUCT_DEFINITION('part','',#73,#75);\n#77=PRODUCT_DEFINITION_SHAPE('','',#76);\n#78=SHAPE_DEFINITION_REPRESENTATION(#77,#71);\nENDSEC;\nEND-ISO-10303-21;",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode aliased topology root");

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.product_definitions[0].bodies.len(), 1);
    assert_eq!(
        decoded.ir().model.product_definitions[0].bodies[0].as_str(),
        "step:data:body#31"
    );
    assert!(!decoded
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown arena")
        .iter()
        .any(|record| record.id.0 == "step:data:shell_based_surface_model#70"));
}

#[test]
fn topology_root_kind_preserves_distinct_body_kinds_for_shared_shells() {
    use cadmpeg_ir::topology::BodyKind;

    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace("#30=OPEN_SHELL('',(#29));", "#30=CLOSED_SHELL('',(#29));")
        .replace(
            "#33=ORIENTED_OPEN_SHELL('',*,#30,.F.);",
            "#33=ORIENTED_CLOSED_SHELL('',*,#30,.F.);",
        )
        .replace(
            "#31=SHELL_BASED_SURFACE_MODEL('',(#33));",
            "#31=SHELL_BASED_SURFACE_MODEL('',(#30));",
        )
        .replace(
            "ENDSEC;\nEND-ISO-10303-21;",
            "#70=MANIFOLD_SOLID_BREP('',#30);\nENDSEC;\nEND-ISO-10303-21;",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode shared shell roots");

    assert_eq!(decoded.ir().model.bodies.len(), 2);
    assert!(decoded
        .ir()
        .model
        .bodies
        .iter()
        .any(|body| body.kind == BodyKind::Sheet));
    assert!(decoded
        .ir()
        .model
        .bodies
        .iter()
        .any(|body| body.kind == BodyKind::Solid));
}

#[test]
fn reused_shell_in_a_distinct_root_gets_a_new_owner_scope() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "ENDSEC;\nEND-ISO-10303-21;",
            "#70=SHELL_BASED_SURFACE_MODEL('',(#33,#71));\n#71=OPEN_SHELL('',(#29));\nENDSEC;\nEND-ISO-10303-21;",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode reused shell root");

    assert_eq!(
        decoded.ir().model.bodies.len(),
        3,
        "{:#?}",
        decoded.report().losses
    );
    assert_eq!(
        decoded
            .ir()
            .model
            .shells
            .iter()
            .filter(|shell| shell.id.as_str().contains("root-70"))
            .count(),
        2
    );
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn distinct_roots_with_shared_topology_get_owner_scopes() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "ENDSEC;\nEND-ISO-10303-21;",
            "#70=SHELL_BASED_SURFACE_MODEL('',(#71));\n#71=OPEN_SHELL('',(#29));\nENDSEC;\nEND-ISO-10303-21;",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode independent roots sharing topology");

    assert_eq!(
        decoded.ir().model.bodies.len(),
        2,
        "{:#?}",
        decoded.report().losses
    );
    assert!(decoded
        .ir()
        .model
        .edges
        .iter()
        .any(|edge| edge.id.as_str().contains("root-70")));
    assert!(decoded
        .ir()
        .model
        .edges
        .iter()
        .any(|edge| edge.id.as_str().contains("root-31")));
    assert!(decoded
        .ir()
        .model
        .vertices
        .iter()
        .any(|vertex| vertex.id.as_str().contains("root-70")));
    assert!(decoded
        .report()
        .losses
        .iter()
        .all(|loss| !loss.message.contains("conflicts with decoded topology")));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
pub(crate) fn reader_recovers_a_valid_solid_from_writer_output() {
    use cadmpeg_ir::topology::BodyKind;

    let source = unit_cube();
    let mut bytes = Vec::new();
    write_step(&source, &mut bytes, &StepWriteOptions::default()).unwrap();
    let result = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode generated cube STEP");

    assert_eq!(result.ir().model.bodies.len(), 1);
    assert_eq!(result.ir().model.bodies[0].kind, BodyKind::Solid);
    assert_eq!(result.ir().model.faces.len(), 6);
    assert_eq!(result.ir().model.edges.len(), 12);
    assert_eq!(result.ir().model.vertices.len(), 8);
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
pub(crate) fn step_color_assets_round_trip_names_and_tessellation_targets_strictly() {
    let cases: [(&[u8], StepSchema, &[&str]); 2] = [
        (
            include_bytes!("../tests/fixtures/ap214_sheet.p21"),
            StepSchema::Ap214,
            &["override red", "blue green"],
        ),
        (
            include_bytes!("../tests/fixtures/ap242_tessellation.p21"),
            StepSchema::Ap242Edition3,
            &["mesh green"],
        ),
    ];
    for (source, schema, expected_names) in cases {
        let ir = StepCodec::default()
            .decode(&mut Cursor::new(source), &DecodeOptions::default())
            .expect("decode styled STEP")
            .into_parts()
            .0;
        let mut bytes = Vec::new();
        write_step(
            &ir,
            &mut bytes,
            &StepWriteOptions {
                schema,
                unsupported: StepUnsupportedPolicy::Reject,
                ..StepWriteOptions::default()
            },
        )
        .expect("strict styled STEP write");
        let decoded = StepCodec::default()
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("decode written styled STEP");
        let names = decoded
            .ir()
            .model
            .appearances
            .iter()
            .filter_map(|appearance| appearance.name.as_deref())
            .collect::<std::collections::BTreeSet<_>>();
        for expected in expected_names {
            assert!(names.contains(expected), "missing color name {expected}");
        }
        if expected_names == ["mesh green"] {
            assert!(decoded
                .ir()
                .model
                .appearance_bindings
                .iter()
                .any(|binding| {
                    matches!(
                        binding.target,
                        cadmpeg_ir::appearance::AppearanceTarget::Tessellation(_)
                    )
                }));
        }
    }
}

#[test]
fn mapped_presentation_does_not_report_body_placement_loss() {
    let result = decode_inline(
        "#1=CARTESIAN_POINT('',(0.,0.));
#2=DIRECTION('',(1.,0.));
#3=AXIS2_PLACEMENT_2D('',#1,#2);
#4=REPRESENTATION_MAP(#3,#5);
#5=PRESENTATION_VIEW('',(),#6);
#6=REPRESENTATION_CONTEXT('','');
#7=CARTESIAN_POINT('',(10.,0.));
#8=AXIS2_PLACEMENT_2D('',#7,#2);
#9=MAPPED_ITEM('',#4,#8);",
    );
    assert!(!result.report().losses.iter().any(|loss| loss
        .message
        .contains("MAPPED_ITEM #9 has no resolved body placement")));
}

#[test]
fn presentation_layers_target_complex_tessellation_surface_sets() {
    let mut source = String::from_utf8(
        include_bytes!("../tests/fixtures/ap242_tessellation.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "#7=COMPLEX_TRIANGULATED_FACE('strip and fan',#6,4,((1.,0.,0.),(0.,1.,0.),(0.,0.,1.),(0.,0.,-1.)),$,(4,3,2,1),((1,2,3,4)),((1,2,4)));",
        "#7=COMPLEX_TRIANGULATED_SURFACE_SET('strip and fan',#6,4,((1.,0.,0.),(0.,1.,0.),(0.,0.,1.),(0.,0.,-1.)),(4,3,2,1),((1,2,3,4)),((1,2,4)));",
    );
    let end = source.rfind("ENDSEC;").expect("STEP data section end");
    source.insert_str(
        end,
        "#78=PRESENTATION_LAYER_ASSIGNMENT('mesh layer','',(#7));\n",
    );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode layer target");

    let layer = decoded
        .ir()
        .model
        .presentation_layers
        .iter()
        .find(|layer| layer.name == "mesh layer")
        .expect("mesh presentation layer");
    assert!(layer.items.iter().any(|item| matches!(
        item,
        cadmpeg_ir::presentation::PresentationItem::Tessellation { tessellation }
            if tessellation.ends_with("#7")
    )));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);

    let mut output = Vec::new();
    let report = write_step(
        decoded.ir(),
        &mut output,
        &StepWriteOptions {
            schema: StepSchema::Ap242Edition3,
            ..StepWriteOptions::default()
        },
    )
    .expect("write tessellation layer");
    assert!(!report.losses.iter().any(|loss| {
        loss.message
            .contains("layer 'mesh layer' has 1 item(s) without a writable STEP carrier")
    }));
    let roundtrip = StepCodec::default()
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("decode tessellation layer");
    assert!(roundtrip
        .ir()
        .model
        .presentation_layers
        .iter()
        .any(|layer| {
            layer.name == "mesh layer"
                && layer.items.iter().any(|item| {
                    matches!(
                        item,
                        cadmpeg_ir::presentation::PresentationItem::Tessellation { .. }
                    )
                })
        }));
}

#[test]
fn complex_presentation_annotation_inherits_text_and_placement() {
    use cadmpeg_ir::pmi::PmiDefinition;

    let source = String::from_utf8(
        include_bytes!("../tests/fixtures/ap242_presentation_pmi.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "#7=TEXT_LITERAL('inspect surface',#6,'left',.RIGHT.,$);",
        "#7=(GEOMETRIC_REPRESENTATION_ITEM() REPRESENTATION_ITEM('') TEXT_LITERAL('inspect surface',#6,'left',.RIGHT.,$));",
    )
    .replace(
        "#8=ANNOTATION_TEXT_OCCURRENCE('surface note',(),#7);",
        "#8=(ANNOTATION_TEXT_OCCURRENCE() ANNOTATION_OCCURRENCE() DRAUGHTING_ANNOTATION_OCCURRENCE() GEOMETRIC_REPRESENTATION_ITEM() REPRESENTATION_ITEM('surface note') MAPPED_ITEM(#7,#7));",
    );
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode complex presentation PMI");

    assert_eq!(result.ir().model.pmi.len(), 1);
    let PmiDefinition::Presentation {
        ref text,
        ref placement,
        ..
    } = result.ir().model.pmi[0].definition
    else {
        panic!("complex annotation occurrence is not presentation PMI")
    };
    assert_eq!(
        result.ir().model.pmi[0].name.as_deref(),
        Some("surface note")
    );
    assert_eq!(text.as_deref(), Some("inspect surface"));
    let transform = placement.as_ref().expect("annotation placement");
    assert_eq!(transform.rows[0][3], 10.0);
    assert_eq!(transform.rows[1][3], 20.0);
    assert_eq!(transform.rows[2][3], 30.0);
}

#[test]
fn composite_presentation_text_does_not_depend_on_set_order() {
    use cadmpeg_ir::pmi::PmiDefinition;

    let result = decode_inline(
        "#1=TEXT_LITERAL('first',$,'left',.RIGHT.,$);
#2=TEXT_LITERAL('second',$,'left',.RIGHT.,$);
#3=COMPOSITE_TEXT('composite',(#1,#2));
#4=ANNOTATION_TEXT_OCCURRENCE('note',(),#3);",
    );

    let PmiDefinition::Presentation { ref text, .. } = result.ir().model.pmi[0].definition else {
        panic!("composite annotation is not presentation PMI")
    };
    assert!(text.is_none());
    assert!(result.report().losses.iter().any(|loss| {
        loss.code == cadmpeg_ir::LossKind::shared(cadmpeg_ir::LossTaxonomy::MetadataNotTransferred)
            && loss.message.contains("2 reachable text carriers")
    }));
    let unknowns = result
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown records");
    for id in [1, 2, 3] {
        assert!(
            unknowns
                .iter()
                .any(|record| record.id.0.ends_with(&format!("#{id}"))),
            "ambiguous text carrier #{id} was not retained"
        );
    }
}

#[test]
fn presentation_graph_search_does_not_hide_unmodeled_tessellated_carriers() {
    let source = String::from_utf8(
        include_bytes!("../tests/fixtures/ap242_presentation_pmi.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "#7=TEXT_LITERAL('inspect surface',#6,'left',.RIGHT.,$);",
        "#7=(GEOMETRIC_REPRESENTATION_ITEM() REPRESENTATION_ITEM('') TEXT_LITERAL('inspect surface',#6,'left',.RIGHT.,$));",
    )
    .replace(
        "#8=ANNOTATION_TEXT_OCCURRENCE('surface note',(),#7);",
        "#8=(TESSELLATED_GEOMETRIC_SET((#11)) ANNOTATION_TEXT_OCCURRENCE() ANNOTATION_OCCURRENCE() DRAUGHTING_ANNOTATION_OCCURRENCE() GEOMETRIC_REPRESENTATION_ITEM() REPRESENTATION_ITEM('surface note') MAPPED_ITEM(#7,#7));",
    )
    .replace(
        "ENDSEC;\nEND-ISO-10303-21;",
        "#11=(GEOMETRIC_REPRESENTATION_ITEM() REPRESENTATION_ITEM('tess carrier') TESSELLATED_GEOMETRIC_SET((#12)) TESSELLATED_ITEM());\n#12=TESSELLATED_CURVE_SET('tess curve',#13,((1,2)));\n#13=COORDINATES_LIST('',((0.,0.,0.),(1.,0.,0.)));\nENDSEC;\nEND-ISO-10303-21;",
    );
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode presentation graph with tessellated carrier");

    let unknowns = result
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown records");
    for id in [11, 12, 13] {
        assert!(
            unknowns
                .iter()
                .any(|record| record.id.0.ends_with(&format!("#{id}"))),
            "tessellated carrier #{id} was consumed without a neutral representation"
        );
    }
}

#[test]
fn styled_free_curve_is_a_reachable_source_carrier() {
    let result = decode_inline(
        "#1=CARTESIAN_POINT('',(0.,0.,0.));
         #2=CARTESIAN_POINT('',(1.,0.,0.));
         #7=POLYLINE('',(#1,#2));
         #10=STYLED_ITEM('',(),#7);",
    );
    let curve = result
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id.0 == "step:data:curve#7")
        .expect("styled polyline carrier");
    assert_eq!(
        curve
            .source_object
            .as_ref()
            .map(|source| source.object_id.as_str()),
        Some("#10")
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(!validation.findings.iter().any(|finding| {
        finding.check == cadmpeg_ir::Check::CarrierReachability
            && finding.entity.as_deref() == Some("step:data:curve#7")
    }));
}

#[test]
fn complex_tessellated_face_keeps_exact_support_surface_reachable() {
    let source = String::from_utf8(
        include_bytes!("../tests/fixtures/ap242_tessellation.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "#7=COMPLEX_TRIANGULATED_FACE('strip and fan',#6,4,((1.,0.,0.),(0.,1.,0.),(0.,0.,1.),(0.,0.,-1.)),$,(4,3,2,1),((1,2,3,4)),((1,2,4)));",
        "#7=COMPLEX_TRIANGULATED_FACE('strip and fan',#6,4,((1.,0.,0.),(0.,1.,0.),(0.,0.,1.),(0.,0.,-1.)),#79,(4,3,2,1),((1,2,3,4)),((1,2,4)));",
    )
    .replace(
        "ENDSEC;\nEND-ISO-10303-21;",
        "#79=PLANE('exact support',#34);\nENDSEC;\nEND-ISO-10303-21;",
    );
    let result = StepCodec::default()
        .decode(
            &mut Cursor::new(source.as_bytes()),
            &DecodeOptions::default(),
        )
        .expect("decode complex tessellated support");
    let support = result
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id.0 == "step:data:surface#79")
        .expect("exact support surface");
    assert_eq!(
        support
            .source_object
            .as_ref()
            .map(|source| source.object_id.as_str()),
        Some("#7")
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(!validation.findings.iter().any(|finding| {
        finding.check == cadmpeg_ir::Check::CarrierReachability
            && finding.entity.as_deref() == Some("step:data:surface#79")
    }));
}

fn oriented_closed_shell_source(derived_slot: bool) -> String {
    let oriented_shell = if derived_slot {
        "#33=ORIENTED_CLOSED_SHELL('',*,#30,.F.);"
    } else {
        "#33=ORIENTED_CLOSED_SHELL('',#30,.F.);"
    };
    String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace("#30=OPEN_SHELL('',(#29));", "#30=CLOSED_SHELL('',(#29));")
        .replace("#33=ORIENTED_OPEN_SHELL('',*,#30,.F.);", oriented_shell)
        .replace(
            "#31=SHELL_BASED_SURFACE_MODEL('',(#33));",
            "#31=BREP_WITH_VOIDS('',#33,());",
        )
}

#[test]
fn overriding_style_suppresses_the_base_binding() {
    let result = decode_inline(
        "#1=COLOUR_RGB('blue',0.,0.,1.);
#2=PRESENTATION_STYLE_ASSIGNMENT((#1));
#3=COLOUR_RGB('red',1.,0.,0.);
#4=PRESENTATION_STYLE_ASSIGNMENT((#3));
#10=STYLED_ITEM('',(#2),#20);
#11=OVER_RIDING_STYLED_ITEM('',(#4),#20,#10);
#20=SOURCE_ITEM();",
    );
    assert_eq!(result.ir().model.appearance_bindings.len(), 1);
    let binding = &result.ir().model.appearance_bindings[0];
    let appearance = result
        .ir()
        .model
        .appearances
        .iter()
        .find(|appearance| appearance.id == binding.appearance)
        .expect("overriding appearance");
    let color = appearance.base_color.expect("override color");
    assert_eq!((color.r, color.g, color.b), (1.0, 0.0, 0.0));
}

#[test]
fn independent_face_styles_keep_bindings_without_source_order_scalar_color() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#68=STYLED_ITEM('',(#66),#19);",
            "#68=STYLED_ITEM('',(#66),#19);\n#69=COLOUR_RGB('independent blue',0.,0.,1.);\n#70=FILL_AREA_STYLE_COLOUR('',#69);\n#71=FILL_AREA_STYLE('',(#70));\n#72=SURFACE_STYLE_FILL_AREA(#71);\n#73=SURFACE_SIDE_STYLE('',(#72));\n#74=SURFACE_STYLE_USAGE(.BOTH.,#73);\n#75=PRESENTATION_STYLE_ASSIGNMENT((#74));\n#76=STYLED_ITEM('',(#75),#29);",
        );
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode independent face styles");

    let face = result
        .ir()
        .model
        .faces
        .iter()
        .find(|face| face.id.as_str() == "step:data:face#29")
        .expect("styled face");
    assert!(face.color.is_none());
    assert_eq!(
        result
            .ir()
            .model
            .appearance_bindings
            .iter()
            .filter(|binding| {
                matches!(
                    &binding.target,
                    cadmpeg_ir::appearance::AppearanceTarget::Face(face)
                        if face.as_str() == "step:data:face#29"
                )
            })
            .count(),
        2
    );
    assert!(result.report().losses.iter().any(|loss| {
        loss.code == cadmpeg_ir::LossKind::shared(cadmpeg_ir::LossTaxonomy::MetadataNotTransferred)
            && loss.message.contains("#47")
            && loss.message.contains("#76")
            && loss.message.contains("scalar color omitted")
    }));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn presentation_records_retain_non_color_geometry_owners() {
    let result = decode_inline(
        "#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=CARTESIAN_POINT('',(1.,0.,0.));
#3=POLYLINE('styled curve',(#1,#2));
#4=CURVE_STYLE('line style',$,$,$);
#5=PRESENTATION_STYLE_ASSIGNMENT((#4));
#6=STYLED_ITEM('',(#5),#3);
#7=DIRECTION('',(0.,0.,1.));
#8=AXIS2_PLACEMENT_3D('',#1,#7,$);
#9=PLANE('annotation support',#8);
#10=ANNOTATION_PLANE('annotation plane',(),#9,());",
    );

    let curve = result
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id.0 == "step:data:curve#3")
        .expect("styled curve");
    assert_eq!(
        curve
            .source_object
            .as_ref()
            .expect("styled curve owner")
            .object_id,
        "#6"
    );
    let surface = result
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id.0 == "step:data:surface#9")
        .expect("annotation support surface");
    assert_eq!(
        surface
            .source_object
            .as_ref()
            .expect("annotation plane owner")
            .object_id,
        "#10"
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn complex_styled_item_decodes_color_and_owns_its_curve() {
    let result = decode_inline(
        "#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=CARTESIAN_POINT('',(1.,0.,0.));
#3=POLYLINE('styled curve',(#1,#2));
#4=COLOUR_RGB('red',1.,0.,0.);
#5=PRESENTATION_STYLE_ASSIGNMENT((#4));
#6=(ANNOTATION_CURVE_OCCURRENCE() STYLED_ITEM((#5),#3));
#7=COLOUR_RGB('blue',0.,0.,1.);
#8=PRESENTATION_STYLE_ASSIGNMENT((#7));
#9=(ANNOTATION_CURVE_OCCURRENCE() OVER_RIDING_STYLED_ITEM((#8),#3,#6));",
    );

    let curve = result
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id.0 == "step:data:curve#3")
        .expect("complex styled curve");
    assert_eq!(
        curve
            .source_object
            .as_ref()
            .expect("complex styled curve owner")
            .object_id,
        "#6"
    );
    assert!(result.ir().model.appearance_bindings.iter().any(|binding| {
        matches!(
            binding.target,
            cadmpeg_ir::appearance::AppearanceTarget::Curve(ref curve)
                if curve.as_str() == "step:data:curve#3"
        )
    }));
    assert_eq!(result.ir().model.appearance_bindings.len(), 1);
    let appearance = result
        .ir()
        .model
        .appearances
        .iter()
        .find(|appearance| appearance.id == result.ir().model.appearance_bindings[0].appearance)
        .expect("overriding appearance");
    assert_eq!(appearance.name.as_deref(), Some("blue"));
    assert!(result
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown arena")
        .iter()
        .all(|record| record.id.0 != "step:data:styled_item#6"));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn complex_colour_rgb_inherits_name_and_components() {
    let result = decode_inline(
        "#1=(COLOUR_RGB(1.,0.,0.) COLOUR_SPECIFICATION('red') COLOUR());
#2=PRESENTATION_STYLE_ASSIGNMENT((#1));
#3=STYLED_ITEM('',(#2),#4);
#4=SOURCE_ITEM();",
    );
    assert_eq!(result.ir().model.appearance_bindings.len(), 1);
    let appearance = result
        .ir()
        .model
        .appearances
        .first()
        .expect("complex RGB appearance");
    assert_eq!(appearance.name.as_deref(), Some("red"));
    assert_eq!(appearance.base_color.unwrap().r, 1.0);
}

#[test]
fn complex_surface_targets_use_surface_style_domain() {
    let result = decode_inline(
        "#1=COLOUR_RGB('red',1.,0.,0.);
#2=COLOUR_RGB('blue',0.,0.,1.);
#3=CURVE_STYLE('',#1,$,$);
#4=SURFACE_STYLE_RENDERING(#2,$,$,$,$,$);
#5=PRESENTATION_STYLE_ASSIGNMENT((#3,#4));
#6=STYLED_ITEM('',(#5),#7);
#7=(ADVANCED_FACE() FACE_SURFACE());",
    );
    assert_eq!(result.ir().model.appearances.len(), 1);
    assert_eq!(
        result.ir().model.appearances[0].base_color,
        Some(cadmpeg_ir::topology::Color {
            r: 0.0,
            g: 0.0,
            b: 1.0,
            a: 1.0,
        })
    );
}

#[test]
fn surface_style_usage_prefers_positive_side_over_set_order() {
    let result = decode_inline(
        "#1=COLOUR_RGB('negative',1.,0.,0.);
#2=SURFACE_STYLE_RENDERING(#1,$,$,$,$,$);
#3=SURFACE_SIDE_STYLE('',(#2));
#4=SURFACE_STYLE_USAGE(.NEGATIVE.,#3);
#5=COLOUR_RGB('positive',0.,1.,0.);
#6=SURFACE_STYLE_RENDERING(#5,$,$,$,$,$);
#7=SURFACE_SIDE_STYLE('',(#6));
#8=SURFACE_STYLE_USAGE(.POSITIVE.,#7);
#9=PRESENTATION_STYLE_ASSIGNMENT((#4,#8));
#10=STYLED_ITEM('',(#9),#11);
#11=(ADVANCED_FACE() FACE_SURFACE());",
    );
    assert_eq!(result.ir().model.appearances.len(), 1);
    assert_eq!(
        result.ir().model.appearances[0].base_color,
        Some(cadmpeg_ir::topology::Color {
            r: 0.0,
            g: 1.0,
            b: 0.0,
            a: 1.0,
        })
    );
}

#[test]
fn curve_targets_use_curve_style_domain() {
    let result = decode_inline(
        "#1=COLOUR_RGB('surface',1.,0.,0.);
#2=SURFACE_STYLE_RENDERING(#1,$,$,$,$,$);
#3=COLOUR_RGB('curve',0.,1.,0.);
#4=CURVE_STYLE('',#3,$,$);
#5=PRESENTATION_STYLE_ASSIGNMENT((#2,#4));
#6=STYLED_ITEM('',(#5),#7);
#7=POLYLINE('styled curve',());",
    );
    assert_eq!(result.ir().model.appearances.len(), 1);
    assert_eq!(
        result.ir().model.appearances[0].base_color,
        Some(cadmpeg_ir::topology::Color {
            r: 0.0,
            g: 1.0,
            b: 0.0,
            a: 1.0,
        })
    );
}

#[test]
fn point_targets_use_point_style_domain() {
    let result = decode_inline(
        "#1=COLOUR_RGB('surface',1.,0.,0.);
#2=SURFACE_STYLE_RENDERING(#1,$,$,$,$,$);
#3=COLOUR_RGB('point',0.,1.,0.);
#4=POINT_STYLE('',.DOT.,POSITIVE_LENGTH_MEASURE(1.),#3);
#5=PRESENTATION_STYLE_ASSIGNMENT((#2,#4));
#6=STYLED_ITEM('',(#5),#7);
#7=CARTESIAN_POINT('styled point',(0.,0.,0.));",
    );
    assert_eq!(result.ir().model.appearances.len(), 1);
    assert_eq!(
        result.ir().model.appearances[0].base_color,
        Some(cadmpeg_ir::topology::Color {
            r: 0.0,
            g: 1.0,
            b: 0.0,
            a: 1.0,
        })
    );
}

#[test]
fn null_style_branch_does_not_suppress_a_sibling_color() {
    let result = decode_inline(
        "#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=COLOUR_RGB('red',1.,0.,0.);
#3=PRESENTATION_STYLE_ASSIGNMENT((NULL_STYLE(.NULL.),#2));
#4=STYLED_ITEM('',(#3),#1);",
    );
    assert_eq!(result.ir().model.appearance_bindings.len(), 1);
}

#[test]
fn complex_null_style_inherited_partial_suppresses_false_color_warning() {
    let result = decode_inline(
        "#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=(PRESENTATION_STYLE_ASSIGNMENT(()) STYLE_ASSIGNMENT((NULL_STYLE(.NULL.))));
#3=STYLED_ITEM('',(#2),#1);",
    );
    assert!(!result.report().losses.iter().any(|loss| {
        loss.message
            .contains("STYLED_ITEM #3 has no resolved surface color")
    }));
}

#[test]
fn advanced_brep_representation_reuses_its_committed_solid_body() {
    let source = export(&unit_cube()).replace(
        "ADVANCED_BREP_SHAPE_REPRESENTATION",
        "ADVANCED_BREP_REPRESENTATION",
    );
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode advanced B-rep representation");

    assert_eq!(result.ir().model.bodies.len(), 1);
    assert!(!result
        .ir()
        .native_unknowns("step")
        .unwrap()
        .iter()
        .any(|record| record.id.0.contains("advanced_brep_representation")));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn advanced_brep_mapped_representation_reuses_its_committed_solid_body() {
    let mut source = export(&unit_cube()).replace(
        "ADVANCED_BREP_SHAPE_REPRESENTATION",
        "ADVANCED_BREP_REPRESENTATION",
    );
    let representation_line = source
        .lines()
        .find(|line| line.contains("ADVANCED_BREP_REPRESENTATION("))
        .expect("written advanced B-rep representation");
    let representation = representation_line
        .split_once('=')
        .and_then(|(id, _)| id.trim().strip_prefix('#'))
        .and_then(|id| id.parse::<u64>().ok())
        .expect("advanced B-rep representation id");
    let context = representation_line
        .split_once('=')
        .and_then(|(_, record)| record.strip_suffix(';'))
        .and_then(|record| record.strip_suffix(')'))
        .and_then(|record| record.rsplit_once(','))
        .map(|(_, context)| context.trim())
        .expect("advanced B-rep representation context");
    let next_id = source
        .lines()
        .filter_map(|line| {
            line.strip_prefix('#')
                .and_then(|line| line.split_once('='))
                .and_then(|(id, _)| id.trim().parse::<u64>().ok())
        })
        .max()
        .expect("written STEP entity")
        + 1;
    let map = next_id;
    let mapped_item = next_id + 1;
    let mapped_representation = next_id + 2;
    let records = format!(
        "#{map}=REPRESENTATION_MAP($,#{representation});\n\
#{mapped_item}=MAPPED_ITEM('',#{map},$);\n\
#{mapped_representation}=(ADVANCED_BREP_REPRESENTATION() REPRESENTATION('mapped',(#{mapped_item}),{context}) REPRESENTATION_ITEM('mapped'));\n"
    );
    let end = source.rfind("ENDSEC;").expect("STEP data section end");
    source.insert_str(end, &records);

    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode mapped advanced B-rep representation");

    assert_eq!(result.ir().model.bodies.len(), 1);
    assert!(!result.report().losses.iter().any(|loss| {
        loss.message
            .contains("ADVANCED_BREP_REPRESENTATION instance(s) as named opaque STEP records")
    }));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
pub(crate) fn face_outer_bound_is_canonicalized_ahead_of_inner_bounds() {
    use cadmpeg_ir::ids::LoopId;
    use cadmpeg_ir::topology::Loop;

    let mut ir = unit_cube();
    let face = ir.model.faces[0].id.clone();
    let vertex = ir.model.vertices[0].id.clone();
    let inner = LoopId("zzzz:test:loop#inner".into());
    ir.model.loops.push(Loop {
        id: inner.clone(),
        face: face.clone(),
        boundary_role: cadmpeg_ir::topology::LoopBoundaryRole::Inner,
        coedges: Vec::new(),
        vertex_uses: vec![cadmpeg_ir::topology::VertexUse {
            vertex,
            after: None,
            pcurves: Vec::new(),
        }],
    });
    ir.model.faces[0].loops.push(inner);
    let output = export(&ir);
    let (exchange, diagnostics) = crate::parse::parse(output.as_bytes()).unwrap();
    assert!(diagnostics.is_empty());
    let (face_step, outer_bound, inner_bound, outer_loop) = exchange
        .records
        .iter()
        .find_map(|(&face_step, record)| {
            let partial = record.partials.first()?;
            if partial.name != "ADVANCED_FACE" {
                return None;
            }
            let crate::parse::Value::List(bounds) = partial.parameters.get(1)? else {
                return None;
            };
            if bounds.len() != 2 {
                return None;
            }
            let crate::parse::Value::Reference(first) = bounds[0] else {
                return None;
            };
            let crate::parse::Value::Reference(second) = bounds[1] else {
                return None;
            };
            let first_record = exchange.records.get(&first)?.partials.first()?;
            let second_record = exchange.records.get(&second)?.partials.first()?;
            let (outer, inner) = if first_record.name == "FACE_OUTER_BOUND" {
                (first, second)
            } else if second_record.name == "FACE_OUTER_BOUND" {
                (second, first)
            } else {
                return None;
            };
            let crate::parse::Value::Reference(outer_loop) = exchange.records.get(&outer)?.partials
                [0]
            .parameters
            .get(1)?
            else {
                return None;
            };
            Some((face_step, outer, inner, outer_loop))
        })
        .expect("face with outer and inner bounds");
    let ordered = format!("(#{outer_bound},#{inner_bound})");
    let reversed = format!("(#{inner_bound},#{outer_bound})");
    let reordered = output.replacen(&ordered, &reversed, 1);
    assert_ne!(reordered, output);
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(reordered), &DecodeOptions::default())
        .expect("decode reversed face bounds");
    let face = decoded
        .ir()
        .model
        .faces
        .iter()
        .find(|face| face.id.as_str() == StepIdentity::data("face", face_step))
        .expect("decoded face");
    assert_eq!(
        face.loops[0].as_str(),
        StepIdentity::data("loop", format!("{outer_loop}-face-{face_step}"))
    );
}

#[test]
fn duplicate_face_outer_bounds_are_reported_without_inventing_inner_roles() {
    use cadmpeg_ir::ids::LoopId;
    use cadmpeg_ir::topology::Loop;

    let mut ir = unit_cube();
    let face = ir.model.faces[0].id.clone();
    let duplicate = LoopId("synthetic:test:loop#duplicate-outer".into());
    ir.model.loops.push(Loop {
        id: duplicate.clone(),
        face: face.clone(),
        boundary_role: cadmpeg_ir::topology::LoopBoundaryRole::Outer,
        coedges: Vec::new(),
        vertex_uses: vec![cadmpeg_ir::topology::VertexUse {
            vertex: ir.model.vertices[0].id.clone(),
            after: None,
            pcurves: Vec::new(),
        }],
    });
    ir.model.faces[0].loops.push(duplicate);

    let output = export(&ir);
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("decode duplicate outer bounds");
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code == cadmpeg_ir::LossKind::shared(cadmpeg_ir::LossTaxonomy::SourceTopologyInvalid)
            && loss.message.contains("violates the STEP face-bound rule")
            && loss
                .message
                .contains("marking the remaining 1 roles unspecified")
    }));
    let face = &decoded.ir().model.faces[0];
    let roles = face
        .loops
        .iter()
        .map(|id| {
            decoded
                .ir()
                .model
                .loops
                .iter()
                .find(|loop_| loop_.id == *id)
                .expect("decoded face loop")
                .boundary_role
        })
        .collect::<Vec<_>>();
    assert_eq!(
        roles
            .iter()
            .filter(|role| **role == cadmpeg_ir::topology::LoopBoundaryRole::Outer)
            .count(),
        1
    );
    assert_eq!(
        roles
            .iter()
            .filter(|role| **role == cadmpeg_ir::topology::LoopBoundaryRole::Unspecified)
            .count(),
        1
    );
    assert!(cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new()).is_ok());
}

#[test]
fn failed_face_bounds_do_not_duplicate_the_shared_surface() {
    let mut ir = unit_cube();
    ir.model.faces[0].surface = ir.model.faces[1].surface.clone();
    ir.model.faces[0].loops.clear();
    let output = export(&ir);
    // Five face-owned surfaces remain after sharing, and the displaced carrier
    // is retained once as standalone construction geometry.
    assert_eq!(output.matches("= PLANE(").count(), 6);
}

#[test]
pub(crate) fn every_region_of_a_body_is_retained_as_a_shape_item() {
    let mut ir = unit_cube();
    let body = ir.model.bodies[0].id.clone();
    let mut region = ir.model.regions[0].clone();
    region.id.0 = "zzzz:test:region#second".into();
    ir.model.bodies[0].regions.push(region.id.clone());
    ir.model.regions.push(region);
    let mut builder = crate::Builder::new(&ir, StepSchema::Ap242Edition3);
    builder.build();
    assert_eq!(builder.body_item_refs[body.as_str()].len(), 2);
}

#[test]
fn body_layers_and_visibility_cover_every_region_shape_item() {
    use cadmpeg_ir::ids::LayerId;
    use cadmpeg_ir::presentation::{PresentationItem, PresentationLayer};

    let mut ir = unit_cube();
    let body = ir.model.bodies[0].id.clone();
    let mut region = ir.model.regions[0].clone();
    region.id.0 = "zzzz:test:region#second".into();
    ir.model.bodies[0].regions.push(region.id.clone());
    ir.model.regions.push(region);
    ir.model.bodies[0].visible = Some(false);
    ir.model.presentation_layers.push(PresentationLayer {
        id: LayerId("test:layer#body".into()),
        name: "all body regions".into(),
        description: None,
        items: vec![PresentationItem::Body { body }],
    });

    let mut bytes = Vec::new();
    write_step(&ir, &mut bytes, &StepWriteOptions::default()).expect("write body presentation");
    let (exchange, diagnostics) = crate::parse::parse(&bytes).expect("parse body presentation");
    assert!(diagnostics.is_empty());
    let layer = exchange
        .records
        .values()
        .find(|record| {
            record
                .partials
                .iter()
                .any(|partial| partial.name == "PRESENTATION_LAYER_ASSIGNMENT")
        })
        .expect("body presentation layer");
    let layer_partial = layer
        .partials
        .iter()
        .find(|partial| partial.name == "PRESENTATION_LAYER_ASSIGNMENT")
        .unwrap();
    let crate::parse::Value::List(layer_items) = &layer_partial.parameters[2] else {
        panic!("layer items are not an aggregate")
    };
    assert_eq!(layer_items.len(), 2);
    let visibility = exchange
        .records
        .values()
        .find(|record| {
            record
                .partials
                .iter()
                .any(|partial| partial.name == "INVISIBILITY")
        })
        .expect("body invisibility");
    let visibility_partial = visibility
        .partials
        .iter()
        .find(|partial| partial.name == "INVISIBILITY")
        .unwrap();
    let crate::parse::Value::List(hidden_items) = &visibility_partial.parameters[0] else {
        panic!("visibility items are not an aggregate")
    };
    assert_eq!(hidden_items.len(), 2);
}

#[test]
pub(crate) fn presentation_reader_normalizes_invalid_layer_and_common_datum_inputs() {
    use cadmpeg_ir::pmi::PmiDefinition;
    use cadmpeg_ir::presentation::PresentationItem;

    let result = decode_inline(
        "#1=PRESENTATION_LAYER_ASSIGNMENT('','',());
#5=PRODUCT_DEFINITION_SHAPE('PMI shape','',#99);
#7=DATUM('',$,#5,.F.,'A');
#8=DATUM_SYSTEM('system','',#5,.F.,(#20));
#20=DATUM_REFERENCE_COMPARTMENT('',$,#5,.F.,COMMON_DATUM_LIST((#21)),());
#21=DATUM_REFERENCE_ELEMENT('',$,#5,.F.,#7,());
#30=PLUS_MINUS_TOLERANCE(#31,#32);
#31=UNKNOWN_LIMIT();
#32=UNKNOWN_CHARACTERISTIC();
#40=PRESENTATION_LAYER_ASSIGNMENT('inspection','',(#30));
#99=UNRESOLVED_PRODUCT();",
    );
    assert_eq!(result.ir().model.presentation_layers.len(), 1);
    assert!(matches!(
        result.ir().model.presentation_layers[0].items.as_slice(),
        [PresentationItem::Source { source_id }] if source_id == "#30"
    ));
    assert!(result.ir().model.pmi.iter().any(|annotation| matches!(
        &annotation.definition,
        PmiDefinition::DatumSystem { references }
            if references.len() == 1 && references[0].common_group.is_none()
    )));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn presentation_reader_resolves_complex_datum_reference_inheritance() {
    use cadmpeg_ir::pmi::PmiDefinition;

    let result = decode_inline(
        "#1=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));
#5=PRODUCT_DEFINITION_SHAPE('PMI shape','',#99);
#7=DATUM('',$,#5,.F.,'A');
#8=DATUM_SYSTEM('system','',#5,.F.,(#20));
#20=(DATUM_REFERENCE_COMPARTMENT() GENERAL_DATUM_REFERENCE(COMMON_DATUM_LIST((#21)),(#22)) SHAPE_ASPECT('','',#5,.F.));
#21=(DATUM_REFERENCE_ELEMENT() GENERAL_DATUM_REFERENCE(#7,()) SHAPE_ASPECT('','',#5,.F.));
#22=DATUM_REFERENCE_MODIFIER_WITH_VALUE(.DISTANCE.,#23);
#23=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(0.2),#1);
#99=UNRESOLVED_PRODUCT();",
    );
    let system = result
        .ir()
        .model
        .pmi
        .iter()
        .find(|annotation| annotation.name.as_deref() == Some("system"))
        .expect("complex datum system");
    assert!(matches!(
        &system.definition,
        PmiDefinition::DatumSystem { references }
            if references.len() == 1
                && references[0].datum.as_str() == "step:presentation:pmi#7"
                && references[0].common_group.is_none()
                && references[0].modifiers == ["distance:0.2"]
    ));
    assert!(result
        .report()
        .losses
        .iter()
        .all(|loss| { !loss.message.contains("DATUM_REFERENCE_COMPARTMENT #20") }));
}

#[test]
pub(crate) fn hidden_body_geometry_and_visibility_round_trip() {
    let mut ir = unit_cube();
    ir.model.bodies[0].visible = Some(false);
    let mut buf = Vec::new();
    let report = write_step(&ir, &mut buf, &StepWriteOptions::default()).unwrap();
    let s = String::from_utf8(buf).unwrap();
    assert!(s.contains("MANIFOLD_SOLID_BREP"));
    assert!(s.contains("ADVANCED_FACE"));
    assert!(s.contains("INVISIBILITY"));
    assert!(report.losses.is_empty());
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(s.into_bytes()), &DecodeOptions::default())
        .expect("decode hidden body");
    assert_eq!(decoded.ir().model.bodies[0].visible, Some(false));

    let mut transformed = unit_cube();
    transformed.model.bodies[0].visible = Some(false);
    transformed.model.bodies[0].transform = Some(cadmpeg_ir::transform::Transform {
        rows: [
            [1.0, 0.0, 0.0, 10.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
    });
    let transformed_text = export(&transformed);
    assert!(transformed_text.contains("MAPPED_ITEM"));
    assert!(!transformed_text.contains("ADVANCED_BREP_SHAPE_REPRESENTATION"));
    let decoded = StepCodec::default()
        .decode(
            &mut Cursor::new(transformed_text),
            &DecodeOptions::default(),
        )
        .expect("decode hidden transformed body");
    assert_eq!(decoded.ir().model.bodies[0].visible, Some(false));

    // An explicitly visible body exports unchanged.
    let mut ir = unit_cube();
    ir.model.bodies[0].visible = Some(true);
    let s = export(&ir);
    assert!(s.contains("MANIFOLD_SOLID_BREP"));
}

#[test]
fn unsupported_invisibility_relation_is_retained_as_opaque() {
    let decoded = decode_inline(
        "#1=INVISIBILITY((#2));
         #2=STYLED_ITEM('',(),#3);
         #3=GEOMETRIC_CURVE_SET('',());",
    );

    assert!(decoded
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown arena")
        .iter()
        .any(|record| record.id.0 == "step:data:invisibility#1"));
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.message
            .contains("INVISIBILITY #1 targets unsupported item #2")
    }));
}

#[test]
pub(crate) fn body_color_becomes_per_face_styled_item_presentation() {
    let mut ir = unit_cube();
    ir.model.bodies[0].color = Some(cadmpeg_ir::topology::Color {
        r: 0.25,
        g: 0.5,
        b: 0.75,
        a: 1.0,
    });
    let face_count = ir.model.faces.len();
    let s = export(&ir);
    assert!(s.contains("COLOUR_RGB('',0.25,0.5,0.75)"));
    assert!(s.contains("MECHANICAL_DESIGN_GEOMETRIC_PRESENTATION_REPRESENTATION"));
    // The body color is pushed down onto every face: one STYLED_ITEM per face,
    // each targeting an ADVANCED_FACE rather than the solid. OCCT/VTK viewers
    // (e.g. f3d) read colors only from faces, not MANIFOLD_SOLID_BREP.
    let styled: Vec<&str> = s.lines().filter(|l| l.contains("STYLED_ITEM")).collect();
    assert_eq!(styled.len(), face_count);
    let solid = s
        .lines()
        .find(|line| line.contains("MANIFOLD_SOLID_BREP"))
        .and_then(|line| line.split(" =").next())
        .unwrap()
        .to_string();
    for item in &styled {
        let target = item
            .rsplit_once(',')
            .map(|(_, tail)| tail.trim_end_matches(");").to_string())
            .unwrap();
        assert_ne!(target, solid, "body color must not style the solid");
        assert!(
            s.lines()
                .any(|line| line.starts_with(&format!("{target} = ADVANCED_FACE"))),
            "styled item must reference a face"
        );
    }
}

#[test]
pub(crate) fn face_appearance_binding_styles_the_advanced_face() {
    use cadmpeg_ir::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
    use cadmpeg_ir::ids::AppearanceId;

    let mut ir = unit_cube();
    let face = ir.model.faces[0].id.clone();
    ir.model.appearances.push(Appearance {
        id: AppearanceId("test:appearance#black".to_string()),
        name: None,
        asset_guid: None,
        library_id: None,
        visual_guid: None,
        physical_token: None,
        schema: None,
        category: None,
        base_color: Some(cadmpeg_ir::topology::Color {
            r: 0.125,
            g: 0.125,
            b: 0.125,
            a: 1.0,
        }),
        properties: std::collections::BTreeMap::default(),
        textures: Vec::new(),
    });
    ir.model.appearance_bindings.push(AppearanceBinding {
        id: "test:appearance-binding#face".to_string(),
        target: AppearanceTarget::Face(face),
        appearance: AppearanceId("test:appearance#black".to_string()),
        source_entity_id: None,
        object_type: None,
        channels: std::collections::BTreeMap::default(),
    });
    let s = export(&ir);
    assert!(s.contains("COLOUR_RGB('',0.125,0.125,0.125)"));
    let styled: Vec<&str> = s.lines().filter(|l| l.contains("STYLED_ITEM")).collect();
    assert_eq!(styled.len(), 1);
    // The styled item targets an ADVANCED_FACE instance.
    let target = styled[0]
        .rsplit_once(',')
        .map(|(_, tail)| tail.trim_end_matches(");").to_string())
        .unwrap();
    let face_line = s
        .lines()
        .find(|line| line.starts_with(&format!("{target} = ADVANCED_FACE")));
    assert!(face_line.is_some(), "styled item must reference a face");
}

#[test]
fn vertex_appearance_binding_styles_the_vertex_point() {
    use cadmpeg_ir::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
    use cadmpeg_ir::ids::AppearanceId;

    let mut ir = unit_cube();
    let vertex = ir.model.vertices[0].id.clone();
    ir.model.appearances.push(Appearance {
        id: AppearanceId("test:appearance#vertex".to_string()),
        name: Some("vertex green".to_string()),
        asset_guid: None,
        library_id: None,
        visual_guid: None,
        physical_token: None,
        schema: None,
        category: None,
        base_color: Some(cadmpeg_ir::topology::Color {
            r: 0.125,
            g: 0.75,
            b: 0.25,
            a: 1.0,
        }),
        properties: std::collections::BTreeMap::default(),
        textures: Vec::new(),
    });
    ir.model.appearance_bindings.push(AppearanceBinding {
        id: "test:appearance-binding#vertex".to_string(),
        target: AppearanceTarget::Vertex(vertex),
        appearance: AppearanceId("test:appearance#vertex".to_string()),
        source_entity_id: None,
        object_type: None,
        channels: std::collections::BTreeMap::default(),
    });

    let text = export(&ir);
    assert!(text.contains("POINT_STYLE"));
    assert!(text.contains("COLOUR_RGB('vertex green',0.125,0.75,0.25)"));

    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(text), &DecodeOptions::default())
        .expect("decode vertex appearance");
    assert!(decoded
        .ir()
        .model
        .appearance_bindings
        .iter()
        .any(|binding| {
            matches!(
                &binding.target,
                AppearanceTarget::Vertex(vertex) if vertex.as_str().starts_with("step:data:vertex#")
            )
        }));
}

#[test]
fn point_presentation_layer_writes_the_cartesian_point_carrier() {
    use cadmpeg_ir::ids::LayerId;
    use cadmpeg_ir::presentation::{PresentationItem, PresentationLayer};

    let mut ir = unit_cube();
    let point = ir.model.points[0].id.clone();
    ir.model.presentation_layers.push(PresentationLayer {
        id: LayerId("test:layer#point".to_string()),
        name: "point layer".to_string(),
        description: Some("standalone points".to_string()),
        items: vec![PresentationItem::Point { point }],
    });

    let mut bytes = Vec::new();
    let report =
        write_step(&ir, &mut bytes, &StepWriteOptions::default()).expect("write point layer");
    assert!(!report.losses.iter().any(|loss| {
        loss.message
            .contains("layer 'point layer' has 1 item(s) without a writable STEP carrier")
    }));
    let text = String::from_utf8(bytes).expect("STEP is UTF-8");
    assert!(text.contains("PRESENTATION_LAYER_ASSIGNMENT('point layer','standalone points',"));
}

#[test]
fn presentation_layer_round_trips_product_occurrence_and_pmi_items() {
    use cadmpeg_ir::ids::{LayerId, OccurrenceId, PmiId, ProductDefinitionId};
    use cadmpeg_ir::pmi::{PmiAnnotation, PmiDefinition};
    use cadmpeg_ir::presentation::{PresentationItem, PresentationLayer};
    use cadmpeg_ir::products::{Occurrence, OccurrenceParent, ProductDefinition};

    let mut ir = unit_cube();
    let body = ir.model.bodies[0].id.clone();
    let parent_product = ProductDefinitionId("test:product#parent".into());
    let child_product = ProductDefinitionId("test:product#child".into());
    ir.model.product_definitions.extend([
        ProductDefinition {
            id: parent_product.clone(),
            kind: cadmpeg_ir::products::ProductDefinitionKind::Part,
            source_name: Some("Parent assembly".into()),
            label: Some("Parent assembly".into()),
            description: None,
            part_number: None,
            bom_properties: std::collections::BTreeMap::default(),
            bodies: Vec::new(),
            native_ref: None,
        },
        ProductDefinition {
            id: child_product.clone(),
            kind: cadmpeg_ir::products::ProductDefinitionKind::Part,
            source_name: Some("Child part".into()),
            label: Some("Child part".into()),
            description: None,
            part_number: None,
            bom_properties: std::collections::BTreeMap::default(),
            bodies: vec![body],
            native_ref: None,
        },
    ]);
    let root = OccurrenceId("test:occurrence#root".into());
    let child = OccurrenceId("test:occurrence#child".into());
    ir.model.occurrences.extend([
        Occurrence {
            id: root.clone(),
            prototype: cadmpeg_ir::products::PrototypeReference::Local {
                definition: parent_product.clone(),
            },
            parent: OccurrenceParent::Root,
            ordinal: 0,
            transform: Transform::identity(),
            prototype_transform: Transform::identity(),
            scale: [1.0; 3],
            name: Some("Root assembly".into()),
            linked_subelements: Vec::new(),
            visible: None,
            element_component: None,
            claim_child: None,
            copy_on_change: None,
            copy_on_change_source: None,
            copy_on_change_group: None,
            copy_on_change_touched: None,
            link_transform: None,
            native_ref: None,
        },
        Occurrence {
            id: child.clone(),
            prototype: cadmpeg_ir::products::PrototypeReference::Local {
                definition: child_product,
            },
            parent: OccurrenceParent::Occurrence { occurrence: root },
            ordinal: 0,
            transform: Transform {
                rows: [
                    [1.0, 0.0, 0.0, 25.0],
                    [0.0, 1.0, 0.0, 0.0],
                    [0.0, 0.0, 1.0, 0.0],
                    [0.0, 0.0, 0.0, 1.0],
                ],
            },
            prototype_transform: Transform::identity(),
            scale: [1.0; 3],
            name: Some("Child occurrence".into()),
            linked_subelements: Vec::new(),
            visible: None,
            element_component: None,
            claim_child: None,
            copy_on_change: None,
            copy_on_change_source: None,
            copy_on_change_group: None,
            copy_on_change_touched: None,
            link_transform: None,
            native_ref: None,
        },
    ]);
    let annotation = PmiId("test:pmi#note".into());
    ir.model.pmi.push(PmiAnnotation {
        id: annotation.clone(),
        name: Some("inspection note".into()),
        targets: Vec::new(),
        definition: PmiDefinition::Presentation {
            text: Some("inspect this assembly".into()),
            placement: Some(Transform::identity()),
            semantics: Vec::new(),
        },
    });
    ir.model.presentation_layers.push(PresentationLayer {
        id: LayerId("test:layer#mixed".into()),
        name: "mixed layer".into(),
        description: None,
        items: vec![
            PresentationItem::Product {
                product: parent_product,
            },
            PresentationItem::Occurrence { occurrence: child },
            PresentationItem::Pmi { annotation },
        ],
    });

    let mut bytes = Vec::new();
    let report = write_step(
        &ir,
        &mut bytes,
        &StepWriteOptions {
            schema: StepSchema::Ap242Edition3,
            unsupported: StepUnsupportedPolicy::Reject,
            ..StepWriteOptions::default()
        },
    )
    .expect("write mixed presentation layer");
    assert!(!report.losses.iter().any(|loss| {
        loss.message
            .contains("layer 'mixed layer' has 3 item(s) without a writable STEP carrier")
    }));

    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode mixed presentation layer");
    let layer = decoded
        .ir()
        .model
        .presentation_layers
        .iter()
        .find(|layer| layer.name == "mixed layer")
        .expect("mixed presentation layer");
    assert_eq!(layer.items.len(), 3);
    assert!(matches!(layer.items[0], PresentationItem::Product { .. }));
    assert!(matches!(
        layer.items[1],
        PresentationItem::Occurrence { .. }
    ));
    assert!(matches!(layer.items[2], PresentationItem::Pmi { .. }));
}

/// The soccer-ball case: a body carries a base color and one face overrides it.
/// Every face must be styled (body color pushed down onto the faces that do not
/// override it), and the overriding face must carry its own color.
#[test]
pub(crate) fn face_override_wins_over_body_color_and_body_fills_the_rest() {
    use cadmpeg_ir::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
    use cadmpeg_ir::ids::AppearanceId;

    let mut ir = unit_cube();
    let face_count = ir.model.faces.len();
    // White body base color.
    ir.model.bodies[0].color = Some(cadmpeg_ir::topology::Color {
        r: 1.0,
        g: 1.0,
        b: 1.0,
        a: 1.0,
    });
    // Black override on a single face, via an appearance binding.
    let face = ir.model.faces[0].id.clone();
    ir.model.appearances.push(Appearance {
        id: AppearanceId("test:appearance#black".to_string()),
        name: None,
        asset_guid: None,
        library_id: None,
        visual_guid: None,
        physical_token: None,
        schema: None,
        category: None,
        base_color: Some(cadmpeg_ir::topology::Color {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        }),
        properties: std::collections::BTreeMap::default(),
        textures: Vec::new(),
    });
    ir.model.appearance_bindings.push(AppearanceBinding {
        id: "test:appearance-binding#face".to_string(),
        target: AppearanceTarget::Face(face),
        appearance: AppearanceId("test:appearance#black".to_string()),
        source_entity_id: None,
        object_type: None,
        channels: std::collections::BTreeMap::default(),
    });

    let s = export(&ir);
    // Both colors are present, and every face is styled.
    assert!(s.contains("COLOUR_RGB('',1.,1.,1.)"));
    assert!(s.contains("COLOUR_RGB('',0.,0.,0.)"));
    let styled: Vec<&str> = s.lines().filter(|l| l.contains("STYLED_ITEM")).collect();
    assert_eq!(styled.len(), face_count);
    // Each color's style chain is emitted once and shared; grouping the styled
    // items by their style ref must yield exactly two groups sized 1 and
    // face_count - 1 (the lone override plus every inherited face).
    let mut per_style = std::collections::BTreeMap::<String, usize>::default();
    for item in &styled {
        // STYLED_ITEM('color',(#psa),#face)
        let psa = item
            .split_once(",(")
            .and_then(|(_, tail)| tail.split(')').next())
            .unwrap()
            .to_string();
        *per_style.entry(psa).or_default() += 1;
    }
    let mut counts: Vec<usize> = per_style.values().copied().collect();
    counts.sort_unstable();
    assert_eq!(counts, vec![1, face_count - 1]);
}
