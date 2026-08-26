// SPDX-License-Identifier: Apache-2.0
//! Container parser tests over synthetic CATPart streams.

#![allow(clippy::doc_markdown, clippy::unwrap_used)]

use super::*;
use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, CodecBackend, Confidence};

use crate::test_support::{
    append_e5_record, external_reference_segment, finjpl_stream, outer_body_catpart,
    outer_directory_catpart, standard_catpart, summary_preview_segment,
};
use crate::variant::Variant;
use crate::CatiaCodec;

fn append_e5_test_record(bytes: &mut Vec<u8>, id: u32) {
    append_e5_test_record_with_payload(bytes, id, &[]);
}

fn append_e5_test_record_with_payload(bytes: &mut Vec<u8>, id: u32, payload: &[u8]) {
    bytes.extend_from_slice(super::E5_MARKER);
    bytes.extend_from_slice(&[0xfe, 0x00]);
    bytes.extend_from_slice(&(payload.len() as u16).to_le_bytes());
    bytes.extend_from_slice(&[0x00, 0x00]);
    bytes.extend_from_slice(&id.to_le_bytes());
    bytes.extend_from_slice(payload);
}

fn outer_with_preamble(body: &[u8]) -> Vec<u8> {
    let directory_length = 32usize;
    let directory_offset = 512usize;
    let mut bytes = vec![0u8; directory_length];
    bytes[..super::OUTER_MAGIC.len()].copy_from_slice(super::OUTER_MAGIC);
    bytes[8..12].copy_from_slice(&(directory_offset as u32).to_be_bytes());
    bytes[12..16].copy_from_slice(&(directory_length as u32).to_be_bytes());
    bytes.extend_from_slice(body);
    bytes.resize(directory_offset + directory_length, 0);
    bytes
}

fn test_descriptor(name: &str, physical_offset: u32, length: u32) -> Descriptor {
    Descriptor {
        name: name.to_string(),
        desc_offset: 0,
        logical_length: length,
        extents: vec![Extent {
            phys_off: physical_offset,
            phys_len: length,
            flags: 0,
        }],
    }
}

fn fbb_only_tables_with_shared_delimiter() -> Vec<u8> {
    let mut bytes = vec![0x30, 0x04, 0x04, 0xff, 0xd2, 0xd2, 0xd2, 0xd2];
    for (kind, handles) in [(1u8, [1u8, 2]), (2, [2, 3])] {
        bytes.extend_from_slice(&[0x01, kind, 0x01, 0x02, 0x02]);
        bytes.extend_from_slice(&handles);
        bytes.extend_from_slice(super::EDGE_DELIMITER.as_slice());
    }
    bytes.extend_from_slice(&[0x01, 0x06, 0x00]);
    bytes
}

#[test]
fn nested_fbb_spine_precedes_a_coherent_e5_stream() {
    let scan = scan_bytes(standard_catpart());
    assert_eq!(
        identify_variant(
            scan.inner.as_ref(),
            scan.brep.as_deref(),
            scan.main_data_stream.as_deref(),
            &scan.census,
            true,
        ),
        Variant::StandardNested
    );
}

#[test]
fn coherent_e5_stream_overrides_zero_entity_markers() {
    let census = Census {
        a9_records: 1,
        ..Census::default()
    };
    assert_eq!(
        identify_variant(None, None, None, &census, true),
        Variant::E5Stream
    );
}

#[test]
fn scan_selects_a_coherent_e5_walk_over_a_zero_entity_record() {
    let mut body = vec![0xa9, 0x03, 0x10, 0x00, 0, 0, 0, 0, 0, 0, 0, 0];
    for id in 0..10 {
        append_e5_test_record(&mut body, id);
    }
    let scan = scan_bytes(outer_with_preamble(&body));
    assert_eq!(scan.census.a9_records, 1);
    assert_eq!(scan.variant, Variant::E5Stream);
}

#[test]
fn coherent_e5_stream_overrides_an_inner_body_without_brep_streams() {
    let inner = InnerDir {
        inner: 0,
        descriptors: Vec::new(),
    };
    assert_eq!(
        identify_variant(Some(&inner), None, None, &Census::default(), true),
        Variant::E5Stream
    );
}

#[test]
fn fbb_only_grammar_wins_when_its_delimiter_is_shared_with_standard() {
    let inner = InnerDir {
        inner: 0,
        descriptors: Vec::new(),
    };
    let brep = fbb_only_tables_with_shared_delimiter();
    let census = Census {
        fbb_runs: 1,
        edge_delimiters: 2,
        ..Census::default()
    };

    assert_eq!(
        crate::families::standard::fbb::standard_edge_count(&brep),
        None
    );
    assert_eq!(
        crate::families::standard::fbb::fbb_only_edge_count(&brep),
        Some(2)
    );
    assert_eq!(
        identify_variant(Some(&inner), Some(&brep), Some(&brep), &census, false),
        Variant::FbbOnly
    );
}

