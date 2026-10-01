// SPDX-License-Identifier: Apache-2.0
//! `DisplayJT` expansion and element-framing resource tests.

use std::io::Write;

use flate2::write::ZlibEncoder;
use flate2::Compression;

use cadmpeg_ir::hash::digest::Sha256Digest;

#[test]
fn display_jt_inflate_propagates_expansion_and_retained_limits() {
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

                    crate::test_support::with_decode_context_over(
                        &compressed,
                        |policy| {
                            policy.limits.max_retained_bytes =
                                cadmpeg_core::decode::u64_from_index(expanded.len()) - 1
                                    + cadmpeg_test_support::decode::arena_registry_bytes();
                        },
                        |ctx| {
                            let root = cadmpeg_core::decode::View::over_retained(&compressed);

                            let error = super::super::inflate_display_jt(ctx, root).unwrap_err();
                            assert!(
                                matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "retain inflated DisplayJT payload")
                            );
                        },
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
        segment_id: [0; 16],
        segment_type: 7,
        segment_byte_len: u32::try_from(data.len()).unwrap(),
        payload_sha256: Sha256Digest::digest(&data[24..]),
        compression: Some(compression),
        source_offset: 0,
    };
    (data, segment)
}

fn assert_compressed_jt_limit(
    adjust: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
    dimension: cadmpeg_core::decode::ResourceDimension,
    operation: &'static str,
) {
    use crate::container::{Container, DirEntry, Region};

    let (data, segment) = compressed_jt_fixture();

    let container = Container {
        data: std::borrow::Cow::Borrowed(&data),
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
        |policy| {
            adjust(policy);
            if dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes {
                policy.limits.max_retained_bytes +=
                    cadmpeg_test_support::decode::arena_registry_bytes();
            } else if dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems {
                policy.limits.max_collection_items += 1;
            }
        },
        |ctx| {
            let root = cadmpeg_core::decode::View::over_retained(&data);

            let error = super::super::display_jt_compressed_element_sequences(
                (ctx, root),
                &container,
                std::slice::from_ref(&segment),
            )
            .unwrap_err();
            assert!(
                matches!(&error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == dimension && limit.operation == operation),
                "{error}"
            );

            crate::test_support::with_decode_context_over(
                &data,
                |_| {},
                |service| {
                    let root = cadmpeg_core::decode::View::over_retained(&data);

                    let (elements, sequences) =
                        super::super::display_jt_compressed_element_sequences(
                            (service, root),
                            &container,
                            &[segment],
                        )
                        .unwrap();
                    assert_eq!((elements.len(), sequences.len()), (1, 1));
                },
            );
        },
    );
}

#[test]
fn jt_element_ids_refuse_before_vector_reservation() {
    use cadmpeg_core::decode::ResourceDimension;
    let adjust_policy = |policy: &mut cadmpeg_core::decode::DecodePolicy| {
        policy.limits.max_collection_items = 1;
    };
    assert_compressed_jt_limit(
        adjust_policy,
        ResourceDimension::CollectionItems,
        "store DisplayJT element ids",
    );
}

#[test]
fn jt_compressed_elements_refuse_before_vector_reservation() {
    use cadmpeg_core::decode::ResourceDimension;
    let adjust_policy = |policy: &mut cadmpeg_core::decode::DecodePolicy| {
        policy.limits.max_collection_items = 2;
    };
    assert_compressed_jt_limit(
        adjust_policy,
        ResourceDimension::CollectionItems,
        "store DisplayJT compressed elements",
    );
}

#[test]
fn jt_compressed_sequence_refuses_before_vector_reservation() {
    use cadmpeg_core::decode::ResourceDimension;
    let adjust_policy = |policy: &mut cadmpeg_core::decode::DecodePolicy| {
        policy.limits.max_collection_items = 3;
    };
    assert_compressed_jt_limit(
        adjust_policy,
        ResourceDimension::CollectionItems,
        "store DisplayJT compressed sequence",
    );
}

#[test]
fn jt_compressed_element_fields_refuse_before_string_allocation() {
    use cadmpeg_core::decode::ResourceDimension;
    let adjust_policy = |policy: &mut cadmpeg_core::decode::DecodePolicy| {
        policy.limits.max_retained_bytes =
            cadmpeg_core::decode::u64_from_index(framed_jt_element().len())
                + 3
                + 2
                + 4 * cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
                    super::super::ParsedJtElement<'_>,
                >())
                + cadmpeg_core::decode::u64_from_index(std::mem::size_of::<String>())
                + cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
                    super::super::DisplayJtCompressedElement,
                >());
    };
    assert_compressed_jt_limit(
        adjust_policy,
        ResourceDimension::RetainedBytes,
        "retain DisplayJT compressed element fields",
    );
}

