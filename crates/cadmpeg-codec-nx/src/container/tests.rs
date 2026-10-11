// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]

use crate::test_support::test_cfb::legacy_cfb_with_two_streams;
use crate::test_support::test_om::offset_only_indexed_om_section_with_index_values;
use crate::test_support::test_om::segment_index_payload;
use crate::test_support::test_om::size_framed_om_section_with_repeated_operations;
use crate::test_support::test_prt::append_rmfastload_table;
use crate::test_support::test_prt::prt_with_indexed_om_section;
use crate::test_support::test_prt::prt_with_named_payloads;
use crate::test_support::test_prt::rmfastload_prt;
use crate::test_support::test_prt::single_part_prt;
use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use cadmpeg_core::decode::{InspectOptions, ResourceDimension};
use cadmpeg_core::CodecError;

use crate::container;
use crate::container::{test_modern_layout, Container, ContainerLayout, DirEntry, Region};
use crate::NxCodec;

#[test]
fn ug_part_segment_index_uses_row_one_self_boundary() {
    let file = prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", segment_index_payload())]);
    let container =
        crate::test_support::with_decode_context(|ctx| container::scan_bytes(ctx, file)).unwrap();
    let (_, index) = container.segment_index().expect("segment index");
    assert_eq!(index.rows().count() * 12 + index.padding.len(), 28);
    assert_eq!(index.rows().count(), 2);
    assert_eq!(index.row(0).expect("first validated row").type_code, 7);
    assert_eq!(index.row(0).expect("first validated row").subtype_code, 9);
    assert_eq!(index.row(0).expect("first validated row").value, 11);
    assert_eq!(index.row(1).expect("second validated row").type_code, 1);
    assert_eq!(index.row(1).expect("second validated row").subtype_code, 1);
    assert_eq!(index.row(1).expect("second validated row").value, 28);
    assert_eq!(index.padding, &[0xaa, 0xbb, 0xcc, 0xdd]);
}

#[test]
fn container_parses_header_and_directory() {
    let c = crate::test_support::with_decode_context(|ctx| {
        container::scan_bytes(ctx, single_part_prt())
    })
    .unwrap();
    assert_eq!(c.layout.version(), 0x06);
    let ContainerLayout::Modern {
        file_tag,
        footer_fingerprint,
        ..
    } = c.layout
    else {
        panic!("SPLMSSTR input must have modern layout facts");
    };
    assert_eq!(
        crate::test_support::with_decode_context(|ctx| c.entry_count(ctx, Region::Header)).unwrap(),
        1
    );
    assert_eq!(file_tag, 0x33_22_11);
    assert_eq!(
        crate::test_support::with_decode_context(|ctx| c.entry_count(ctx, Region::Footer)).unwrap(),
        0
    );
    assert_eq!(footer_fingerprint, [0; 4]);
    assert!(c
        .entries
        .iter()
        .any(|e| e.name == "/Root/UG_PART/UG_PART" && e.file_span().is_some()));
}

#[test]
fn legacy_scan_refuses_work_after_directory_traversal() {
    let file = legacy_cfb_with_two_streams();

    // Start with the same small policy and advance across the shared CFB charges.
    let mut cap = 3;
    // Each positive refusal advances the exact cumulative allowance.
    let reached = loop {
        let limit = crate::test_support::with_decode_context_over(
            &file,
            |policy| policy.limits.max_work_units = cap,
            |ctx| {
                let root = cadmpeg_core::decode::View::over_retained(&file);
                let error = container::scan_legacy(ctx, root)
                    .expect_err("the legacy NX scan must refuse before its next operation");
                let CodecError::ResourceLimit(limit) = error else {
                    panic!("expected a work refusal: {error}");
                };
                assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
                assert_eq!(limit.limit, cap);
                limit
            },
        );
        if limit.operation == "scan legacy NX directory" {
            break true;
        }
        let needed = limit
            .used
            .checked_add(limit.additional)
            .expect("fixture work fits");
        assert!(needed > cap);
        cap = needed;
    };
    assert!(
        reached,
        "CFB admission must reach the legacy NX directory refusal"
    );
    crate::test_support::with_decode_context_over(
        &file,
        |_| {},
        |ctx| {
            assert!(
                container::scan_legacy(ctx, cadmpeg_core::decode::View::over_retained(&file))
                    .is_ok()
            );
        },
    );
}

#[test]
fn container_bounded_entry_tail_stops_at_the_next_stream() {
    let payload = [1, 2, 3, 4, 5, 6];
    let container = Container {
        data: payload.as_slice().into(),
        physical_size: cadmpeg_core::decode::u64_from_index(payload.len()),
        layout: ContainerLayout::LegacyCfb { version: 0 },
        entries: vec![
            DirEntry {
                name: "/Root/first".into(),
                region: Region::Header,
                body: crate::container::DirEntryBody::File { offset: 0, len: 3 },
            },
            DirEntry {
                name: "/Root/second".into(),
                region: Region::Header,
                body: crate::container::DirEntryBody::File { offset: 3, len: 3 },
            },
        ],
        fastload_table: None,
        segment_index: None,
        segment_wrappers: Vec::new(),
        indexed_section_layouts: std::sync::OnceLock::new(),
        om_section_cache: std::sync::OnceLock::new(),
    };
    assert_eq!(
        crate::test_support::with_decode_context(|ctx| container.bounded_entry_bytes(ctx, 1, 2))
            .unwrap(),
        Some(&payload[1..3])
    );
    assert_eq!(
        crate::test_support::with_decode_context(|ctx| container.bounded_entry_bytes(ctx, 1, 3))
            .unwrap(),
        None
    );
    assert_eq!(
        crate::test_support::with_decode_context(|ctx| container.bounded_entry_bytes(ctx, 3, 3))
            .unwrap(),
        Some(&payload[3..6])
    );
    assert_eq!(
        crate::test_support::with_decode_context(|ctx| container.bounded_entry_tail(ctx, 1))
            .unwrap(),
        Some(&payload[1..3])
    );
    assert_eq!(
        crate::test_support::with_decode_context(|ctx| container.bounded_entry_tail(ctx, 4))
            .unwrap(),
        Some(&payload[4..6])
    );
    assert_eq!(
        crate::test_support::with_decode_context(|ctx| container.bounded_entry_tail(ctx, 6))
            .unwrap(),
        None
    );
}

#[test]
fn container_cached_operation_labels_preserve_section_materialization() {
    let payload = size_framed_om_section_with_repeated_operations(2);
    let container = Container {
        data: payload.as_slice().into(),
        physical_size: cadmpeg_core::decode::u64_from_index(payload.len()),
        layout: test_modern_layout(0),
        entries: vec![DirEntry {
            name: "/Root/om".into(),
            region: Region::Header,
            body: crate::container::DirEntryBody::File {
                offset: 0,
                len: cadmpeg_core::decode::u64_from_index(payload.len()),
            },
        }],
        fastload_table: None,
        segment_index: None,
        segment_wrappers: Vec::new(),
        indexed_section_layouts: std::sync::OnceLock::new(),
        om_section_cache: std::sync::OnceLock::new(),
    };
    let direct =
        crate::test_support::with_decode_context(|ctx| crate::om::sections(ctx, &payload)).unwrap();
    let cached = crate::test_support::with_decode_context(|ctx| {
        container
            .om_sections(ctx)
            .map(|(sections, _storage)| sections)
    })
    .unwrap();
    assert_eq!(cached.len(), direct.len());
    assert!(container.om_section_cache.get().is_some());
    for ((entry, section), expected) in cached.iter().zip(direct.iter()) {
        assert_eq!(entry.name, "/Root/om");
        assert_eq!(section, expected);
        assert_eq!(section.operation_labels(), expected.operation_labels());
        assert_eq!(
            crate::test_support::with_decode_context(
                |ctx| section.operation_records_with_label_ordinals(ctx)
            )
            .unwrap(),
            crate::test_support::with_decode_context(
                |ctx| expected.operation_records_with_label_ordinals(ctx)
            )
            .unwrap()
        );
    }
    let repeated = crate::test_support::with_decode_context(|ctx| {
        container
            .om_sections(ctx)
            .map(|(sections, _storage)| sections)
    })
    .unwrap();
    assert_eq!(repeated, cached);
    assert!(std::sync::Arc::ptr_eq(
        &cached[0].1.types,
        &repeated[0].1.types
    ));
}