#[test]
fn unadmitted_fbb_region_is_unknown_even_with_delimiter_markers() {
    let inner = InnerDir {
        inner: 0,
        descriptors: Vec::new(),
    };
    let mut brep = vec![0x30, 0x04, 0x04, 0xff, 0, 0, 0, 0];
    brep.extend_from_slice(EDGE_DELIMITER.as_slice());
    let census = Census {
        fbb_runs: 1,
        edge_delimiters: 1,
        ..Census::default()
    };

    assert_eq!(
        crate::families::standard::fbb::standard_edge_count(&brep),
        None
    );
    assert_eq!(
        crate::families::standard::fbb::fbb_only_edge_count(&brep),
        None
    );
    assert_eq!(
        identify_variant(Some(&inner), Some(&brep), Some(&brep), &census, false),
        Variant::Unknown
    );

    let no_vertex_brep = vec![0x30, 0x04, 0x04, 0xff, 0, 0, 0, 0];
    let no_vertex_census = Census {
        fbb_runs: 1,
        ..Census::default()
    };
    assert_eq!(
        identify_variant(
            Some(&inner),
            Some(&no_vertex_brep),
            Some(&no_vertex_brep),
            &no_vertex_census,
            false,
        ),
        Variant::Unknown
    );
}

#[test]
fn coherent_e5_stream_precedes_a_partial_fbb_spine() {
    let inner = InnerDir {
        inner: 0,
        descriptors: Vec::new(),
    };
    let brep = fbb_only_tables_with_shared_delimiter();
    let census = Census {
        fbb_runs: 1,
        edge_delimiters: 2,
        ..Census::default()
    };

    assert_eq!(
        identify_variant(Some(&inner), Some(&brep), Some(&brep), &census, true),
        Variant::E5Stream
    );
}

#[test]
fn e5_stream_requires_declared_stride_or_coordinate_rows_between_records() {
    let mut body = Vec::new();
    for id in 0..10 {
        append_e5_test_record(&mut body, id);
        body.push(0x7f);
    }
    assert!(super::e5_record_stream(&outer_with_preamble(&body)).is_none());

    let mut body = Vec::new();
    for id in 0..10 {
        append_e5_test_record(&mut body, id);
        if id != 9 {
            body.extend_from_slice(&[0x05, 0x08, 0x01]);
            body.extend_from_slice(&[0; 12]);
        }
    }
    assert!(super::e5_record_stream(&outer_with_preamble(&body)).is_some());
}

#[test]
fn e5_stream_ignores_markers_inside_framed_payloads() {
    let mut false_record = Vec::new();
    append_e5_test_record(&mut false_record, 100);
    let mut body = Vec::new();
    append_e5_test_record_with_payload(&mut body, 0, &false_record);
    for id in 1..9 {
        append_e5_test_record(&mut body, id);
    }
    assert!(super::e5_record_stream(&outer_with_preamble(&body)).is_none());
}

#[test]
fn e5_stream_and_finjpl_inventory_exclude_the_trailing_directory() {
    let directory_length = 192usize;
    let directory_offset = 512usize;
    let mut bytes = vec![0u8; directory_length];
    bytes[..super::OUTER_MAGIC.len()].copy_from_slice(super::OUTER_MAGIC);
    bytes[8..12].copy_from_slice(&(directory_offset as u32).to_be_bytes());
    bytes[12..16].copy_from_slice(&(directory_length as u32).to_be_bytes());
    bytes.resize(directory_offset, 0);

    let mut directory = vec![0u8; super::DIR_MAGIC.len()];
    directory[..super::DIR_MAGIC.len()].copy_from_slice(super::DIR_MAGIC);
    directory.extend_from_slice(super::FINJPL_MARKER);
    directory.extend_from_slice(&0x0000_008eu32.to_be_bytes());
    for id in 0..10 {
        append_e5_test_record(&mut directory, id);
    }
    directory.resize(directory_length, 0);
    bytes.extend_from_slice(&directory);

    assert!(super::e5_record_stream(&bytes).is_none());
    let scan = super::scan_bytes(bytes);
    assert!(scan.finjpl_segments.is_empty());
    assert_eq!(scan.census.e5_markers, 0);
}

#[test]
fn all_e5_record_spans_cross_other_framed_records() {
    let mut body = Vec::new();
    append_e5_test_record(&mut body, 1);
    body.extend_from_slice(&[0xe5, 0x0d, 0x13, 0xf4, 0x01, 0x09, 0, 0, 0]);
    append_e5_test_record(&mut body, 2);
    assert_eq!(super::all_e5_record_spans(&body).len(), 2);
}

