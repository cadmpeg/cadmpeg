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
            fragment: Some("target".into()),
        }
    );
    assert_eq!(
        resolve_uri("parts/child.p21", "../shared.p21#value").unwrap(),
        ReferenceTarget::Internal {
            member: "shared.p21".into(),
            fragment: Some("value".into()),
        }
    );
    assert_eq!(
        resolve_uri(ROOT_NAME, "https://example.invalid/part.p21#root").unwrap(),
        ReferenceTarget::External
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
use zip::{CompressionMethod, ZipArchive, ZipWriter};

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

fn duplicate_first_central_record(mut bytes: Vec<u8>) -> Vec<u8> {
    let end = bytes
        .windows(4)
        .rposition(|signature| signature == b"PK\x05\x06")
        .expect("ZIP end record");
    let central_start = u32::from_le_bytes(bytes[end + 16..end + 20].try_into().unwrap()) as usize;
    let name_len = u16::from_le_bytes(
        bytes[central_start + 28..central_start + 30]
            .try_into()
            .unwrap(),
    ) as usize;
    let extra_len = u16::from_le_bytes(
        bytes[central_start + 30..central_start + 32]
            .try_into()
            .unwrap(),
    ) as usize;
    let comment_len = u16::from_le_bytes(
        bytes[central_start + 32..central_start + 34]
            .try_into()
            .unwrap(),
    ) as usize;
    let record_len = 46 + name_len + extra_len + comment_len;
    let record = bytes[central_start..central_start + record_len].to_vec();
    let central_end = end;
    bytes.splice(central_end..central_end, record.iter().copied());
    let new_end = end + record_len;
    let count = u16::from_le_bytes(bytes[new_end + 10..new_end + 12].try_into().unwrap());
    bytes[new_end + 8..new_end + 10].copy_from_slice(&(count + 1).to_le_bytes());
    bytes[new_end + 10..new_end + 12].copy_from_slice(&(count + 1).to_le_bytes());
    let size = u32::from_le_bytes(bytes[new_end + 12..new_end + 16].try_into().unwrap());
    bytes[new_end + 12..new_end + 16].copy_from_slice(&(size + record_len as u32).to_le_bytes());
    bytes
}

fn mark_entries_encrypted(mut bytes: Vec<u8>) -> Vec<u8> {
    let locations = {
        let mut archive = ZipArchive::new(Cursor::new(&bytes)).unwrap();
        (0..archive.len())
            .map(|index| {
                let file = archive.by_index(index).unwrap();
                (
                    file.header_start() as usize,
                    file.central_header_start() as usize,
                )
            })
            .collect::<Vec<_>>()
    };
    for (local, central) in locations {
        let local_flags = u16::from_le_bytes(bytes[local + 6..local + 8].try_into().unwrap()) | 1;
        bytes[local + 6..local + 8].copy_from_slice(&local_flags.to_le_bytes());
        let central_flags =
            u16::from_le_bytes(bytes[central + 8..central + 10].try_into().unwrap()) | 1;
        bytes[central + 8..central + 10].copy_from_slice(&central_flags.to_le_bytes());
    }
    bytes
}

fn corrupt_first_payload(mut bytes: Vec<u8>) -> Vec<u8> {
    let mut archive = ZipArchive::new(Cursor::new(&bytes)).unwrap();
    let data_start = archive.by_index(0).unwrap().data_start().unwrap() as usize;
    bytes[data_start] ^= 1;
    bytes
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
fn codec_checks_forwarded_root_references_without_decoding_the_subsidiary() {
    let root = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('zip forwarded reference'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;ANCHOR;<target>=<parts/child.p21#target>;ENDSEC;REFERENCE;#10=<#target>;ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
    let bytes = step_zip(&[
        ("ISO-10303.p21", root, CompressionMethod::Stored),
        (
            "parts/child.p21",
            b"not a STEP exchange structure",
            CompressionMethod::Stored,
        ),
    ]);
    let codec = StepCodec::default();

    let summary = codec
        .inspect(&mut Cursor::new(&bytes), &InspectOptions::default())
        .expect("inspect forwarded ZIP reference");
    assert!(summary
        .notes
        .iter()
        .any(|note| note == "internal resource #10 -> parts/child.p21#target"));

    let missing = step_zip(&[("ISO-10303.p21", root, CompressionMethod::Stored)]);
    assert!(matches!(
        codec.inspect(&mut Cursor::new(missing), &InspectOptions::default()),
        Err(cadmpeg_core::CodecError::Malformed(_))
    ));

    let result = codec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .expect("decode root without parsing subsidiary");
    assert!(result
        .report()
        .notes
        .iter()
        .any(|note| note == "internal resource #10 -> parts/child.p21#target"));
}

#[test]
fn codec_keeps_external_reference_graph_resource_local() {
    let root = include_bytes!("tests/data/er03_root.p21");
    let subsidiary = include_bytes!("tests/data/er03_subsidiary.p21");
    let bytes = step_zip(&[
        (ROOT_NAME, root, CompressionMethod::Stored),
        (
            "parts/er03_subsidiary.p21",
            subsidiary,
            CompressionMethod::Stored,
        ),
    ]);
    let codec = StepCodec::default();

    let summary = codec
        .inspect(&mut Cursor::new(&bytes), &InspectOptions::default())
        .expect("inspect resource-composition witness");
    assert_eq!(summary.entries.len(), 2);
    assert_eq!(summary.entries[0].name, ROOT_NAME);
    assert!(!summary.entries[1]
        .attributes
        .contains_key("logical_sections"));
    assert!(summary
        .notes
        .iter()
        .any(|note| note == "internal resource #10 -> parts/er03_subsidiary.p21#remote_item"));

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
        .contains(&"external reference #10 -> parts/er03_subsidiary.p21#remote_item".into()));
    assert!(result
        .report()
        .notes
        .contains(&"internal resource #10 -> parts/er03_subsidiary.p21#remote_item".into()));
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

    let unicode_name = step_zip(&[
        ("ISO-10303.p21", root, CompressionMethod::Stored),
        ("π-preview.bin", b"ancillary", CompressionMethod::Stored),
    ]);
    assert!(matches!(
        codec.inspect(&mut Cursor::new(unicode_name), &InspectOptions::default()),
        Err(cadmpeg_core::CodecError::Malformed(_))
    ));

    let duplicate_name = duplicate_first_central_record(step_zip(&[(
        "ISO-10303.p21",
        root,
        CompressionMethod::Stored,
    )]));
    assert!(matches!(
        codec.inspect(&mut Cursor::new(duplicate_name), &InspectOptions::default()),
        Err(cadmpeg_core::CodecError::Malformed(_))
    ));

    let encrypted = mark_entries_encrypted(step_zip(&[(
        "ISO-10303.p21",
        root,
        CompressionMethod::Stored,
    )]));
    assert!(matches!(
        codec.inspect(&mut Cursor::new(encrypted), &InspectOptions::default()),
        Err(cadmpeg_core::CodecError::Malformed(_))
    ));

    let checksum_mismatch = corrupt_first_payload(step_zip(&[(
        "ISO-10303.p21",
        root,
        CompressionMethod::Stored,
    )]));
    assert!(matches!(
        codec.inspect(
            &mut Cursor::new(checksum_mismatch),
            &InspectOptions::default()
        ),
        Err(cadmpeg_core::CodecError::Malformed(_))
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
