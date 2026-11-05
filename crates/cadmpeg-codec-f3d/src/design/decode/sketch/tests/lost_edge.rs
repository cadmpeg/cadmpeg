// SPDX-License-Identifier: Apache-2.0

use crate::design::decode::sketch::decode_lost_edge_references_from_stream;

fn lost_edge_bytes() -> Vec<u8> {
    let marker = b"EDGE_REFERENCE_LOST";
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"281");
    bytes.extend_from_slice(&41u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 14]);
    bytes.extend_from_slice(
        &(u32::try_from(marker.len()).expect("fixture value fits u32")).to_le_bytes(),
    );
    bytes.extend_from_slice(marker);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"282");
    bytes.extend_from_slice(&42u32.to_le_bytes());
    bytes
}

#[test]
fn damaged_lost_edge_record_keeps_a_sibling() {
    use cadmpeg_ir::source_fidelity::parts::PartOutput;
    use std::io::{Cursor, Write};

    for truncated in [false, true] {
        let mut first = lost_edge_bytes();
        let mut second = lost_edge_bytes();
        second[7..11].copy_from_slice(&43u32.to_le_bytes());
        second[55..59].copy_from_slice(&44u32.to_le_bytes());
        if truncated {
            drop(first.drain(40..48));
        } else {
            first[25..29].copy_from_slice(&0u32.to_le_bytes());
        }
        first.extend_from_slice(&second);
        let meta = crate::test_support::zip_test::indexed_fixture_metastream(
            &first,
            &[41, 42, 43, 44],
            &[],
        );
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let stored = crate::zip_write::file_options(zip::CompressionMethod::Stored);
        crate::test_support::manifest_test::write_synthetic_manifests(&mut zip, stored);
        zip.start_file("FusionAssetName[Active]/Design1/BulkStream.dat", stored)
            .unwrap();
        zip.write_all(&first).unwrap();
        zip.start_file("FusionAssetName[Active]/Design1/MetaStream.dat", stored)
            .unwrap();
        zip.write_all(&meta).unwrap();
        let bytes = zip.finish().unwrap().into_inner();
        crate::test_support::zip_test::with_scan(&bytes, |scan| {
            let ctx = cadmpeg_test_support::service_decode_context();
            let mut parts = PartOutput::new(cadmpeg_ir::CodecFormat::F3d, &ctx);
            let references =
                super::super::decode_lost_edge_references(&ctx, &mut parts, scan).unwrap();
            assert_eq!(references.len(), 1);
            assert_eq!(
                (references[0].record_index, references[0].next_record_index),
                (43, 44)
            );
            let (_, native, fidelity, losses) = parts.into_parts();
            assert_eq!(losses.len(), 1);
            assert_eq!(
                losses[0].code,
                cadmpeg_ir::codec::DecodeLossCode::RecordUnreadable
                    .note(String::new())
                    .code
            );
            assert!(losses[0].message.contains("lost-edge reference 41"));
            assert_eq!(fidelity.retained_records().len(), 1);
            assert_eq!(
                native.namespace("f3d").unwrap().arenas()["unknowns"].len(),
                1
            );
        });
    }
}

#[test]
fn lost_edge_reference_scan_preserves_record_fields() {
    let bytes = lost_edge_bytes();
    let mut references = Vec::new();
    let frames = [
        crate::metastream::PrimaryRecordFrame {
            entity_id: 41,
            start: 0,
            member_end: 48,
            end: 48,
        },
        crate::metastream::PrimaryRecordFrame {
            entity_id: 42,
            start: 48,
            member_end: bytes.len(),
            end: bytes.len(),
        },
    ];
    decode_lost_edge_references_from_stream("BulkStream.dat", &bytes, &frames, &mut references);
    assert_eq!(references.len(), 1);
    assert_eq!(
        references[0].id,
        crate::ids::native_lost_edge_reference_id("BulkStream.dat", 0)
    );
    assert_eq!(references[0].record_index, 41);
    assert_eq!(references[0].next_record_index, 42);
}

#[test]
fn header_shaped_payload_does_not_select_a_lost_edge() {
    let mut bytes = lost_edge_bytes();
    let second = bytes.len();
    bytes.extend_from_slice(&3_u32.to_le_bytes());
    bytes.extend_from_slice(b"282");
    bytes.extend_from_slice(&43_u32.to_le_bytes());
    let frames = [
        crate::metastream::PrimaryRecordFrame {
            entity_id: 41,
            start: 0,
            member_end: second,
            end: second,
        },
        crate::metastream::PrimaryRecordFrame {
            entity_id: 43,
            start: second,
            member_end: bytes.len(),
            end: bytes.len(),
        },
    ];
    let mut references = Vec::new();
    decode_lost_edge_references_from_stream("BulkStream.dat", &bytes, &frames, &mut references);
    assert!(references.is_empty());
}