#[test]
fn jt_sequence_tail_hash_refuses_before_scoped_validation_allocation() {
    use cadmpeg_core::decode::ResourceDimension;
    let adjust_policy = |policy: &mut cadmpeg_core::decode::DecodePolicy| {
        policy.limits.max_materialized_bytes = 63;
    };
    assert_compressed_jt_limit(
        adjust_policy,
        ResourceDimension::MaterializedBytes,
        "check DisplayJT sequence tail hash",
    );
}

struct CompressedJtRetainedStages {
    ids: u64,
    elements: u64,
    sequence: u64,
    sequence_fields: u64,
    tail: u64,
}

fn compressed_jt_retained_stages() -> CompressedJtRetainedStages {
    let segment = "nx:jt:segment#0";
    let expanded_len = cadmpeg_core::decode::u64_from_index(framed_jt_element().len() + 3 + 2);
    let before_ids = expanded_len
        + 4 * cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
            super::super::ParsedJtElement<'_>,
        >());
    let before_elements =
        before_ids + cadmpeg_core::decode::u64_from_index(std::mem::size_of::<String>());
    let element_id_len = segment.len() + "-inflated-element-".len() + 1;
    let element_fields =
        cadmpeg_core::decode::u64_from_index(element_id_len * 2 + segment.len() + 64);
    let before_sequence = before_elements
        + cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
            super::super::DisplayJtCompressedElement,
        >())
        + element_fields;
    let before_sequence_fields = before_sequence
        + cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
            super::super::DisplayJtCompressedElementSequence,
        >());
    let before_tail = before_sequence_fields
        + cadmpeg_core::decode::u64_from_index(segment.len() * 2 + "-inflated-sequence".len());
    CompressedJtRetainedStages {
        ids: before_ids,
        elements: before_elements,
        sequence: before_sequence,
        sequence_fields: before_sequence_fields,
        tail: before_tail,
    }
}

#[test]
fn jt_element_ids_refuse_before_retained_vector_allocation() {
    use cadmpeg_core::decode::ResourceDimension;
    let adjust_policy = |policy: &mut cadmpeg_core::decode::DecodePolicy| {
        policy.limits.max_retained_bytes = compressed_jt_retained_stages().ids;
    };
    assert_compressed_jt_limit(
        adjust_policy,
        ResourceDimension::RetainedBytes,
        "retain DisplayJT element ids",
    );
}

#[test]
fn jt_compressed_elements_refuse_before_retained_vector_allocation() {
    use cadmpeg_core::decode::ResourceDimension;
    let adjust_policy = |policy: &mut cadmpeg_core::decode::DecodePolicy| {
        policy.limits.max_retained_bytes = compressed_jt_retained_stages().elements;
    };
    assert_compressed_jt_limit(
        adjust_policy,
        ResourceDimension::RetainedBytes,
        "retain DisplayJT compressed elements",
    );
}

#[test]
fn jt_compressed_sequence_refuses_before_retained_vector_allocation() {
    use cadmpeg_core::decode::ResourceDimension;
    let adjust_policy = |policy: &mut cadmpeg_core::decode::DecodePolicy| {
        policy.limits.max_retained_bytes = compressed_jt_retained_stages().sequence;
    };
    assert_compressed_jt_limit(
        adjust_policy,
        ResourceDimension::RetainedBytes,
        "retain DisplayJT compressed sequence",
    );
}

#[test]
fn jt_compressed_sequence_fields_refuse_before_string_allocation() {
    use cadmpeg_core::decode::ResourceDimension;
    let adjust_policy = |policy: &mut cadmpeg_core::decode::DecodePolicy| {
        policy.limits.max_retained_bytes = compressed_jt_retained_stages().sequence_fields;
    };
    assert_compressed_jt_limit(
        adjust_policy,
        ResourceDimension::RetainedBytes,
        "retain DisplayJT compressed sequence fields",
    );
}

#[test]
fn jt_compressed_sequence_tail_refuses_before_copy() {
    use cadmpeg_core::decode::ResourceDimension;
    let adjust_policy = |policy: &mut cadmpeg_core::decode::DecodePolicy| {
        policy.limits.max_retained_bytes = compressed_jt_retained_stages().tail;
    };
    assert_compressed_jt_limit(
        adjust_policy,
        ResourceDimension::RetainedBytes,
        "retain DisplayJT compressed sequence tail",
    );
}