#[test]
fn container_caches_owned_section_layouts() {
    let payload = size_framed_om_section_with_repeated_operations(2);
    let payload_len = cadmpeg_core::decode::u64_from_index(payload.len());
    let mut file = vec![0xaa; 17];
    file.extend_from_slice(&payload);
    let physical_size = cadmpeg_core::decode::u64_from_index(file.len());
    let container = Container {
        data: file.into(),
        physical_size,
        layout: test_modern_layout(0),
        entries: vec![DirEntry {
            name: "/Root/om".into(),
            region: Region::Header,
            body: crate::container::DirEntryBody::File {
                offset: 17,
                len: payload_len,
            },
        }],
        fastload_table: None,
        segment_index: None,
        segment_wrappers: Vec::new(),
        indexed_section_layouts: std::sync::OnceLock::new(),
        om_section_cache: std::sync::OnceLock::new(),
    };
    let first = crate::test_support::with_decode_context(|ctx| {
        container
            .om_sections(ctx)
            .map(|(sections, _storage)| sections)
    })
    .unwrap();
    let second = crate::test_support::with_decode_context(|ctx| {
        container
            .om_sections(ctx)
            .map(|(sections, _storage)| sections)
    })
    .unwrap();
    assert_eq!(first.len(), 1);
    assert_eq!(second, first);
    assert_eq!(
        first[0].1,
        crate::test_support::with_decode_context(|ctx| crate::om::sections(
            ctx,
            &container.data[17..]
        ))
        .unwrap()[0]
    );
    assert!(container.om_section_cache.get().is_some_and(|cache| {
        matches!(
            cache,
            container::FramedSectionCache::Owned { layouts } if layouts.len() == 1
        )
    }));
}

#[test]
fn framed_section_cache_reader_refuses_collection_limit() {
    let payload = size_framed_om_section_with_repeated_operations(2);
    let container = Container {
        data: payload.as_slice().into(),
        physical_size: cadmpeg_core::decode::u64_from_index(payload.len()),
        layout: test_modern_layout(0),
        entries: vec![DirEntry {
            name: "/Root/om".into(),
            region: Region::Header,
            body: crate::container::DirEntryBody::File {
                offset: 0,
                len: cadmpeg_core::decode::u64_from_index(payload.len()),
            },
        }],
        fastload_table: None,
        segment_index: None,
        segment_wrappers: Vec::new(),
        indexed_section_layouts: std::sync::OnceLock::new(),
        om_section_cache: std::sync::OnceLock::new(),
    };
    crate::test_support::with_decode_context(|ctx| {
        container
            .om_sections(ctx)
            .map(|(sections, _storage)| sections)
    })
    .unwrap();

    crate::test_support::with_decode_context_over(
        &payload,
        |policy| {
            policy.limits.max_collection_items = 0;
        },
        |ctx| {
            let error = container
                .om_sections(ctx)
                .map(|(sections, _storage)| sections)
                .expect_err("one cached section exceeds zero items");
            assert!(
                matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::CollectionItems && limit.operation == "NX framed section readers")
            );
        },
    );
}

#[test]
fn indexed_section_cache_reader_refuses_collection_limit() {
    let file = prt_with_indexed_om_section();
    let container =
        crate::test_support::with_decode_context(|ctx| container::scan_bytes(ctx, file.as_slice()))
            .unwrap();
    crate::test_support::with_decode_context(|ctx| {
        container
            .indexed_om_sections(ctx)
            .map(|(sections, _storage)| sections)
    })
    .unwrap();

    crate::test_support::with_decode_context_over(
        &file,
        |policy| {
            policy.limits.max_collection_items = 0;
        },
        |ctx| {
            let error = container
                .indexed_om_sections(ctx)
                .map(|(sections, _storage)| sections)
                .expect_err("one cached section exceeds zero items");
            assert!(
                matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::CollectionItems && limit.operation == "NX indexed section readers")
            );
        },
    );
}

#[test]
fn container_reuses_materialized_indexed_sections_for_borrowed_input() {
    let file = prt_with_indexed_om_section();
    let container =
        crate::test_support::with_decode_context(|ctx| container::scan_bytes(ctx, file.as_slice()))
            .unwrap();
    let first = crate::test_support::with_decode_context(|ctx| {
        container
            .indexed_om_sections(ctx)
            .map(|(sections, _storage)| sections)
    })
    .unwrap();
    let second = crate::test_support::with_decode_context(|ctx| {
        container
            .indexed_om_sections(ctx)
            .map(|(sections, _storage)| sections)
    })
    .unwrap();
    assert!(!first.is_empty());
    assert_eq!(first, second);
    assert!(std::sync::Arc::ptr_eq(
        &first[0].1.types,
        &second[0].1.types
    ));
    match (&first[0].1.store, &second[0].1.store) {
        (
            crate::om::IndexedStore::Fixed {
                records: first_records,
            },
            crate::om::IndexedStore::Fixed {
                records: second_records,
            },
        ) => assert!(std::sync::Arc::ptr_eq(first_records, second_records)),
        (
            crate::om::IndexedStore::OffsetOnly {
                records: first_records,
                ..
            },
            crate::om::IndexedStore::OffsetOnly {
                records: second_records,
                ..
            },
        ) => assert!(std::sync::Arc::ptr_eq(first_records, second_records)),
        _ => panic!("indexed section store kind changed between cache hits"),
    }
}

#[test]
fn container_reuses_borrowed_offset_store_block_index() {
    let section = offset_only_indexed_om_section_with_index_values();
    let file = prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", section)]);
    let container =
        crate::test_support::with_decode_context(|ctx| container::scan_bytes(ctx, file.as_slice()))
            .unwrap();
    let _ = crate::test_support::with_decode_context(|ctx| {
        container
            .indexed_om_sections(ctx)
            .map(|(sections, _storage)| sections)
    })
    .unwrap();
    let first = container
        .cached_offset_data_block_bytes()
        .expect("borrowed indexed sections cache their offset-store blocks");
    let second = container
        .cached_offset_data_block_bytes()
        .expect("cached offset-store blocks remain available");

    assert!(!first.is_empty());
    assert!(first.contains_key("nx:om-data-blocks-0:block#0"));
    assert!(std::ptr::eq(first, second));
}

#[test]
fn container_counts_admitted_entries_in_each_region() {
    let mut file = single_part_prt();
    file.truncate(file.len() - 8);
    file.extend_from_slice(&1_u32.to_le_bytes());
    file.extend_from_slice(&6_u32.to_le_bytes());
    file.extend_from_slice(b"/Root/");
    file.extend_from_slice(&[0; 16]);
    file.extend_from_slice(&[0; 4]);
    let container =
        crate::test_support::with_decode_context(|ctx| container::scan_bytes(ctx, file))
            .expect("one entry in each counted region");
    assert_eq!(
        crate::test_support::with_decode_context(|ctx| container.entry_count(ctx, Region::Header))
            .unwrap(),
        1
    );
    assert_eq!(
        crate::test_support::with_decode_context(|ctx| container.entry_count(ctx, Region::Footer))
            .unwrap(),
        1
    );
    assert_eq!(container.entries.len(), 2);
}

