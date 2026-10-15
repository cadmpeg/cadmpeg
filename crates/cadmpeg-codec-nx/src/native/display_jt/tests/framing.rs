// SPDX-License-Identifier: Apache-2.0
//! `DisplayJT` expansion and element-framing tests.

use std::io::Write;

use flate2::write::ZlibEncoder;
use flate2::Compression;

use cadmpeg_ir::hash::digest::Sha256Digest;

#[test]
fn display_jt_inflate_propagates_expansion_limit() {
    use cadmpeg_core::decode::ResourceDimension;

    let expanded = [7_u8; 64];
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
    encoder.write_all(&expanded).unwrap();
    let compressed = encoder.finish().unwrap();

    crate::test_support::with_decode_context_over(
        &compressed,
        |_| {},
        |ctx| {
            let root = cadmpeg_core::decode::View::over_retained(&compressed);

            assert_eq!(
                super::super::inflate_display_jt(ctx, root)
                    .unwrap()
                    .unwrap(),
                expanded
            );

            crate::test_support::with_decode_context_over(
                &compressed,
                |policy| {
                    policy.limits.max_decompressed_bytes_per_expand =
                        cadmpeg_core::decode::u64_from_index(expanded.len()) - 1;
                },
                |ctx| {
                    let root = cadmpeg_core::decode::View::over_retained(&compressed);

                    let error = super::super::inflate_display_jt(ctx, root).unwrap_err();
                    assert!(
                        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::DecompressedBytes)
                    );
                },
            );
        },
    );
}

fn framed_jt_element() -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&21_u32.to_le_bytes());
    bytes.extend_from_slice(&[7; 16]);
    bytes.push(4);
    bytes.extend_from_slice(&9_u32.to_le_bytes());
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&[0xff; 16]);
    bytes
}

fn compressed_jt_fixture() -> (Vec<u8>, super::super::DisplayJtSegment) {
    let mut expanded = framed_jt_element();
    expanded[..4].copy_from_slice(&24_u32.to_le_bytes());
    expanded.splice(25..25, [9, 8, 7]);
    expanded.extend_from_slice(&[6, 5]);
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
    encoder.write_all(&expanded).unwrap();
    let compressed = encoder.finish().unwrap();
    let mut data = vec![0; 33];
    data.extend_from_slice(&compressed);
    let compression: super::super::DisplayJtCompression =
        serde_json::from_value(serde_json::json!({
            "flag": 2,
            "compressed_data_byte_len": compressed.len() + 1,
            "algorithm": 2,
            "compressed_byte_len": compressed.len(),
            "inflated_sha256": cadmpeg_ir::hash::sha256_hex(&expanded)
        }))
        .unwrap();
    let segment = super::super::DisplayJtSegment {
        id: "nx:jt:segment#0".into(),
        document: "nx:jt:document#0".into(),
        toc_entry: "nx:jt:toc-entry#0".into(),
        segment_id: [0; 16].into(),
        segment_type: 7,
        segment_byte_len: u32::try_from(data.len()).unwrap(),
        payload_sha256: Sha256Digest::digest(&data[24..]),
        compression: Some(compression),
        source_offset: 0,
    };
    (data, segment)
}

#[test]
fn jt_compressed_sequence_decodes_one_element_and_sequence() {
    use crate::container::{Container, DirEntry, Region};

    let (data, segment) = compressed_jt_fixture();
    let container = Container {
        data: data.as_slice().into(),
        physical_size: cadmpeg_core::decode::u64_from_index(data.len()),
        layout: crate::container::test_modern_layout(6),
        entries: vec![DirEntry {
            name: "/Root/UG_PART/DisplayJT".into(),
            region: Region::Footer,
            body: crate::container::DirEntryBody::File {
                offset: 0,
                len: cadmpeg_core::decode::u64_from_index(data.len()),
            },
        }],
        fastload_table: None,
        indexed_section_layouts: std::sync::OnceLock::new(),
        om_section_cache: std::sync::OnceLock::new(),
    };
    crate::test_support::with_decode_context_over(
        &data,
        |_| {},
        |ctx| {
            let (elements, sequences) =
                super::super::display_jt_compressed_element_sequences(ctx, &container, &[segment])
                    .unwrap();
            assert_eq!((elements.len(), sequences.len()), (1, 1));
        },
    );
}

#[test]
fn display_jt_element_sequence_reads_one_framed_element() {
    let bytes = framed_jt_element();
    assert_eq!(
        super::super::parse_jt_element_sequence(&bytes)
            .unwrap()
            .0
            .len(),
        1
    );
}