#[test]
fn equal_unpreferred_e5_segment_walks_are_ambiguous() {
    let mut body = Vec::new();
    for segment in 0..2 {
        body.extend_from_slice(super::FINJPL_MARKER);
        body.extend_from_slice(&0x0000_0080u32.to_be_bytes());
        for id in 0..10 {
            append_e5_test_record(&mut body, segment * 10 + id);
        }
    }
    assert!(super::e5_record_stream(&outer_with_preamble(&body)).is_none());
}

#[test]
fn brep_stream_requires_unique_canonical_descriptors() {
    let data = (0..32u8).collect::<Vec<_>>();
    let tied = InnerDir {
        inner: 0,
        descriptors: vec![
            test_descriptor("MainDataStream", 0, 4),
            test_descriptor("MainDataStream", 4, 4),
            test_descriptor("SurfacicReps", 8, 2),
        ],
    };
    assert!(super::brep_stream(&data, &tied).is_none());

    let noncanonical = InnerDir {
        inner: 0,
        descriptors: vec![
            test_descriptor("MainDataStream", 0, 4),
            test_descriptor("SurfacicRepsAlias", 4, 4),
        ],
    };
    assert!(super::brep_stream(&data, &noncanonical).is_none());

    let unique = InnerDir {
        inner: 0,
        descriptors: vec![
            test_descriptor("MainDataStream", 0, 4),
            test_descriptor("MainDataStream", 4, 5),
            test_descriptor("SurfacicReps", 9, 2),
        ],
    };
    assert_eq!(
        super::brep_stream(&data, &unique),
        Some(data[4..9].iter().chain(&data[9..11]).copied().collect())
    );
    assert_eq!(
        super::main_data_stream(&data, &unique),
        Some(data[4..9].to_vec())
    );
}