#[test]
fn service_profile_admits_both_directory_regions() {
    let mut file = single_part_prt();
    file.truncate(file.len() - 8);
    file.extend_from_slice(&1_u32.to_le_bytes());
    file.extend_from_slice(&6_u32.to_le_bytes());
    file.extend_from_slice(b"/Root/");
    file.extend_from_slice(&[0; 16]);
    file.extend_from_slice(&[0; 4]);

    crate::test_support::with_decode_context_over(
        &file,
        |_| {},
        |ctx| {
            let container =
                container::scan_bytes(ctx, file.as_slice()).expect("service directory admission");
            assert_eq!(
                crate::test_support::with_decode_context(
                    |ctx| container.entry_count(ctx, Region::Header)
                )
                .unwrap(),
                1
            );
            assert_eq!(
                crate::test_support::with_decode_context(
                    |ctx| container.entry_count(ctx, Region::Footer)
                )
                .unwrap(),
                1
            );
        },
    );
}

#[test]
fn header_directory_refuses_collection_limit_before_entry_reserve() {
    let file = single_part_prt();

    crate::test_support::with_decode_context_over(
        &file,
        |policy| {
            policy.limits.max_collection_items = 0;
        },
        |ctx| {
            let error = container::scan_bytes(ctx, file.as_slice())
                .expect_err("header entry needs one item");
            let CodecError::ResourceLimit(limit) = error else {
                panic!("directory must return a resource refusal: {error}");
            };
            assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
            assert_eq!(limit.operation, "admit NX directory entries");
        },
    );
}

#[test]
fn footer_directory_refuses_collection_limit_before_entry_reserve() {
    let mut file = single_part_prt();
    file.truncate(file.len() - 8);
    file.extend_from_slice(&1_u32.to_le_bytes());
    file.extend_from_slice(&6_u32.to_le_bytes());
    file.extend_from_slice(b"/Root/");
    file.extend_from_slice(&[0; 16]);
    file.extend_from_slice(&[0; 4]);

    crate::test_support::with_decode_context_over(
        &file,
        |policy| {
            policy.limits.max_collection_items = 1;
        },
        |ctx| {
            let error = container::scan_bytes(ctx, file.as_slice())
                .expect_err("footer entry needs second item");
            let CodecError::ResourceLimit(limit) = error else {
                panic!("directory must return a resource refusal: {error}");
            };
            assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
            assert_eq!(limit.operation, "admit NX directory entries");
        },
    );
}

#[test]
fn header_directory_refuses_retained_name_limit_before_copy() {
    let file = single_part_prt();

    crate::test_support::with_decode_context_over(
        &file,
        |policy| {
            policy.limits.max_retained_bytes = cadmpeg_core::decode::u64_from_index(
                4 * std::mem::size_of::<DirEntry>() + "/Root/UG_PART/UG_PART".len() - 1,
            );
        },
        |ctx| {
            let error = container::scan_bytes(ctx, file.as_slice())
                .expect_err("name copy needs one more byte");
            let CodecError::ResourceLimit(limit) = error else {
                panic!("directory name must return a resource refusal: {error}");
            };
            assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
            assert_eq!(limit.operation, "retain NX directory name");
        },
    );
}

#[test]
fn container_rejects_incomplete_counted_directories() {
    let mut header = single_part_prt();
    header[0x1f..0x23].copy_from_slice(&2_u32.to_le_bytes());
    assert!(
        crate::test_support::with_decode_context(|ctx| container::scan_bytes(ctx, header)).is_err()
    );

    let mut footer = single_part_prt();
    let footer_offset = usize::try_from(u64::from_le_bytes([
        footer[0x11],
        footer[0x12],
        footer[0x13],
        footer[0x14],
        footer[0x15],
        footer[0x16],
        0,
        0,
    ]))
    .expect("synthetic footer offset");
    footer[footer_offset + 6..footer_offset + 10].copy_from_slice(&1_u32.to_le_bytes());
    assert!(
        crate::test_support::with_decode_context(|ctx| container::scan_bytes(ctx, footer)).is_err()
    );
}

#[test]
fn container_rejects_trailing_or_overlapping_footer_data() {
    let mut trailing = single_part_prt();
    trailing.push(0);
    assert!(
        crate::test_support::with_decode_context(|ctx| container::scan_bytes(ctx, trailing))
            .is_err()
    );

    let mut overlap = single_part_prt();
    let name_len = usize::try_from(u32::from_le_bytes(
        overlap[0x23..0x27]
            .try_into()
            .expect("synthetic name length"),
    ))
    .expect("synthetic name length fits usize");
    let span = 0x27 + name_len;
    let offset = u64::from_le_bytes(
        overlap[span..span + 8]
            .try_into()
            .expect("synthetic file offset"),
    );
    let footer_offset = u64::from_le_bytes([
        overlap[0x11],
        overlap[0x12],
        overlap[0x13],
        overlap[0x14],
        overlap[0x15],
        overlap[0x16],
        0,
        0,
    ]);
    overlap[span + 8..span + 16].copy_from_slice(&(footer_offset - offset + 1).to_le_bytes());
    assert!(
        crate::test_support::with_decode_context(|ctx| container::scan_bytes(ctx, overlap))
            .is_err()
    );
}

#[test]
fn container_rejects_footer_offset_beyond_the_file_image() {
    let mut bytes = single_part_prt();
    bytes[0x11..0x17].copy_from_slice(&[0xff; 6]);
    let error = crate::test_support::with_decode_context(|ctx| container::scan_bytes(ctx, bytes))
        .expect_err("required invariant");
    assert_eq!(
        error.to_string(),
        "malformed container: FOOTER offset exceeds the file image"
    );
}

#[test]
fn container_reads_rmfastload_active_ids() {
    let container = crate::test_support::with_decode_context(|ctx| {
        container::scan_bytes(ctx, rmfastload_prt())
    })
    .unwrap();
    let (entry, table) = container
        .rmfastload_object_id_table()
        .expect("RMFastLoad object-id table");
    assert_eq!(entry.name, "/Root/FastLoad/RMFastLoad");
    assert_eq!(table.registry_offset, 0);
    assert_eq!(table.count_offset, b"UGS::Solid::Topol".len());
    assert_eq!(table.object_ids.count().to_le_bytes(), 50u32.to_le_bytes());
    assert_eq!(
        table.object_ids.as_slice().to_vec(),
        (1..=50).collect::<Vec<_>>()
    );
    assert_eq!(table.member_offset(0), table.count_offset + 4);
    assert_eq!(
        table.object_ids.as_slice()[0].to_le_bytes(),
        1u32.to_le_bytes()
    );
    assert_eq!(table.member_offset(49), table.count_offset + 4 + 49 * 4);
    assert_eq!(
        table.object_ids.as_slice()[49].to_le_bytes(),
        50u32.to_le_bytes()
    );
    let (_, same_table) = container
        .rmfastload_object_id_table()
        .expect("shared RMFastLoad table");
    assert!(std::ptr::eq(table, same_table));
}

#[test]
fn fastload_id_table_refuses_collection_limit_before_reserve() {
    let file = rmfastload_prt();

    crate::test_support::with_decode_context_over(
        &file,
        |policy| {
            policy.limits.max_collection_items = 50;
        },
        |ctx| {
            let error = container::scan_bytes(ctx, file.as_slice())
                .expect_err("fifty IDs need a second collection charge");
            let CodecError::ResourceLimit(limit) = error else {
                panic!("FastLoad IDs must return a resource refusal: {error}");
            };
            assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
            assert_eq!(limit.operation, "admit NX FastLoad object IDs");
        },
    );
}

#[test]
fn fastload_candidate_scan_refuses_work_limit_before_reading_count() {
    let file = rmfastload_prt();
    // Admit marker search and refuse the candidate visit before its count read.
    let error = crate::test_support::resource_refusal_at(
        &file,
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "scan NX FastLoad table candidates",
        |ctx| container::scan_bytes(ctx, file.as_slice()),
    );
    let CodecError::ResourceLimit(limit) = error else {
        panic!("FastLoad scan must return a resource refusal: {error}");
    };
    assert_eq!(
        limit.dimension,
        cadmpeg_core::decode::ResourceDimension::WorkUnits
    );
    assert_eq!(limit.operation, "scan NX FastLoad table candidates");
}

