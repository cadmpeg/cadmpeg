// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]
#![allow(unused_imports)]
use super::{resolve_uri, ReferenceTarget, ROOT_NAME};

#[test]
fn resolves_archive_relative_uris_and_fragments() {
    assert_eq!(
        resolve_uri(ROOT_NAME, "parts/child.p21#target").unwrap(),
        ReferenceTarget::Internal {
            member: "parts/child.p21".into(),
            query: None,
            fragment: Some("target".into()),
        }
    );
    assert_eq!(
        resolve_uri("parts/child.p21", "../shared.p21#value").unwrap(),
        ReferenceTarget::Internal {
            member: "shared.p21".into(),
            query: None,
            fragment: Some("value".into()),
        }
    );
    assert_eq!(
        resolve_uri(ROOT_NAME, "https://example.invalid/part.p21#root").unwrap(),
        ReferenceTarget::External
    );
    assert_eq!(
        resolve_uri("parts/sub/child.p21", "./../shared.p21#value").unwrap(),
        ReferenceTarget::Internal {
            member: "parts/shared.p21".into(),
            query: None,
            fragment: Some("value".into()),
        }
    );
    assert_eq!(
        resolve_uri(ROOT_NAME, "parts/child.p21?query=../outside#target").unwrap(),
        ReferenceTarget::Internal {
            member: "parts/child.p21".into(),
            query: Some("query=../outside".into()),
            fragment: Some("target".into()),
        }
    );
}

#[test]
fn rejects_archive_relative_traversal() {
    assert!(resolve_uri(ROOT_NAME, "../outside.p21").is_err());
    assert!(resolve_uri(ROOT_NAME, "parts//child.p21").is_err());
}

use std::fmt::Write as _;
use std::io::Cursor;