#[test]
fn jt_compressed_element_hash_refuses_before_body_work() {
    use cadmpeg_core::decode::ResourceDimension;
    assert_compressed_jt_limit(
        |policy| policy.limits.max_work_units = 2,
        ResourceDimension::WorkUnits,
        "zlib compressed input",
    );
    let (data, _) = compressed_jt_fixture();
    // This member finishes in one 8192-byte expansion step and copies its whole output;
    // retaining the inflated payload then copies that output again.
    let expanded_len = cadmpeg_core::decode::u64_from_index(framed_jt_element().len() + 3 + 2);
    let expansion_work =
        cadmpeg_core::decode::u64_from_index(data.len() - 33) + 8192 + expanded_len;
    let adjust_policy = |policy: &mut cadmpeg_core::decode::DecodePolicy| {
        policy.limits.max_work_units = expansion_work + expanded_len + 2;
    };
    assert_compressed_jt_limit(
        adjust_policy,
        ResourceDimension::WorkUnits,
        "hash DisplayJT compressed element body",
    );
}

#[test]
fn jt_compressed_sequence_hash_refuses_before_tail_work() {
    use cadmpeg_core::decode::ResourceDimension;
    assert_compressed_jt_limit(
        |policy| policy.limits.max_work_units = 5,
        ResourceDimension::WorkUnits,
        "zlib compressed input",
    );
    let (data, _) = compressed_jt_fixture();
    // This member finishes in one 8192-byte expansion step and copies its whole output;
    // retaining the inflated payload then copies that output again.
    let expanded_len = cadmpeg_core::decode::u64_from_index(framed_jt_element().len() + 3 + 2);
    let expansion_work =
        cadmpeg_core::decode::u64_from_index(data.len() - 33) + 8192 + expanded_len;
    let adjust_policy = |policy: &mut cadmpeg_core::decode::DecodePolicy| {
        policy.limits.max_work_units = expansion_work + expanded_len + 5 + 64 + 2 + 2 + 64;
    };
    assert_compressed_jt_limit(
        adjust_policy,
        ResourceDimension::WorkUnits,
        "hash DisplayJT compressed sequence tail",
    );
}

#[test]
fn jt_compressed_sequence_validation_refuses_before_second_hash() {
    use cadmpeg_core::decode::ResourceDimension;
    assert_compressed_jt_limit(
        |policy| policy.limits.max_work_units = 7,
        ResourceDimension::WorkUnits,
        "zlib compressed input",
    );
    let (data, _) = compressed_jt_fixture();
    // This member finishes in one 8192-byte expansion step and copies its whole output;
    // retaining the inflated payload then copies that output again.
    let expanded_len = cadmpeg_core::decode::u64_from_index(framed_jt_element().len() + 3 + 2);
    let expansion_work =
        cadmpeg_core::decode::u64_from_index(data.len() - 33) + 8192 + expanded_len;
    let adjust_policy = |policy: &mut cadmpeg_core::decode::DecodePolicy| {
        policy.limits.max_work_units = expansion_work + expanded_len + 7 + 2 + 64;
    };
    assert_compressed_jt_limit(
        adjust_policy,
        ResourceDimension::WorkUnits,
        "check DisplayJT compressed sequence tail hash",
    );
}

#[test]
fn display_jt_element_index_refuses_before_collection_growth() {
    use cadmpeg_core::decode::ResourceDimension;

    let bytes = framed_jt_element();

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            policy.limits.max_collection_items = 0;
        },
        |ctx| {
            let error = super::super::parse_jt_element_sequence(ctx, &bytes).unwrap_err();
            assert!(
                matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "store DisplayJT element")
            );

            crate::test_support::with_decode_context(|service| {
                assert_eq!(
                    super::super::parse_jt_element_sequence(service, &bytes)
                        .unwrap()
                        .unwrap()
                        .0
                        .len(),
                    1
                );
            });
        },
    );
}

#[test]
fn display_jt_element_index_refuses_before_retained_reservation() {
    use cadmpeg_core::decode::ResourceDimension;

    let bytes = framed_jt_element();

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            policy.limits.max_retained_bytes =
                cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
                    super::super::ParsedJtElement<'_>,
                >()) - 1;
        },
        |ctx| {
            let error = super::super::parse_jt_element_sequence(ctx, &bytes).unwrap_err();
            assert!(
                matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "store DisplayJT element")
            );

            crate::test_support::with_decode_context(|service| {
                assert!(super::super::parse_jt_element_sequence(service, &bytes)
                    .unwrap()
                    .is_some());
            });
        },
    );
}

#[test]
fn display_jt_element_scan_refuses_before_framing_work() {
    use cadmpeg_core::decode::ResourceDimension;

    let bytes = framed_jt_element();

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            policy.limits.max_work_units = 0;
        },
        |ctx| {
            let error = super::super::parse_jt_element_sequence(ctx, &bytes).unwrap_err();
            assert!(
                matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits
            && limit.operation == "scan DisplayJT element")
            );

            crate::test_support::with_decode_context(|service| {
                assert!(super::super::parse_jt_element_sequence(service, &bytes)
                    .unwrap()
                    .is_some());
            });
        },
    );
}