#[test]
fn fastload_id_table_refuses_retained_limit_before_reserve() {
    let file = rmfastload_prt();

    let directory_bytes = std::mem::size_of::<DirEntry>() + "/Root/FastLoad/RMFastLoad".len();

    crate::test_support::with_decode_context_over(
        &file,
        |policy| {
            policy.limits.max_retained_bytes = cadmpeg_core::decode::u64_from_index(
                directory_bytes + 50 * std::mem::size_of::<u32>() - 1,
            );
        },
        |ctx| {
            let error = container::scan_bytes(ctx, file.as_slice())
                .expect_err("ID copy needs one more byte");
            let CodecError::ResourceLimit(limit) = error else {
                panic!("FastLoad IDs must return a resource refusal: {error}");
            };
            assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
            assert_eq!(limit.operation, "retain NX FastLoad object IDs");
        },
    );
}

#[test]
fn container_reads_rmfastload_table_from_product_boundary_without_range_floor() {
    let mut payload = b"UGS::Solid::Topol".to_vec();
    append_rmfastload_table(&mut payload, [0, u32::MAX, 7]);
    let file = prt_with_named_payloads(&[("/Root/FastLoad/RMFastLoad", payload)]);
    let container =
        crate::test_support::with_decode_context(|ctx| container::scan_bytes(ctx, file)).unwrap();
    let (_, table) = container
        .rmfastload_object_id_table()
        .expect("product-bounded RMFastLoad table");
    assert_eq!(table.object_ids.as_slice().len(), 3);
    assert_eq!(table.object_ids.as_slice().to_vec(), [0, u32::MAX, 7]);
}

#[test]
fn fuzz_oom_splmsstr_header_is_rejected_without_count_allocation() {
    // libFuzzer artifact: SPLMSSTR + HEADER with a footer offset past EOF and a
    // directory count that would request >2 GiB if taken as a Vec capacity.
    let bytes: &[u8] = &[
        0x53, 0x50, 0x4c, 0x4d, 0x53, 0x53, 0x54, 0x52, 0x26, 0x00, 0x00, 0x00, 0xff, 0xff, 0xff,
        0xff, 0xff, 0x0a, 0x00, 0x06, 0x00, 0xff, 0xff, 0x90, 0xff, 0x48, 0x45, 0x41, 0x44, 0x45,
        0x52, 0x20, 0x00, 0x00, 0x6f, 0x2f, 0xf9, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x04, 0x00, 0x04,
    ];
    assert!(
        crate::test_support::with_decode_context(|ctx| container::scan_bytes(ctx, bytes.to_vec()))
            .is_err()
    );
    let _ = cadmpeg_test_support::detection::confidence(&NxCodec, bytes);
    let _probe = NxCodec.inspect(&mut Cursor::new(bytes), &InspectOptions::default());
    let _probe = NxCodec.decode(&mut Cursor::new(bytes), &DecodeOptions::default());
}

#[test]
fn container_bounds_rmfastload_table_at_its_first_product_record() {
    let mut payload = b"UGS::Solid::Topol".to_vec();
    append_rmfastload_table(&mut payload, [1, 2, 3]);
    append_rmfastload_table(&mut payload, [4, 5]);
    let file = prt_with_named_payloads(&[("/Root/FastLoad/RMFastLoad", payload)]);
    let container =
        crate::test_support::with_decode_context(|ctx| container::scan_bytes(ctx, file)).unwrap();
    let (_, table) = container
        .rmfastload_object_id_table()
        .expect("first product-bounded table");
    assert_eq!(table.object_ids.as_slice().to_vec(), [1, 2, 3]);
}

#[test]
fn external_reference_string_table_is_end_anchored() {
    let table = b"prefix\x01\x02\x00\x00\x00\x09\x00child.prt\x0c\x00nested/b.prt";
    let (_, strings) = crate::test_support::with_decode_context(|ctx| {
        crate::container::parse_extref_string_table(ctx, table)
            .map(|table| table.map(|(table, _storage)| table))
    })
    .expect("string table resources")
    .expect("string table");
    assert_eq!(
        strings
            .into_iter()
            .map(|(_, value)| value)
            .collect::<Vec<_>>(),
        ["child.prt", "nested/b.prt"]
    );

    let mut trailed = table.to_vec();
    trailed.push(0);
    assert!(crate::test_support::with_decode_context(|ctx| {
        crate::container::parse_extref_string_table(ctx, &trailed)
            .map(|table| table.map(|(table, _storage)| table))
    })
    .expect("string table resources")
    .is_none());
    assert!(crate::test_support::with_decode_context(|ctx| {
        crate::container::parse_extref_string_table(ctx, b"\x01\xff\xff\xff\xff")
            .map(|table| table.map(|(table, _storage)| table))
    })
    .expect("string table resources")
    .is_none());
}

#[test]
fn external_reference_paths_refuse_collection_limit() {
    let payload = b"prefix\x01\x01\x00\x00\x00\x09\x00child.prt";
    let container = external_reference_path_container(payload);
    let error = crate::test_support::resource_refusal_at(
        payload,
        ResourceDimension::CollectionItems,
        "nx external reference paths",
        |ctx| {
            container
                .external_reference_paths(ctx)
                .map(|(paths, _storage)| paths)
        },
    );
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::CollectionItems && limit.operation == "nx external reference paths")
    );
}

fn external_reference_path_container(payload: &[u8]) -> Container<'_> {
    Container {
        data: payload.into(),
        physical_size: cadmpeg_core::decode::u64_from_index(payload.len()),
        layout: ContainerLayout::LegacyCfb { version: 0 },
        entries: vec![DirEntry {
            name: "/Root/ExternalReferences".into(),
            region: Region::Header,
            body: crate::container::DirEntryBody::File {
                offset: 0,
                len: cadmpeg_core::decode::u64_from_index(payload.len()),
            },
        }],
        fastload_table: None,
        segment_index: None,
        segment_wrappers: Vec::new(),
        indexed_section_layouts: std::sync::OnceLock::new(),
        om_section_cache: std::sync::OnceLock::new(),
    }
}

#[test]
fn external_reference_paths_refuse_retained_string_limit() {
    let payload = b"prefix\x01\x01\x00\x00\x00\x09\x00child.prt";
    let container = external_reference_path_container(payload);
    let error = crate::test_support::resource_refusal_at(
        payload,
        ResourceDimension::RetainedBytes,
        "nx external reference string",
        |ctx| {
            container
                .external_reference_paths(ctx)
                .map(|(paths, _storage)| paths)
        },
    );
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::RetainedBytes && limit.operation == "nx external reference string")
    );
}

#[test]
fn external_reference_paths_refuse_work_limit() {
    let payload = b"prefix\x01\x01\x00\x00\x00\x09\x00child.prt";
    let container = external_reference_path_container(payload);
    let error = crate::test_support::resource_refusal_at(
        payload,
        ResourceDimension::WorkUnits,
        "project NX external reference paths",
        |ctx| {
            container
                .external_reference_paths(ctx)
                .map(|(paths, _storage)| paths)
        },
    );
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::WorkUnits && limit.operation == "project NX external reference paths")
    );
}

#[test]
fn external_reference_paths_refuse_before_visiting_the_unselected_suffix() {
    let payload = b"prefix\x01\x02\x00\x00\x00\x09\x00child.prt\x0c\x00nested/b.prt";
    let container = external_reference_path_container(payload);
    let error = crate::test_support::resource_refusal_at(
        payload,
        ResourceDimension::WorkUnits,
        "project NX external reference paths",
        |ctx| {
            container
                .external_reference_paths(ctx)
                .map(|(paths, _storage)| paths)
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::WorkUnits
                && limit.operation == "project NX external reference paths"
                && limit.additional == 1));
}