#[test]
fn extent_parser_retains_the_raw_flags_word() {
    let mut directory = vec![0; 24];
    for (offset, value) in [(4, 40u32), (8, 8), (12, 8), (16, 0), (20, 0xa501_0080)] {
        directory[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
    }
    let (extents, logical_length) =
        parse_extents(&directory, 0, 1, 0, 64).expect("complete extent");
    assert_eq!(logical_length, 8);
    assert_eq!(extents[0].flags, 0xa501_0080);
    assert!(parse_extents(&directory, 0, 1, usize::MAX, usize::MAX).is_none());
}

#[test]
fn descriptor_name_is_anchored_to_the_descriptor_tail() {
    let mut directory = vec![0u8; 0x80];
    let ds = 0x40;
    let name = b"MainDataStream";
    let name_start = ds - 3 - name.len() * 2;
    for (index, byte) in name.iter().enumerate() {
        directory[name_start + index * 2] = *byte;
    }
    directory[ds - 3..ds].copy_from_slice(&[0, 0, 0]);

    assert_eq!(super::descriptor_name(&directory, ds), "MainDataStream");
}

#[test]
fn descriptor_name_ignores_unrelated_utf16_runs_and_requires_the_tail() {
    let mut directory = vec![0u8; 0x80];
    for (index, byte) in b"UNRELATED_LONGER_RUN".iter().enumerate() {
        directory[8 + index * 2] = *byte;
    }
    let ds = 0x40;
    let name = b"Data";
    let name_start = ds - 3 - name.len() * 2;
    for (index, byte) in name.iter().enumerate() {
        directory[name_start + index * 2] = *byte;
    }
    directory[ds - 3..ds].copy_from_slice(&[0, 0, 1]);
    assert!(super::descriptor_name(&directory, ds).is_empty());

    directory[ds - 3..ds].copy_from_slice(&[0, 0, 0]);
    assert_eq!(super::descriptor_name(&directory, ds), "Data");
}

#[test]
fn descriptor_name_accepts_the_legacy_fixed_header_form() {
    let mut directory = vec![0u8; 0x80];
    let ds = 0x10;
    let name = b"RootStorage";
    let name_start = ds + 0x10;
    for (index, byte) in name.iter().enumerate() {
        directory[name_start + index * 2] = *byte;
    }
    directory[name_start + name.len() * 2..name_start + name.len() * 2 + 2]
        .copy_from_slice(&[0, 0]);

    assert_eq!(super::descriptor_name(&directory, ds), "RootStorage");
}

#[test]
fn directory_parser_accepts_a_structurally_bounded_extent_roster_above_64() {
    let descriptor_start = 16;
    let extent_count_offset = descriptor_start + 0x50;
    let extent_count = 65usize;
    let directory_end = extent_count_offset + 4 + extent_count * 20;
    let mut directory = vec![0u8; directory_end];
    directory[..super::DIR_MAGIC.len()].copy_from_slice(super::DIR_MAGIC);
    directory[descriptor_start + 0x0c..descriptor_start + 0x10]
        .copy_from_slice(&(extent_count as u32).to_be_bytes());
    directory[extent_count_offset..extent_count_offset + 4]
        .copy_from_slice(&(extent_count as u32).to_be_bytes());
    for index in 0..extent_count {
        let extent = extent_count_offset + 4 + index * 20;
        directory[extent..extent + 4].copy_from_slice(&(index as u32).to_be_bytes());
        directory[extent + 4..extent + 8].copy_from_slice(&1u32.to_be_bytes());
        directory[extent + 8..extent + 12].copy_from_slice(&1u32.to_be_bytes());
        directory[extent + 12..extent + 16].copy_from_slice(&(index as u32).to_be_bytes());
    }

    let parsed =
        parse_directory_region(&directory, 0, 0, directory.len()).expect("bounded extent roster");
    let descriptor = parsed
        .descriptors
        .iter()
        .find(|descriptor| descriptor.desc_offset == descriptor_start)
        .expect("descriptor at synthesized header");
    assert_eq!(descriptor.logical_length, extent_count as u32);
    assert_eq!(descriptor.extents.len(), extent_count);
}

#[test]
fn logical_stream_reconstruction_is_atomic_over_its_extent_roster() {
    let descriptor = Descriptor {
        name: "MAIN".to_string(),
        desc_offset: 0,
        logical_length: 4,
        extents: vec![
            Extent {
                phys_off: 1,
                phys_len: 2,
                flags: 0,
            },
            Extent {
                phys_off: 7,
                phys_len: 2,
                flags: 0,
            },
        ],
    };
    assert_eq!(
        reconstruct_logical_stream(b"0123456789", &descriptor, 0),
        b"1278"
    );

    let mut outside = descriptor.clone();
    outside.extents[1].phys_off = 9;
    assert!(reconstruct_logical_stream(b"0123456789", &outside, 0).is_empty());

    let mut wrong_length = descriptor.clone();
    wrong_length.logical_length = 3;
    assert!(reconstruct_logical_stream(b"0123456789", &wrong_length, 0).is_empty());
}

#[test]
fn logical_stream_reconstruction_rejects_overflowing_physical_offsets() {
    let descriptor = Descriptor {
        name: "MAIN".to_string(),
        desc_offset: 0,
        logical_length: 1,
        extents: vec![Extent {
            phys_off: 1,
            phys_len: 1,
            flags: 0,
        }],
    };
    assert!(reconstruct_logical_stream(&[0], &descriptor, usize::MAX).is_empty());
}

#[test]
fn container_summary_exposes_extent_flags_in_logical_order() {
    let scan = ContainerScan {
        data: Vec::new().into(),
        outer_dir_offset: 0,
        outer_dir_length: 0,
        outer: Some(InnerDir {
            inner: 0,
            descriptors: vec![Descriptor {
                name: "MAIN".to_string(),
                desc_offset: 16,
                logical_length: 12,
                extents: vec![
                    Extent {
                        phys_off: 40,
                        phys_len: 4,
                        flags: 0xa501_0080,
                    },
                    Extent {
                        phys_off: 80,
                        phys_len: 8,
                        flags: 0,
                    },
                ],
            }],
        }),
        inner: None,
        brep: None,
        main_data_stream: None,
        previews: Vec::new(),
        last_save_version: None,
        external_references: Vec::new(),
        finjpl_segments: Vec::new(),
        outer_container_declarations: Vec::new(),
        surface_alias_tags: std::collections::HashMap::new(),
        census: Census::default(),
        variant: Variant::Unknown,
    };
    let summary = summarize(&scan);
    assert_eq!(
        summary.entries[0].attributes["extent_flags"],
        "0xa5010080,0x00000000"
    );
}

#[test]
fn outer_data_declaration_assigns_class_to_its_uuid_stream() {
    let mut data = vec![0; 40];
    data[8..12].copy_from_slice(b"\x01\x00\x03\x00");
    data[12..16].copy_from_slice(&2u32.to_le_bytes());
    data[16..24].copy_from_slice(b"\x01\x00\x6c\x00\x02\x00\x00\x00");
    data[32..36].copy_from_slice(b"\x02\x00\x81\x20");
    data.extend_from_slice(b"CATPrtCont\0CATProdCont\0\0");
    data.extend_from_slice(b"\x03\x00\xf7\x00\x03\x00\x00\x00");
    data.extend_from_slice(&0x4bbc_295cu32.to_be_bytes());
    data.extend_from_slice(&0x0000_1048u32.to_be_bytes());
    data.extend_from_slice(&0x62eb_7b6fu32.to_be_bytes());
    data.extend_from_slice(&0x0000_1825u32.to_be_bytes());
    let data_len = u32::try_from(data.len()).expect("bounded declaration");
    let outer = InnerDir {
        inner: 0,
        descriptors: vec![
            Descriptor {
                name: "Data".to_string(),
                desc_offset: 10,
                logical_length: data_len,
                extents: vec![Extent {
                    phys_off: 0,
                    phys_len: data_len,
                    flags: 0,
                }],
            },
            Descriptor {
                name: "1048_62eb7b6f_1825".to_string(),
                desc_offset: 20,
                logical_length: 1,
                extents: vec![Extent {
                    phys_off: data_len,
                    phys_len: 1,
                    flags: 0,
                }],
            },
        ],
    };
    data.push(0);

    let declarations = outer_container_declarations(&data, &outer);

    assert_eq!(declarations.len(), 1);
    assert_eq!(declarations[0].data_offset, 0);
    assert_eq!(declarations[0].ordinal, 2);
    assert_eq!(declarations[0].class_name, "CATPrtCont");
    assert_eq!(declarations[0].base_class, "CATProdCont");
    assert_eq!(declarations[0].stream_name, "1048_62eb7b6f_1825");
    assert_eq!(
        outer_container_for_extent(&outer, &declarations, u64::from(data_len), 1)
            .map(|declaration| declaration.class_name.as_str()),
        Some("CATPrtCont")
    );
    assert!(
        outer_container_for_extent(&outer, &declarations, u64::from(data_len) - 1, 2).is_none()
    );

    let mut prefixed_outer = outer.clone();
    prefixed_outer.descriptors[1].name = "_1048_62eb7b6f_1825".to_string();
    let prefixed_declarations = outer_container_declarations(&data, &prefixed_outer);
    assert_eq!(prefixed_declarations.len(), 1);
    assert_eq!(prefixed_declarations[0].stream_name, "_1048_62eb7b6f_1825");
    assert_eq!(
        outer_container_for_extent(
            &prefixed_outer,
            &prefixed_declarations,
            u64::from(data_len),
            1
        )
        .map(|declaration| declaration.class_name.as_str()),
        Some("CATPrtCont")
    );

    let mut ambiguous_outer = prefixed_outer;
    ambiguous_outer.descriptors.push(Descriptor {
        name: "1048_62eb7b6f_1825".to_string(),
        desc_offset: 30,
        logical_length: 1,
        extents: vec![Extent {
            phys_off: data_len,
            phys_len: 1,
            flags: 0,
        }],
    });
    assert!(outer_container_declarations(&data, &ambiguous_outer).is_empty());

    let scan = ContainerScan {
        data: data.into(),
        outer_dir_offset: 0,
        outer_dir_length: 0,
        outer: Some(outer),
        inner: None,
        brep: None,
        main_data_stream: None,
        previews: Vec::new(),
        last_save_version: None,
        external_references: Vec::new(),
        finjpl_segments: Vec::new(),
        outer_container_declarations: declarations,
        surface_alias_tags: std::collections::HashMap::new(),
        census: Census::default(),
        variant: Variant::Unknown,
    };
    let summary = summarize(&scan);
    assert_eq!(
        summary.entries[1].attributes["container_class"],
        "CATPrtCont"
    );
    assert_eq!(
        summary.entries[1].attributes["container_base_class"],
        "CATProdCont"
    );
    assert_eq!(summary.entries[1].attributes["container_ordinal"], "2");
    assert_eq!(summary.entries[1].attributes["container_data_offset"], "0");
}

#[test]
fn outer_data_declaration_uses_the_terminal_marker_after_long_class_names() {
    let long_class = "C".repeat(193);
    let mut data = vec![0; 40];
    data[8..12].copy_from_slice(b"\x01\x00\x03\x00");
    data[12..16].copy_from_slice(&2u32.to_le_bytes());
    data[16..24].copy_from_slice(b"\x01\x00\x6c\x00\x02\x00\x00\x00");
    data[32..36].copy_from_slice(b"\x02\x00\x81\x20");
    data.extend_from_slice(long_class.as_bytes());
    data.extend_from_slice(b"\0CATProdCont\0\0");
    data.extend_from_slice(b"\x03\x00\xf7\x00\x03\x00\x00\x00");
    data.extend_from_slice(&0x4bbc_295cu32.to_be_bytes());
    data.extend_from_slice(&0x0000_1048u32.to_be_bytes());
    data.extend_from_slice(&0x62eb_7b6fu32.to_be_bytes());
    data.extend_from_slice(&0x0000_1825u32.to_be_bytes());
    let data_len = u32::try_from(data.len()).expect("bounded declaration");
    let outer = InnerDir {
        inner: 0,
        descriptors: vec![
            Descriptor {
                name: "Data".to_string(),
                desc_offset: 10,
                logical_length: data_len,
                extents: vec![Extent {
                    phys_off: 0,
                    phys_len: data_len,
                    flags: 0,
                }],
            },
            Descriptor {
                name: "1048_62eb7b6f_1825".to_string(),
                desc_offset: 20,
                logical_length: 1,
                extents: vec![Extent {
                    phys_off: data_len,
                    phys_len: 1,
                    flags: 0,
                }],
            },
        ],
    };
    data.push(0);

    let declarations = outer_container_declarations(&data, &outer);

    assert_eq!(declarations.len(), 1);
    assert_eq!(declarations[0].class_name, long_class);
    assert_eq!(declarations[0].base_class, "CATProdCont");
}

#[test]
fn detect_high_on_outer_magic() {
    assert_eq!(CatiaCodec.detect(OUTER_MAGIC), Confidence::High);
    assert_eq!(CatiaCodec.detect(&standard_catpart()), Confidence::High);
    assert_eq!(CatiaCodec.detect(b"PK\x03\x04 not catia"), Confidence::No);
}

#[test]
fn summary_preview_parser_extracts_exact_jpeg_and_dimensions() {
    let bytes = summary_preview_segment();
    let segments = crate::container::finjpl_segments(&bytes, 0, bytes.len());
    assert_eq!(segments[0].name.as_deref(), Some("CATSummaryInformation"));
    let previews = crate::container::preview_images(&bytes);
    assert_eq!(previews.len(), 1);
    assert_eq!(previews[0].width, 640);
    assert_eq!(previews[0].height, 288);
    assert_eq!(previews[0].components, 1);
    assert_eq!(&bytes[previews[0].range.clone()][..2], [0xff, 0xd8]);
    assert_eq!(
        &bytes[previews[0].range.clone()][previews[0].range.len() - 2..],
        [0xff, 0xd9]
    );
    let summary =
        crate::container::summarize(&crate::container::scan_bytes(outer_body_catpart(&bytes)));
    assert!(summary.entries.iter().any(|entry| {
        entry.role == crate::container::role::FINJPL_SEGMENT
            && entry.name == "CATSummaryInformation"
    }));

    let mut truncated = bytes;
    let eoi = truncated
        .windows(2)
        .position(|value| value == [0xff, 0xd9])
        .unwrap();
    truncated.truncate(eoi + 1);
    assert!(crate::container::preview_images(&truncated).is_empty());
}

#[test]
fn summary_version_parser_requires_one_consistent_tuple() {
    let bytes = summary_preview_segment();
    let version = crate::container::last_save_version(&bytes).unwrap();
    assert_eq!(version.version, 5);
    assert_eq!(version.release, 27);
    assert_eq!(version.service_pack, 2);
    assert_eq!(version.hot_fix, 0);
    assert_eq!(version.build_date, "03-10-2017.22.00");

    let mut conflicting = bytes;
    let mut other = summary_preview_segment();
    let release = other
        .windows(11)
        .position(|value| value == b"<Release>27")
        .unwrap();
    other[release + 9] = b'2';
    other[release + 10] = b'8';
    conflicting.extend_from_slice(&other);
    assert!(crate::container::last_save_version(&conflicting).is_none());

    let mut non_summary = summary_preview_segment();
    non_summary[8..12].copy_from_slice(&0x0101_0002u32.to_be_bytes());
    assert!(crate::container::last_save_version(&non_summary).is_none());
    assert!(crate::container::preview_images(&non_summary).is_empty());
    let native = crate::native::CatiaNative::decode(&non_summary);
    assert!(native.preview_images.is_empty());
}

#[test]
fn storage_property_parser_enumerates_external_catia_documents() {
    let mut bytes = external_reference_segment("Support.CATPart");
    bytes.extend_from_slice(&external_reference_segment("Assembly.CATProduct"));
    bytes.extend_from_slice(&external_reference_segment("notes.txt"));
    let references = crate::container::external_references(&bytes);
    assert_eq!(references.len(), 2);
    assert_eq!(references[0].target, "Support.CATPart");
    assert_eq!(references[1].target, "Assembly.CATProduct");

    let scan = crate::container::scan_bytes(outer_body_catpart(&bytes));
    let summary = crate::container::summarize(&scan);
    assert_eq!(
        summary
            .entries
            .iter()
            .filter(|entry| entry.role == crate::container::role::EXTERNAL_REFERENCE)
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>(),
        ["Support.CATPart", "Assembly.CATProduct"]
    );

    let native = crate::native::CatiaNative::decode(&bytes);
    assert_eq!(native.version, crate::native::CATIA_NATIVE_VERSION);
    assert_eq!(native.external_references.len(), 2);
    assert_eq!(native.external_references[0].target, "Support.CATPart");
    assert_eq!(
        native.external_references[0].segment,
        native.finjpl_segments[0].id
    );
    assert_eq!(
        native.external_references[1].segment,
        native.finjpl_segments[1].id
    );
    for reference in &native.external_references {
        let segment = native
            .finjpl_segments
            .iter()
            .find(|segment| segment.id == reference.segment)
            .expect("external-reference segment");
        assert!(reference.byte_offset >= segment.byte_offset);
        assert!(reference.byte_offset < segment.byte_offset + segment.byte_len);
    }
}

#[test]
fn summary_preview_requires_a_coherent_frame_header() {
    let valid = summary_preview_segment();
    let frame = valid
        .windows(2)
        .position(|bytes| bytes == [0xff, 0xc0])
        .expect("fixture SOF marker");

    let mut zero_height = valid.clone();
    zero_height[frame + 5..frame + 7].copy_from_slice(&0u16.to_be_bytes());
    assert!(crate::container::preview_images(&zero_height).is_empty());

    let mut inconsistent_components = valid;
    inconsistent_components[frame + 9] = 2;
    assert!(crate::container::preview_images(&inconsistent_components).is_empty());
    assert!(crate::native::CatiaNative::decode(&inconsistent_components)
        .preview_images
        .is_empty());
}

#[test]
fn summary_preview_requires_one_complete_jpeg_candidate() {
    let valid = summary_preview_segment();
    let image_start = valid
        .windows(3)
        .position(|bytes| bytes == [0xff, 0xd8, 0xff])
        .expect("fixture JPEG SOI");

    let mut malformed_prefix = valid.clone();
    malformed_prefix.splice(image_start..image_start, [0xff, 0xd8, 0xff, 0xd9]);
    let previews = crate::container::preview_images(&malformed_prefix);
    let [preview] = previews.as_slice() else {
        panic!("one complete preview after malformed SOI")
    };
    assert_eq!(&malformed_prefix[preview.range.clone()][..2], [0xff, 0xd8]);

    let image_end = valid
        .windows(2)
        .enumerate()
        .skip(image_start)
        .find_map(|(at, bytes)| (bytes == [0xff, 0xd9]).then_some(at + 2))
        .expect("fixture JPEG EOI");
    let image = valid[image_start..image_end].to_vec();
    let mut duplicate = valid;
    duplicate.extend(image);
    assert!(crate::container::preview_images(&duplicate).is_empty());
}

#[test]
fn scan_parses_directory_and_identifies_standard() {
    let f = standard_catpart();
    let scan = crate::container::scan_bytes(f);
    assert_eq!(scan.variant, Variant::StandardNested);
    let dir = scan.inner.expect("inner directory");
    assert!(dir.descriptors.iter().any(|d| d.name == "MainDataStream"));
    assert!(dir.descriptors.iter().any(|d| d.name == "SurfacicReps"));
    let brep = scan.brep.expect("reconstructed brep stream");
    // The BREP stream is MainDataStream followed by SurfacicReps.
    assert!(brep.windows(3).any(|w| w == [0x05, 0x08, 0x01]));
    assert!(brep.windows(3).any(|w| w == [0x00, 0x33, 0x33]));
    assert_eq!(scan.census.fbb_runs, 1);
    assert_eq!(scan.census.fbb_face_rows, 2);
    assert!(scan.census.edge_delimiters >= 1);
    assert_eq!(scan.census.vertex_markers, 3);
}

#[test]
fn scan_parses_outer_directory_with_absolute_extents() {
    let bytes = outer_directory_catpart();
    let directory_offset =
        usize::try_from(u32::from_be_bytes(bytes[8..12].try_into().unwrap())).unwrap();
    assert_eq!(
        crate::container::outer_stream_directory_range(&bytes),
        Some(directory_offset..bytes.len())
    );
    let scan = crate::container::scan_bytes(bytes.clone());
    let outer = scan.outer.as_ref().expect("outer directory");
    assert_eq!(outer.inner, 0);
    assert_eq!(outer.descriptors.len(), 1);
    let descriptor = &outer.descriptors[0];
    assert_eq!(descriptor.name, "RootStorage");
    assert_eq!(
        crate::container::reconstruct_logical_stream(&bytes, descriptor, outer.inner),
        b"outer logical stream"
    );

    let summary = crate::container::summarize(&scan);
    let entry = summary
        .entries
        .iter()
        .find(|entry| entry.name == "RootStorage")
        .expect("outer stream summary");
    assert_eq!(entry.attributes["directory"], "outer");
}

#[test]
fn inspect_enumerates_streams_and_names_variant() {
    let f = standard_catpart();
    let mut cur = Cursor::new(f);
    let summary = CatiaCodec
        .inspect(&mut cur, &cadmpeg_core::decode::InspectOptions::default())
        .unwrap();
    assert_eq!(summary.format, "catia");
    assert_eq!(summary.container_kind, "v5-cfv2");
    assert!(summary.entries.iter().any(|e| e.name == "MainDataStream"));
    assert!(summary.entries.iter().any(|e| e.name == "SurfacicReps"));
    assert!(summary.notes.iter().any(|n| n.contains("standard nested")));
}

#[test]
fn finjpl_parser_splits_segments_and_classifies_type_words() {
    use crate::container::FinjplKind;

    let bytes = finjpl_stream();
    let segments = crate::container::finjpl_segments(&bytes, 0, bytes.len());
    assert_eq!(segments.len(), 2);
    assert_eq!(segments[0].kind, FinjplKind::Storage);
    assert_eq!(segments[0].type_word, 0x0000_008e);
    assert_eq!(segments[0].range, 2..17);
    assert_eq!(segments[1].kind, FinjplKind::ProjectFlags);
}

#[test]
fn e5_stream_selection_prefers_coherent_storage_segment_over_stray_preamble_marker() {
    let mut bytes = vec![0u8; 32];
    bytes[..8].copy_from_slice(OUTER_MAGIC);
    bytes[8..12].copy_from_slice(&512u32.to_be_bytes());
    bytes[12..16].copy_from_slice(&32u32.to_be_bytes());
    append_e5_record(&mut bytes, 0xfe, 1, &[]);
    bytes.extend_from_slice(b"FINJPL  ");
    bytes.extend_from_slice(&0x0000_0080u32.to_be_bytes());
    for id in 10..21 {
        append_e5_record(&mut bytes, 0xfe, id, &[]);
    }
    bytes.extend_from_slice(b"FINJPL  ");
    bytes.extend_from_slice(&0x0000_008eu32.to_be_bytes());
    let expected_start = bytes.len() - 12;
    for id in 30..41 {
        append_e5_record(&mut bytes, 0xfe, id, &[]);
    }
    bytes.resize(544, 0);

    let range = crate::container::e5_record_stream(&bytes).expect("coherent E5 stream");
    assert_eq!(range.start, expected_start);
    assert_eq!(&bytes[range.start..range.start + 8], b"FINJPL  ");
}

#[test]
fn consolidated_record_sources_follow_physical_stream_extents() {
    let scan = crate::container::scan_bytes(standard_catpart());
    let inner = scan.inner.as_ref().expect("inner stream directory");
    let expected = inner
        .descriptors
        .iter()
        .flat_map(|descriptor| {
            descriptor.extents.iter().map(|extent| {
                let start = inner.inner + extent.phys_off as usize;
                start..start + extent.phys_len as usize
            })
        })
        .collect::<Vec<_>>();
    let expected_sources = inner
        .descriptors
        .iter()
        .map(|descriptor| {
            descriptor
                .extents
                .iter()
                .map(|extent| {
                    let start = inner.inner + extent.phys_off as usize;
                    start..start + extent.phys_len as usize
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        crate::container::consolidated_record_ranges(&scan),
        expected
    );
    assert_eq!(
        crate::container::consolidated_record_sources(&scan),
        expected_sources
    );
    assert!(crate::container::consolidated_record_ranges(&scan)
        .iter()
        .all(|range| !range.contains(&inner.inner)));
}

#[test]
fn flagged_fbb_marker_is_structural() {
    assert!(crate::container::is_fbb_row(&[
        0xb0, 0x04, 0x04, 0xff, 0x99, 0x1f, 0x1a, 0xd1,
    ]));
    assert!(!crate::container::is_fbb_row(&[
        0x20, 0x04, 0x04, 0xff, 0xff, 0xc4, 0xb2, 0xaa,
    ]));
}

#[test]
fn fbb_census_separates_groups_from_face_rows() {
    let row = [0x30, 0x04, 0x04, 0xff, 0, 1, 2, 3];
    let mut body = row.to_vec();
    body.extend_from_slice(&row);
    body.extend_from_slice(&[0xaa; 8]);
    body.extend_from_slice(&row);

    assert_eq!(crate::container::fbb_run_ranges(&body), vec![0..16, 24..32]);
    let scan = crate::container::scan_bytes(standard_catpart());
    assert_eq!(scan.census.fbb_runs, 1);
    assert_eq!(scan.census.fbb_face_rows, 2);
}