use cadmpeg_core::decode::{DecodeMode, InspectOptions};
use cadmpeg_ir::codec::{Codec, CodecBackend, Confidence, DecodeOptions};
use cadmpeg_ir::eval::{
    model_curve_point_by_id, model_surface_partials_by_id, model_surface_point_by_id, pcurve_uv,
};
use cadmpeg_ir::examples::unit_cube;
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, NurbsCurve, NurbsSurface, PcurveGeometry, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{CurveId, ProceduralCurveId, SurfaceId};
use cadmpeg_ir::index::ModelIndex;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::units::{LengthUnit, Units};
use cadmpeg_ir::CadIr;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::ids::StepIdentity;
use crate::test_support::{decode_inline, export};
use crate::{
    write_step, StepCodec, StepError, StepSchema, StepUnsupportedPolicy, StepWriteOptions,
};

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
pub(crate) fn codec_detects_and_inspects_ap242_exchange_structure() {
    let bytes = include_bytes!("../../tests/fixtures/ap242_minimal.p21");
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
    let root = include_bytes!("../../tests/fixtures/ap242_minimal.p21");
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
    let root = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('zip references'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;REFERENCE;#10=<parts/child.p21?query=../outside#target>;ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
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
        .any(|note| note == "internal resource #10 -> parts/child.p21?query=../outside#target"));

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
fn valid_external_resource_pair_keeps_target_anchor_and_root_graph_separate() {
    let root = include_bytes!("tests/data/er03_root_valid.p21");
    let subsidiary = include_bytes!("tests/data/er03_subsidiary_valid.p21");
    let (root_exchange, root_diagnostics) = crate::parse::parse(root).expect("parse valid root");
    assert!(root_diagnostics.is_empty());
    assert!(root_exchange
        .header
        .iter()
        .any(|record| record.name == "SCHEMA_POPULATION"));
    assert_eq!(
        root_exchange.references[0].uri,
        "parts/er03_subsidiary_valid.p21#remote_item"
    );
    assert_eq!(
        root_exchange.records[&1].partials[0].parameters,
        vec![crate::parse::Value::Reference(10)]
    );

    let (subsidiary_exchange, subsidiary_diagnostics) =
        crate::parse::parse(subsidiary).expect("parse valid subsidiary");
    assert!(subsidiary_diagnostics.is_empty());
    assert_eq!(
        subsidiary_exchange.anchors[0].value,
        crate::parse::Value::Reference(1)
    );
    assert_eq!(
        subsidiary_exchange.records[&1].partials[0].parameters,
        vec![crate::parse::Value::String(b"remote".to_vec())]
    );

    let bytes = step_zip(&[
        (ROOT_NAME, root, CompressionMethod::Stored),
        (
            "parts/er03_subsidiary_valid.p21",
            subsidiary,
            CompressionMethod::Stored,
        ),
    ]);
    let codec = StepCodec::default();
    let summary = codec
        .inspect(&mut Cursor::new(&bytes), &InspectOptions::default())
        .expect("inspect valid resource pair");
    assert_eq!(summary.entries.len(), 2);
    assert!(summary
        .notes
        .contains(&"internal resource #10 -> parts/er03_subsidiary_valid.p21#remote_item".into()));

    let result = codec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .expect("decode root without composing subsidiary");
    let source = result.ir().source.as_ref().expect("STEP source metadata");
    assert_eq!(source.attributes["entity_instances"], "1");
    assert_eq!(
        result
            .ir()
            .native_unknowns("step")
            .expect("STEP unknown arena")
            .len(),
        1
    );
    assert!(result
        .report()
        .notes
        .contains(&"internal resource #10 -> parts/er03_subsidiary_valid.p21#remote_item".into()));
    assert!(result
        .report()
        .notes
        .contains(&"external reference #10 -> parts/er03_subsidiary_valid.p21#remote_item".into()));
}

#[test]
fn zip_subsidiary_instance_names_do_not_enter_root_graph() {
    let root = include_bytes!("tests/data/ce02_zip_root.p21");
    let subsidiary = include_bytes!("tests/data/ce02_zip_subsidiary.p21");
    let (root_exchange, root_diagnostics) = crate::parse::parse(root).expect("parse CE-02 root");
    assert!(root_diagnostics.is_empty());
    assert_eq!(
        root_exchange.anchors[0].value,
        crate::parse::Value::Reference(10)
    );
    assert_eq!(
        root_exchange.records[&1].partials[0].parameters,
        vec![crate::parse::Value::Reference(10)]
    );

    let (subsidiary_exchange, subsidiary_diagnostics) =
        crate::parse::parse(subsidiary).expect("parse CE-02 subsidiary");
    assert!(subsidiary_diagnostics.is_empty());
    assert_eq!(
        subsidiary_exchange.anchors[0].value,
        crate::parse::Value::Reference(1)
    );
    assert_eq!(
        subsidiary_exchange.records[&1].partials[0].parameters,
        vec![crate::parse::Value::String(b"subsidiary".to_vec())]
    );

    let bytes = step_zip(&[
        (ROOT_NAME, root, CompressionMethod::Stored),
        (
            "parts/ce02_zip_subsidiary.p21",
            subsidiary,
            CompressionMethod::Stored,
        ),
    ]);
    let codec = StepCodec::default();
    let summary = codec
        .inspect(&mut Cursor::new(&bytes), &InspectOptions::default())
        .expect("inspect CE-02 ZIP witness");
    assert_eq!(summary.entries.len(), 2);
    assert!(summary
        .notes
        .contains(&"internal resource #10 -> parts/ce02_zip_subsidiary.p21#remote_item".into()));

    let result = codec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .expect("decode CE-02 ZIP root only");
    let source = result.ir().source.as_ref().expect("STEP source metadata");
    assert_eq!(source.attributes["archive_root"], ROOT_NAME);
    assert_eq!(source.attributes["entity_instances"], "1");
    let unknowns = result
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown arena");
    assert_eq!(unknowns.len(), 1);
    assert_eq!(unknowns[0].id.0, "step:data:item#1");
    assert!(result
        .report()
        .notes
        .contains(&"external reference #10 -> parts/ce02_zip_subsidiary.p21#remote_item".into()));
}

#[test]
fn codec_rejects_step_zip_without_root_or_with_unsupported_layout() {
    let root = include_bytes!("../../tests/fixtures/ap242_minimal.p21");
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
    let bytes = include_bytes!("../../tests/fixtures/ap242_ed3_sections.p21");
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