#[test]
fn external_reference_strings_refuse_before_visiting_the_unselected_suffix() {
    let payload = b"prefix\x01\x02\x00\x00\x00\x09\x00child.prt\x0c\x00nested/b.prt";
    let container = external_reference_path_container(payload);
    let error = crate::test_support::resource_refusal_at(
        payload,
        ResourceDimension::WorkUnits,
        "project NX external reference strings",
        |ctx| {
            let (strings, storage) = container.external_reference_strings(ctx)?;
            drop(strings);
            drop(storage);
            Ok(())
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits
            && limit.operation == "project NX external reference strings"
            && limit.additional == 1));
}

#[test]
fn external_reference_record_parser_accepts_sorted_repeated_handles() {
    let mut payload = b"EXTREFSTREAM".to_vec();
    payload.extend_from_slice(&3u32.to_le_bytes());
    payload.extend_from_slice(&0u32.to_le_bytes());
    payload.extend_from_slice(&0u32.to_le_bytes());
    payload.push(0);
    payload.extend_from_slice(&6u32.to_le_bytes());
    payload.extend_from_slice(&41u32.to_le_bytes());
    payload.extend_from_slice(&0u32.to_le_bytes());
    payload.extend_from_slice(&0u32.to_le_bytes());
    assert_eq!(payload.len(), 41);
    payload.extend_from_slice(&[1, 0, 0, 0]);
    payload.extend_from_slice(&2u16.to_be_bytes());
    payload.push(1);
    for value in [8u32, 11, 12, 4] {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    payload.extend_from_slice(&[1, 5]);
    for handle in [0x1020_3040u32, 0x2030_4050, 0x2030_4050, 0x2030_4050] {
        payload.push(0xe0);
        payload.extend_from_slice(&handle.to_be_bytes());
    }
    payload.push(5);
    payload.extend_from_slice(b"\x01\x01\x00\x00\x00\x09\x00child.prt");

    crate::test_support::with_decode_context(|ctx| {
        let (records, records_storage, handles_storage) =
            crate::container::parse_extref_records(ctx, &payload).expect("record resources");
        let (indexed, indexed_storage) = crate::container::parse_extref_record_index(ctx, &payload)
            .expect("index resources")
            .expect("record index");
        assert_eq!(indexed.len(), 1);
        assert_eq!(indexed[0].record_id, 6);
        assert_eq!(indexed[0].offset, 41);
        assert_eq!(indexed[0].byte_len, 46);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].record_id, 6);
        assert_eq!(records[0].declared_count, 2);
        assert_eq!(records[0].id_slots, [8, 11, 12, 4]);
        assert_eq!(
            records[0].handles.values(),
            [0x1020_3040, 0x2030_4050, 0x2030_4050]
        );
        assert!(records[0].handles.closing_duplicate());
        assert_eq!(records[0].tail_byte_len, 0);
        drop(records);
        drop(records_storage);
        drop(handles_storage);
        drop(indexed);
        drop(indexed_storage);
    });

    let duplicate = payload
        .windows(5)
        .rposition(|window| window == [0xe0, 0x20, 0x30, 0x40, 0x50])
        .expect("closing duplicate");
    payload[duplicate + 1] = 0x10;
    crate::test_support::with_decode_context(|ctx| {
        let (records, records_storage, handles_storage) =
            crate::container::parse_extref_records(ctx, &payload).expect("record resources");
        assert!(records.is_empty());
        drop(records);
        drop(records_storage);
        drop(handles_storage);
        let (indexed, indexed_storage) = crate::container::parse_extref_record_index(ctx, &payload)
            .expect("index resources")
            .expect("opaque indexed record");
        assert_eq!(indexed.len(), 1);
        drop(indexed);
        drop(indexed_storage);
    });
}

fn one_btree_node_storage_bytes<K, V>() -> usize {
    // Match DecodeContext::tree_node_bytes: eleven key/value lanes, twelve
    // child pointers, and metadata plus alignment for the std B-tree node.
    let alignment = std::mem::align_of::<K>()
        .max(std::mem::align_of::<V>())
        .max(std::mem::align_of::<usize>());
    11 * (std::mem::size_of::<K>() + std::mem::size_of::<V>())
        + 16 * std::mem::size_of::<usize>()
        + 2 * alignment
}

fn indexed_extref_payload(record_count: usize) -> Vec<u8> {
    let mut payload = b"EXTREFSTREAM".to_vec();
    payload.extend([0u8; 13]);
    let records_start = payload.len() + 8 * record_count + 4;
    for ordinal in 0..record_count {
        let record_id = u32::try_from(ordinal + 1).expect("record id fits u32");
        let offset = u32::try_from(records_start + ordinal).expect("record offset fits u32");
        payload.extend(record_id.to_le_bytes());
        payload.extend(offset.to_le_bytes());
    }
    payload.extend(0u32.to_le_bytes());
    assert_eq!(payload.len(), records_start);
    payload.resize(records_start + record_count, 0);
    payload.push(1);
    payload.extend(0u32.to_le_bytes());
    payload
}

fn nine_push_growth_copy_work<T>() -> u64 {
    // Core amortized Vec growth starts these element sizes at capacity 4,
    // then doubles: nine pushes move four old slots at push 5 and eight at push 9.
    let item_size = std::mem::size_of::<T>();
    assert!((2..=1024).contains(&item_size));
    let moved_bytes = 4usize
        .checked_mul(item_size)
        .and_then(|bytes| bytes.checked_add(8usize.checked_mul(item_size)?))
        .expect("nine-push vector growth work fits usize");
    cadmpeg_core::decode::u64_from_index(moved_bytes)
}

#[test]
fn external_reference_record_parser_charges_only_visited_index_records() {
    const RECORD_COUNT: usize = 9;
    let payload = crate::test_support::test_streams::external_reference_handle_sets(RECORD_COUNT);
    let error = crate::test_support::resource_refusal_at(
        &[],
        ResourceDimension::WorkUnits,
        "parse NX external reference records",
        |ctx| {
            let (records, storage, handles_storage) =
                container::parse_extref_records(ctx, &payload)?;
            drop(records);
            drop(storage);
            drop(handles_storage);
            Ok(())
        },
    );
    let CodecError::ResourceLimit(limit) = error else {
        panic!("external reference record visits must be charged individually");
    };
    assert_eq!(limit.additional, 1);
    let exact_work = limit
        .used
        .checked_add(cadmpeg_core::decode::u64_from_index(RECORD_COUNT))
        .and_then(|work| work.checked_add(nine_push_growth_copy_work::<container::ExtrefRecord>()))
        .expect("parser visit and vector growth work fits u64");

    crate::test_support::with_decode_context_over(
        &[],
        |policy| policy.limits.max_work_units = exact_work,
        |ctx| {
            let (records, storage, handles_storage) =
                container::parse_extref_records(ctx, &payload).expect("all nine visits fit");
            assert_eq!(records.len(), RECORD_COUNT);
            drop(records);
            drop(storage);
            drop(handles_storage);
            assert_eq!(ctx.resource_refusal(), None);
        },
    );
}

#[test]
fn external_reference_handle_parser_accepts_the_bounded_token_count() {
    use crate::layout::extrefstream_handle_set_record as handle_set;

    let mut payload = crate::test_support::test_streams::external_reference_handle_sets(1);
    let record_start = 25 + 8 + 4;
    let tokens_start = record_start + handle_set::LEN;
    payload.truncate(tokens_start);
    payload[record_start + handle_set::COUNT] = 255;
    for handle in 0..254_u32 {
        payload.push(0xe0);
        payload.extend(handle.to_be_bytes());
    }
    payload.push(255);
    payload.push(1);
    payload.extend(0_u32.to_le_bytes());
    crate::test_support::with_decode_context(|ctx| {
        let (records, _storage, _handles_storage) =
            container::parse_extref_records(ctx, &payload).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(
            records[0].handles.serialized(),
            (0..254).collect::<Vec<_>>()
        );
        assert_eq!(
            records[0].handles.prefix_byte_len(),
            handle_set::LEN + 254 * 5 + 1
        );
    });
}

#[test]
fn external_reference_record_parser_rejects_unsorted_handles() {
    let mut payload = crate::test_support::test_streams::external_reference_stream();
    let first = 51 + crate::layout::extrefstream_handle_set_record::LEN;
    let second = first + 5;
    payload[first + 1..first + 5].copy_from_slice(&0x20u32.to_be_bytes());
    payload[second + 1..second + 5].copy_from_slice(&0x10u32.to_be_bytes());

    crate::test_support::with_decode_context(|ctx| {
        let (records, storage, handles_storage) = container::parse_extref_records(ctx, &payload)
            .expect("invalid handle ordering is a grammar rejection");
        assert!(records.is_empty());
        assert_eq!(ctx.resource_refusal(), None);
        drop(records);
        drop(storage);
        drop(handles_storage);
    });
}

#[test]
fn external_reference_index_parser_charges_only_formed_rows() {
    const RECORD_COUNT: usize = 9;
    let payload = indexed_extref_payload(RECORD_COUNT);
    let error = crate::test_support::resource_refusal_at(
        &[],
        ResourceDimension::WorkUnits,
        "form NX external reference index",
        |ctx| {
            let Some((records, storage)) = container::parse_extref_record_index(ctx, &payload)?
            else {
                return Ok(());
            };
            drop(records);
            drop(storage);
            Ok(())
        },
    );
    let CodecError::ResourceLimit(limit) = error else {
        panic!("external reference index rows must be charged individually");
    };
    assert_eq!(limit.additional, 1);
    let exact_work = limit
        .used
        .checked_add(cadmpeg_core::decode::u64_from_index(RECORD_COUNT))
        .expect("index visit work fits u64");

    crate::test_support::with_decode_context_over(
        &[],
        |policy| policy.limits.max_work_units = exact_work,
        |ctx| {
            let (records, storage) = container::parse_extref_record_index(ctx, &payload)
                .expect("index parser resources")
                .expect("nine indexed rows");
            assert_eq!(records.len(), RECORD_COUNT);
            drop(records);
            drop(storage);
            assert_eq!(ctx.resource_refusal(), None);
        },
    );
}

#[test]
fn external_reference_record_container_projection_charges_each_tuple() {
    const RECORD_COUNT: usize = 9;
    let payload = crate::test_support::test_streams::external_reference_handle_sets(RECORD_COUNT);
    let file = prt_with_named_payloads(&[("/Root/ExternalReferences", payload)]);
    let container =
        crate::test_support::with_decode_context(|ctx| container::scan_bytes(ctx, file.as_slice()))
            .expect("external reference container");
    let error = crate::test_support::resource_refusal_at(
        &[],
        ResourceDimension::WorkUnits,
        "project NX external reference record entries",
        |ctx| {
            let (records, storage, handles_storage) = container.external_reference_records(ctx)?;
            drop(records);
            drop(storage);
            drop(handles_storage);
            Ok(())
        },
    );
    let CodecError::ResourceLimit(limit) = error else {
        panic!("external reference tuple visits must be charged individually");
    };
    assert_eq!(limit.additional, 1);
    let exact_work = limit
        .used
        .checked_add(cadmpeg_core::decode::u64_from_index(RECORD_COUNT))
        .and_then(|work| {
            work.checked_add(nine_push_growth_copy_work::<(
                &DirEntry,
                container::ExtrefRecord,
            )>())
        })
        .expect("record tuple and vector growth work fits u64");

    crate::test_support::with_decode_context_over(
        &[],
        |policy| policy.limits.max_work_units = exact_work,
        |ctx| {
            let (records, storage, handles_storage) = container
                .external_reference_records(ctx)
                .expect("all nine tuple visits fit");
            assert_eq!(records.len(), RECORD_COUNT);
            drop(records);
            drop(storage);
            drop(handles_storage);
            assert_eq!(ctx.resource_refusal(), None);
        },
    );
}

#[test]
fn external_reference_index_container_projection_charges_each_tuple() {
    const RECORD_COUNT: usize = 9;
    let payload = indexed_extref_payload(RECORD_COUNT);
    let file = prt_with_named_payloads(&[("/Root/ExternalReferences", payload)]);
    let container =
        crate::test_support::with_decode_context(|ctx| container::scan_bytes(ctx, file.as_slice()))
            .expect("external reference container");
    let error = crate::test_support::resource_refusal_at(
        &[],
        ResourceDimension::WorkUnits,
        "project NX external reference indexed entries",
        |ctx| {
            let (records, storage) = container.external_reference_indexed_records(ctx)?;
            drop(records);
            drop(storage);
            Ok(())
        },
    );
    let CodecError::ResourceLimit(limit) = error else {
        panic!("external reference index tuple visits must be charged individually");
    };
    assert_eq!(limit.additional, 1);
    let exact_work = limit
        .used
        .checked_add(cadmpeg_core::decode::u64_from_index(RECORD_COUNT))
        .and_then(|work| {
            work.checked_add(nine_push_growth_copy_work::<(
                &DirEntry,
                container::ExtrefIndexedRecord,
            )>())
        })
        .expect("indexed tuple and vector growth work fits u64");

    crate::test_support::with_decode_context_over(
        &[],
        |policy| policy.limits.max_work_units = exact_work,
        |ctx| {
            let (records, storage) = container
                .external_reference_indexed_records(ctx)
                .expect("all nine tuple visits fit");
            assert_eq!(records.len(), RECORD_COUNT);
            drop(records);
            drop(storage);
            assert_eq!(ctx.resource_refusal(), None);
        },
    );
}

#[test]
fn external_reference_record_projection_keeps_scratch_and_retained_handles_separate() {
    let payload = crate::test_support::test_streams::external_reference_stream();
    let file = prt_with_named_payloads(&[("/Root/ExternalReferences", payload)]);
    let container =
        crate::test_support::with_decode_context(|ctx| container::scan_bytes(ctx, file.as_slice()))
            .expect("external reference container");

    let index_node = one_btree_node_storage_bytes::<u32, ()>();
    let index_peak = index_node
        + 4 * std::mem::size_of::<(u32, usize)>()
        + 2 * std::mem::size_of::<container::ExtrefIndexedRecord>();
    let handle_storage = 2 * std::mem::size_of::<u32>();
    let parsed_records_peak = 2 * std::mem::size_of::<container::ExtrefIndexedRecord>()
        + 4 * std::mem::size_of::<container::ExtrefRecord>()
        + handle_storage;
    let output_slots = 4 * std::mem::size_of::<(&DirEntry, container::ExtrefRecord)>();
    let projection_peak =
        4 * std::mem::size_of::<container::ExtrefRecord>() + handle_storage + output_slots;
    let peak = index_peak.max(parsed_records_peak).max(projection_peak);

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            policy.limits.max_materialized_bytes = cadmpeg_core::decode::u64_from_index(peak);
        },
        |ctx| {
            let (records, storage, handles_storage) = container
                .external_reference_records(ctx)
                .expect("record projection fits its exact scratch peak");
            assert_eq!(records.len(), 1);
            assert_eq!(records[0].1.handles.serialized().len(), 2);
            let refusal = ctx
                .reserve_scoped(
                    cadmpeg_core::decode::u64_from_index(peak - output_slots - handle_storage + 1),
                    "probe external reference result lifetime",
                )
                .expect_err("returned outer slots remain scoped until the vector is dropped");
            assert!(matches!(refusal, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::MaterializedBytes
                && limit.used == cadmpeg_core::decode::u64_from_index(
                    output_slots + handle_storage
                )
                && limit.additional == cadmpeg_core::decode::u64_from_index(
                    peak - output_slots - handle_storage + 1
                )));
            drop(records);
            drop(storage);
            drop(handles_storage);
        },
    );
}

#[test]
fn external_reference_record_projection_refuses_exact_outer_scratch_growth() {
    let payload = crate::test_support::test_streams::external_reference_stream();
    let file = prt_with_named_payloads(&[("/Root/ExternalReferences", payload)]);
    let container =
        crate::test_support::with_decode_context(|ctx| container::scan_bytes(ctx, file.as_slice()))
            .expect("external reference container");
    let index_node = one_btree_node_storage_bytes::<u32, ()>();
    let index_peak = index_node
        + 4 * std::mem::size_of::<(u32, usize)>()
        + 2 * std::mem::size_of::<container::ExtrefIndexedRecord>();
    let handle_storage = 2 * std::mem::size_of::<u32>();
    let parsed_records_peak = 2 * std::mem::size_of::<container::ExtrefIndexedRecord>()
        + 4 * std::mem::size_of::<container::ExtrefRecord>()
        + handle_storage;
    let output_slots = 4 * std::mem::size_of::<(&DirEntry, container::ExtrefRecord)>();
    let projection_peak =
        4 * std::mem::size_of::<container::ExtrefRecord>() + handle_storage + output_slots;
    let peak = index_peak.max(parsed_records_peak).max(projection_peak);

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            policy.limits.max_materialized_bytes = cadmpeg_core::decode::u64_from_index(peak - 1);
        },
        |ctx| {
            let error = container
                .external_reference_records(ctx)
                .expect_err("the outer record vector needs all four initial slots");
            assert!(matches!(error, CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::MaterializedBytes
                    && limit.operation == "nx external reference record entries"
                    && limit.limit == cadmpeg_core::decode::u64_from_index(peak - 1)
                    && limit.used == cadmpeg_core::decode::u64_from_index(
                        4 * std::mem::size_of::<container::ExtrefRecord>() + handle_storage
                    )
                    && limit.additional == cadmpeg_core::decode::u64_from_index(output_slots)));
        },
    );
}

#[test]
fn external_reference_indexed_projection_keeps_outer_growth_receipt_live() {
    let payload = indexed_extref_payload(9);
    let file = prt_with_named_payloads(&[("/Root/ExternalReferences", payload)]);
    let container =
        crate::test_support::with_decode_context(|ctx| container::scan_bytes(ctx, file.as_slice()))
            .expect("external reference container");
    let index_item = std::mem::size_of::<container::ExtrefIndexedRecord>();
    let output_item = std::mem::size_of::<(&DirEntry, container::ExtrefIndexedRecord)>();
    let final_capacity = 16;
    let previous_capacity = 8;
    let result_slots = final_capacity * output_item;
    // At the ninth append, reserve_scoped_vec keeps the old eight slots,
    // admits eight new slots, and accounts the old allocation during its move.
    let peak = 3 * previous_capacity * output_item + 9 * index_item;

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            policy.limits.max_materialized_bytes = cadmpeg_core::decode::u64_from_index(peak);
        },
        |ctx| {
            let (records, storage) = container
                .external_reference_indexed_records(ctx)
                .expect("nine indexed records fit the exact growth peak");
            assert_eq!(records.len(), 9);
            let refusal = ctx
                .reserve_scoped(
                    cadmpeg_core::decode::u64_from_index(peak - result_slots + 1),
                    "probe indexed external reference result lifetime",
                )
                .expect_err("the outer record allocation remains scoped until drop");
            assert!(matches!(refusal, CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::MaterializedBytes
                    && limit.used == cadmpeg_core::decode::u64_from_index(result_slots)
                    && limit.additional == cadmpeg_core::decode::u64_from_index(peak - result_slots + 1)));
            drop(records);
            drop(storage);
        },
    );
}

#[test]
fn external_reference_indexed_projection_refuses_peak_reallocation_overlap() {
    let payload = indexed_extref_payload(9);
    let file = prt_with_named_payloads(&[("/Root/ExternalReferences", payload)]);
    let container =
        crate::test_support::with_decode_context(|ctx| container::scan_bytes(ctx, file.as_slice()))
            .expect("external reference container");
    let index_item = std::mem::size_of::<container::ExtrefIndexedRecord>();
    let output_item = std::mem::size_of::<(&DirEntry, container::ExtrefIndexedRecord)>();
    let previous_capacity = 8;
    let peak = 3 * previous_capacity * output_item + 9 * index_item;

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            policy.limits.max_materialized_bytes = cadmpeg_core::decode::u64_from_index(peak - 1);
        },
        |ctx| {
            let error = container
                .external_reference_indexed_records(ctx)
                .expect_err("the final growth must admit old and new storage overlap");
            assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::MaterializedBytes
                && limit.operation == "nx external reference indexed entries"
                && limit.limit == cadmpeg_core::decode::u64_from_index(peak - 1)
                && limit.used == cadmpeg_core::decode::u64_from_index(
                    2 * previous_capacity * output_item + 9 * index_item
                )
                && limit.additional == cadmpeg_core::decode::u64_from_index(
                    previous_capacity * output_item
                )));
        },
    );
}

#[test]
fn external_reference_record_handles_remain_scoped_until_native_projection() {
    let payload = crate::test_support::test_streams::external_reference_stream();
    let file = prt_with_named_payloads(&[("/Root/ExternalReferences", payload)]);
    let container =
        crate::test_support::with_decode_context(|ctx| container::scan_bytes(ctx, file.as_slice()))
            .expect("external reference container");

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            policy.limits.max_retained_bytes = 0;
            policy.limits.max_materialized_bytes = 1024;
        },
        |ctx| {
            let (records, storage, handles_storage) = container
                .external_reference_records(ctx)
                .expect("parsed handles remain scoped through native projection");
            assert_eq!(records.len(), 1);
            assert_eq!(records[0].1.handles.serialized(), [0x10, 0x20]);
            let error = ctx
                .charge_retained(1, "probe retained external reference handles")
                .expect_err("the parser does not retain its nested handle vector");
            assert!(matches!(error, CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.used == 0
                    && limit.additional == 1));
            drop(records);
            drop(storage);
            drop(handles_storage);
        },
    );
}

#[test]
fn overlapping_extref_candidates_refuse_string_scan_work() {
    let mut bytes = vec![b'A'; 32_775];
    for ordinal in 1..=127 {
        let marker = ordinal * 256;
        bytes[marker] = 1;
        bytes[marker + 1..marker + 5].copy_from_slice(&1_u32.to_le_bytes());
        bytes[marker + 5..marker + 7]
            .copy_from_slice(&u16::try_from(32_768 - marker).unwrap().to_le_bytes());
    }
    *bytes.last_mut().unwrap() = 0;
    crate::test_support::with_decode_context_over(
        &bytes,
        |policy| policy.limits.max_work_units = 100_000,
        |ctx| {
            let error = super::locate_extref_string_table(ctx, &bytes).unwrap_err();
            assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::WorkUnits
                && matches!(limit.operation, "validate NX external reference UTF-8" | "validate NX external reference controls")));
        },
    );
    crate::test_support::with_decode_context_over(
        &bytes,
        |_| {},
        |ctx| {
            assert_eq!(
                super::locate_extref_string_table(ctx, &bytes).unwrap(),
                None
            );
        },
    );
}

#[test]
fn bounded_entry_reads_refuse_directory_work() {
    let file = single_part_prt();
    let container =
        crate::test_support::with_decode_context(|ctx| super::scan_bytes(ctx, file)).unwrap();
    let (offset, len) = container.entries[0].file_span().unwrap();
    for tail in [false, true] {
        crate::test_support::with_decode_context_over(
            &[],
            |policy| policy.limits.max_work_units = 0,
            |ctx| {
                let error = if tail {
                    container.bounded_entry_tail(ctx, offset)
                } else {
                    container.bounded_entry_bytes(ctx, offset, len)
                }
                .unwrap_err();
                assert!(matches!(error, CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::WorkUnits && limit.operation == if tail { "bound NX directory tail" } else { "bound NX directory entry" }));
            },
        );
    }
}

#[test]
fn directory_counts_refuse_exhausted_scan_work() {
    let container =
        crate::test_support::with_decode_context(|ctx| super::scan_bytes(ctx, single_part_prt()))
            .unwrap();
    crate::test_support::with_decode_context_over(
        &[],
        |policy| policy.limits.max_work_units = 0,
        |ctx| {
            let error = container.entry_count(ctx, Region::Header).unwrap_err();
            assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::WorkUnits && limit.operation == "count NX directory entries"));
        },
    );
}

#[test]
fn external_reference_routes_refuse_unadmitted_names() {
    let container =
        crate::test_support::with_decode_context(|ctx| super::scan_bytes(ctx, single_part_prt()))
            .unwrap();
    for route in 0..4 {
        crate::test_support::with_decode_context_over(
            &[],
            |policy| policy.limits.max_work_units = 1,
            |ctx| {
                let error = match route {
                    0 => container.has_external_references(ctx).err(),
                    1 => container.external_reference_strings(ctx).err(),
                    2 => container.external_reference_records(ctx).err(),
                    _ => container.external_reference_indexed_records(ctx).err(),
                }
                .expect("name scan must refuse");
                assert!(
                    matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::WorkUnits && limit.operation == "scan NX external reference names")
                );
            },
        );
    }
}

#[test]
fn external_reference_table_refuses_validation_without_copying_strings() {
    let text = "A".repeat(1000);
    let mut bytes = vec![1];
    bytes.extend(1u32.to_le_bytes());
    bytes.extend(1000u16.to_le_bytes());
    bytes.extend(text.as_bytes());
    let operation = "read NX external reference UTF-8";
    let error = crate::test_support::resource_refusal_at(
        &bytes,
        ResourceDimension::WorkUnits,
        operation,
        |ctx| super::parse_extref_string_table(ctx, &bytes).map(|_| ()),
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit) if limit.operation == operation));
    crate::test_support::with_decode_context_over(
        &bytes,
        |_| {},
        |ctx| {
            let (_, values) = super::parse_extref_string_table(ctx, &bytes)
                .map(|table| table.map(|(table, _storage)| table))
                .unwrap()
                .unwrap();
            assert_eq!(values, [(7, text.as_str())]);
        },
    );
}

#[test]
fn cached_section_reader_traversal_refuses_at_its_named_boundary() {
    let file = crate::test_support::test_prt::prt_with_size_framed_om_section();
    let container =
        crate::test_support::with_decode_context(|ctx| super::scan_bytes(ctx, &file)).unwrap();
    crate::test_support::with_decode_context(|ctx| {
        assert!(!container
            .om_sections(ctx)
            .map(|(sections, _storage)| sections)
            .unwrap()
            .is_empty());
    });
    crate::test_support::resource_refusal_at(
        &[],
        ResourceDimension::WorkUnits,
        "visit NX framed cached section readers",
        |ctx| {
            container
                .om_sections(ctx)
                .map(|(sections, _storage)| sections)
        },
    );
}

#[test]
fn external_reference_table_storage_is_scoped_and_text_is_borrowed() {
    let payload = b"prefix\x01\x01\x00\x00\x00\x09\x00child.prt";
    let slot_bytes = cadmpeg_core::decode::u64_from_index(std::mem::size_of::<(usize, &str)>());
    crate::test_support::with_decode_context_over(
        payload,
        |policy| {
            policy.limits.max_retained_bytes = 0;
            policy.limits.max_materialized_bytes = slot_bytes;
        },
        |ctx| {
            let ((marker, strings), storage) = super::parse_extref_string_table(ctx, payload)
                .unwrap()
                .unwrap();
            assert_eq!(marker, 6);
            assert_eq!(strings, [(13, "child.prt")]);
            assert_eq!(strings[0].1.as_ptr(), payload[13..].as_ptr());
            drop(strings);
            drop(storage);
            ctx.reserve_scoped(slot_bytes, "reused external reference table scratch")
                .unwrap();
        },
    );
}

#[test]
fn external_reference_projection_keeps_borrowed_text_under_its_scope() {
    let payload = b"prefix\x01\x01\x00\x00\x00\x09\x00child.prt";
    let container = external_reference_path_container(payload);
    crate::test_support::with_decode_context_over(
        payload,
        |policy| policy.limits.max_retained_bytes = 0,
        |ctx| {
            let (strings, _storage) = container.external_reference_strings(ctx).unwrap();
            assert_eq!(strings.len(), 1);
            assert_eq!(strings[0].1, 13);
            assert_eq!(strings[0].2, "child.prt");
            assert_eq!(strings[0].2.as_ptr(), payload[13..].as_ptr());
        },
    );
    crate::test_support::resource_refusal_at(
        payload,
        ResourceDimension::MaterializedBytes,
        "nx external reference strings",
        |ctx| container.external_reference_strings(ctx).map(|_| ()),
    );
}

#[test]
fn cached_section_reader_vectors_are_scoped_and_reusable() {
    let framed_file = crate::test_support::test_prt::prt_with_size_framed_om_section();
    let indexed_file = prt_with_indexed_om_section();
    let framed =
        crate::test_support::with_decode_context(|ctx| super::scan_bytes(ctx, &framed_file))
            .unwrap();
    let indexed =
        crate::test_support::with_decode_context(|ctx| super::scan_bytes(ctx, &indexed_file))
            .unwrap();
    crate::test_support::with_decode_context(|ctx| {
        framed.om_sections(ctx).unwrap();
        indexed.indexed_om_sections(ctx).unwrap();
    });
    let framed_bytes = cadmpeg_core::decode::u64_from_index(std::mem::size_of::<(
        crate::container::entry_ref::EntryRef<'_>,
        crate::om::Section<'_>,
    )>());
    let indexed_bytes = cadmpeg_core::decode::u64_from_index(std::mem::size_of::<(
        crate::container::entry_ref::EntryRef<'_>,
        crate::om::IndexedSection<'_>,
    )>());
    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            policy.limits.max_retained_bytes = 0;
            policy.limits.max_materialized_bytes = framed_bytes.max(indexed_bytes);
        },
        |ctx| {
            for _ in 0..3 {
                let (readers, storage) = framed.om_sections(ctx).unwrap();
                assert_eq!(readers.len(), 1);
                drop(readers);
                drop(storage);
                let (readers, storage) = indexed.indexed_om_sections(ctx).unwrap();
                assert_eq!(readers.len(), 1);
                drop(readers);
                drop(storage);
            }
        },
    );
    for (container, operation, is_indexed) in [
        (&framed, "NX framed section readers", false),
        (&indexed, "NX indexed section readers", true),
    ] {
        crate::test_support::resource_refusal_at(
            &[],
            ResourceDimension::MaterializedBytes,
            operation,
            |ctx| {
                if is_indexed {
                    container.indexed_om_sections(ctx).map(|_| ())
                } else {
                    container.om_sections(ctx).map(|_| ())
                }
            },
        );
    }
}

#[test]
fn external_reference_output_copy_refuses_at_its_named_work_boundary() {
    let text = "A".repeat(1000);
    let mut bytes = vec![1];
    bytes.extend(1_u32.to_le_bytes());
    bytes.extend(1000_u16.to_le_bytes());
    bytes.extend(text.as_bytes());
    let container = external_reference_path_container(&bytes);
    crate::test_support::with_decode_context(|ctx| {
        let (paths, _storage) = container.external_reference_paths(ctx).unwrap();
        assert_eq!(paths, [text]);
    });
    crate::test_support::resource_refusal_at(
        &bytes,
        ResourceDimension::WorkUnits,
        "nx external reference string",
        |ctx| container.external_reference_paths(ctx).map(|_| ()),
    );
}
