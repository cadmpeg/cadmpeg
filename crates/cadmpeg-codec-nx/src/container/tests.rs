// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]

use crate::test_support::test_cfb::legacy_cfb_with_two_streams;
use crate::test_support::test_om::offset_only_indexed_om_section_with_index_values;
use crate::test_support::test_om::segment_index_payload;
use crate::test_support::test_om::size_framed_om_section_with_repeated_operations;
use crate::test_support::test_prt::append_rmfastload_table;
use crate::test_support::test_prt::damaged_arrangements_prt;
use crate::test_support::test_prt::damaged_part_attrs_prt;
use crate::test_support::test_prt::damaged_qaf_catalog_prt;
use crate::test_support::test_prt::footer_directory_entries_prt;
use crate::test_support::test_prt::footer_partial_cut_prt;
use crate::test_support::test_prt::prt_with_indexed_om_section;
use crate::test_support::test_prt::prt_with_named_payloads;
use crate::test_support::test_prt::rmfastload_prt;
use crate::test_support::test_prt::single_part_prt;
use crate::test_support::test_prt::truncated_om_section_prt;
use crate::test_support::test_prt::without_footer;
use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use cadmpeg_core::decode::InspectOptions;

use crate::container;
use crate::container::{test_modern_layout, Container, ContainerLayout, DirEntry, Region};
use crate::NxCodec;

#[test]
fn ug_part_segment_index_uses_row_one_self_boundary() {
    let file = prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", segment_index_payload())]);
    let container = container::scan_bytes(file).unwrap();
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
    let c = container::scan_bytes(single_part_prt()).unwrap();
    assert_eq!(c.layout.version(), 0x06);
    let ContainerLayout::Modern {
        file_tag,
        footer: Some(footer),
        ..
    } = c.layout
    else {
        panic!("SPLMSSTR input must have modern layout facts and a FOOTER region");
    };
    assert_eq!(c.entry_count(Region::Header), 1);
    assert_eq!(file_tag, 0x33_22_11);
    assert_eq!(c.entry_count(Region::Footer), 0);
    assert_eq!(footer.fingerprint, [0; 4]);
    assert!(c
        .entries
        .iter()
        .any(|e| e.name == "/Root/UG_PART/UG_PART" && e.file_span().is_some()));
}

#[test]
fn legacy_scan_admits_two_stream_directory() {
    let file = legacy_cfb_with_two_streams();
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
        indexed_section_layouts: std::sync::OnceLock::new(),
        om_section_cache: std::sync::OnceLock::new(),
    };
    assert_eq!(container.bounded_entry_bytes(1, 2), Some(&payload[1..3]));
    assert_eq!(container.bounded_entry_bytes(1, 3), None);
    assert_eq!(container.bounded_entry_bytes(3, 3), Some(&payload[3..6]));
    assert_eq!(container.bounded_entry_tail(1), Some(&payload[1..3]));
    assert_eq!(container.bounded_entry_tail(4), Some(&payload[4..6]));
    assert_eq!(container.bounded_entry_tail(6), None);
}

#[test]
fn container_cached_operation_labels_preserve_section_materialization() {
    let payload = size_framed_om_section_with_repeated_operations(2);
    let container = Container {
        data: payload.as_slice().into(),
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
        indexed_section_layouts: std::sync::OnceLock::new(),
        om_section_cache: std::sync::OnceLock::new(),
    };
    let direct = crate::om::sections(&payload).unwrap();
    let cached = container.om_sections().unwrap();
    assert_eq!(cached.len(), direct.len());
    assert!(container.om_section_cache.get().is_some());
    for ((entry, section), expected) in cached.iter().zip(direct.iter()) {
        assert_eq!(entry.name, "/Root/om");
        assert_eq!(section, expected);
        assert_eq!(section.operation_labels(), expected.operation_labels());
        assert_eq!(
            section.operation_records_with_label_ordinals(),
            expected.operation_records_with_label_ordinals()
        );
    }
    let repeated = container.om_sections().unwrap();
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
    let container = Container {
        data: file.into(),
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
        indexed_section_layouts: std::sync::OnceLock::new(),
        om_section_cache: std::sync::OnceLock::new(),
    };
    let first = container.om_sections().unwrap();
    let second = container.om_sections().unwrap();
    assert_eq!(first.len(), 1);
    assert_eq!(second, first);
    assert_eq!(
        first[0].1,
        crate::om::sections(&container.data[17..]).unwrap()[0]
    );
    assert!(container.om_section_cache.get().is_some_and(|cache| {
        matches!(
            cache,
            container::FramedSectionCache::Owned { layouts, .. } if layouts.len() == 1
        )
    }));
}

#[test]
fn container_reuses_materialized_indexed_sections_for_borrowed_input() {
    let file = prt_with_indexed_om_section();
    let container = container::scan_bytes(file.as_slice()).unwrap();
    let first = container.indexed_om_sections();
    let second = container.indexed_om_sections();
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
    let container = container::scan_bytes(file.as_slice()).unwrap();
    let _ = container.indexed_om_sections();
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
    let container = container::scan_bytes(file).expect("one entry in each counted region");
    assert_eq!(container.entry_count(Region::Header), 1);
    assert_eq!(container.entry_count(Region::Footer), 1);
    assert_eq!(container.entries.len(), 2);
}

#[test]
fn container_salvages_intact_footer_past_an_overcounted_header() {
    // A HEADER count of two with one serialized entry is directory damage,
    // not a cut: the surviving entry frames, and the independently readable
    // footer region is salvaged, so the lie costs nothing decodable and no
    // part is given up (#215 §4.5).
    let mut header = single_part_prt();
    header[0x1f..0x23].copy_from_slice(&2_u32.to_le_bytes());
    let container =
        container::scan_bytes(header.clone()).expect("the surviving entry still frames");
    assert_eq!(container.entries.len(), 1);
    let ContainerLayout::Modern {
        footer: Some(_), ..
    } = container.layout
    else {
        panic!("an intact footer region is salvaged");
    };
    let decoded = NxCodec
        .decode(&mut Cursor::new(header), &DecodeOptions::default())
        .expect("an overcounted header still decodes");
    assert!(
        unreadable_losses(&decoded).is_empty(),
        "nothing is missing, so nothing is given up: {:#?}",
        decoded.report().losses
    );

    // A FOOTER count of one with no serialized entry is damage to that
    // region: the HEADER entries frame, and the rest of the stream is one
    // `nx.footer_directory` part give-up.
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
    let container = container::scan_bytes(footer.clone()).expect("the header entries still frame");
    let ContainerLayout::Modern { footer: None, .. } = container.layout else {
        panic!("a damaged footer region is not salvaged");
    };
    let decoded = NxCodec
        .decode(&mut Cursor::new(footer.clone()), &DecodeOptions::default())
        .expect("a damaged footer gives up its region");
    assert_one_part_give_up(
        &decoded,
        &footer,
        "nx.footer_directory",
        cadmpeg_core::decode::u64_from_index(footer_offset),
        &footer[footer_offset..],
    );
}

#[test]
fn container_gives_up_trailing_or_overlapping_footer_data() {
    // One trailing byte past the fingerprint is damage to the footer
    // region: the HEADER entries frame, and the rest of the stream is one
    // `nx.footer_directory` part give-up (#215 §4.5).
    let mut trailing = single_part_prt();
    trailing.push(0);
    let footer_offset = usize::try_from(u64::from_le_bytes([
        trailing[0x11],
        trailing[0x12],
        trailing[0x13],
        trailing[0x14],
        trailing[0x15],
        trailing[0x16],
        0,
        0,
    ]))
    .expect("synthetic footer offset");
    let decoded = NxCodec
        .decode(
            &mut Cursor::new(trailing.clone()),
            &DecodeOptions::default(),
        )
        .expect("trailing footer data gives up its region");
    assert_one_part_give_up(
        &decoded,
        &trailing,
        "nx.footer_directory",
        cadmpeg_core::decode::u64_from_index(footer_offset),
        &trailing[footer_offset..],
    );

    // A directory span reaching one byte into the FOOTER region is damage
    // to that region: the entry frames, and the footer region is one
    // `nx.footer_directory` part give-up.
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
    let decoded = NxCodec
        .decode(&mut Cursor::new(overlap.clone()), &DecodeOptions::default())
        .expect("overlapping footer data gives up its region");
    let footer_offset = usize::try_from(footer_offset).expect("the footer offset fits usize");
    assert_one_part_give_up(
        &decoded,
        &overlap,
        "nx.footer_directory",
        cadmpeg_core::decode::u64_from_index(footer_offset),
        &overlap[footer_offset..],
    );
}

#[test]
fn container_gives_up_cut_footer_tail() {
    // Port of `container_rejects_footer_offset_beyond_the_file_image` under
    // #215 §4.5: a FOOTER offset beyond the image no longer refuses the
    // file. The HEADER entries frame, and the tail the cut source cannot
    // supply is one part give-up, which asserts more than the old refusal.
    //
    // A bogus offset over an intact image decodes whole with no give-up:
    // nothing is missing, so nothing is given up.
    let mut bogus = single_part_prt();
    let beyond = cadmpeg_core::decode::u64_from_index(bogus.len() + 1);
    bogus[0x11..0x17].copy_from_slice(&beyond.to_le_bytes()[..6]);
    let container =
        container::scan_bytes(bogus.clone()).expect("cut footer still frames its entries");
    assert!(container.cut_tails().is_empty());
    let whole = NxCodec
        .decode(&mut Cursor::new(bogus), &DecodeOptions::default())
        .expect("bogus FOOTER offset over an intact image decodes");
    assert!(
        !whole
            .report()
            .losses
            .iter()
            .any(|loss| loss.code.local_code() == "record.unreadable"),
        "nothing is missing, so nothing is given up: {:#?}",
        whole.report().losses
    );

    // The same image cut at half length gives up its entry tail as one part.
    // The payload starts at 76: 35 + 4 + len("/Root/UG_PART/UG_PART") + 16.
    let full = single_part_prt();
    let cut_len = full.len() / 2;
    let mut cut = full[..cut_len].to_vec();
    let beyond = cadmpeg_core::decode::u64_from_index(full.len() + 1);
    cut[0x11..0x17].copy_from_slice(&beyond.to_le_bytes()[..6]);
    let container =
        container::scan_bytes(cut.clone()).expect("cut footer still frames its entries");
    assert_eq!(
        container.cut_tails(),
        vec![(76, cadmpeg_core::decode::u64_from_index(cut_len))]
    );
    let decoded = NxCodec
        .decode(&mut Cursor::new(cut.clone()), &DecodeOptions::default())
        .expect("cut footer gives up its tail");
    let unreadable = decoded
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code.local_code() == "record.unreadable")
        .collect::<Vec<_>>();
    assert_eq!(unreadable.len(), 1, "{:#?}", decoded.report().losses);
    assert!(
        unreadable[0].message.contains("nx.cut_tail"),
        "{}",
        unreadable[0].message
    );
    assert_eq!(unreadable[0].severity, cadmpeg_ir::report::Severity::Error);
    // One home: exactly one part stub retains exactly the tail bytes.
    let stubs = decoded
        .ir()
        .native
        .namespace("nx")
        .expect("nx namespace")
        .clone()
        .arena_as::<cadmpeg_ir::NativeUnknownRecord>("unknowns")
        .expect("unknowns arena");
    let part_stubs = stubs
        .iter()
        .filter(|stub| stub.id.as_str().starts_with("nx:source:part#"))
        .collect::<Vec<_>>();
    assert_eq!(part_stubs.len(), 1);
    let tails = decoded
        .source_fidelity()
        .retained_records()
        .iter()
        .filter(|(_, record)| record.data().is_some_and(|data| data == &cut[76..cut_len]))
        .collect::<Vec<_>>();
    assert_eq!(tails.len(), 1);
    assert_eq!(tails[0].1.offset(), 76);
    assert_eq!(tails[0].0.as_str(), part_stubs[0].id.as_str());
}

/// Collect the `record.unreadable` losses of one decode.
fn unreadable_losses(
    decoded: &cadmpeg_ir::codec::DecodeResult,
) -> Vec<&cadmpeg_ir::report::loss::LossNote> {
    decoded
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code.local_code() == "record.unreadable")
        .collect()
}

/// The `nx:source:part#` stubs of one decode, with their retained bytes.
fn part_home(
    decoded: &cadmpeg_ir::codec::DecodeResult,
) -> (Vec<String>, Vec<(String, u64, Vec<u8>)>) {
    let stubs = decoded
        .ir()
        .native
        .namespace("nx")
        .expect("nx namespace")
        .clone()
        .arena_as::<cadmpeg_ir::NativeUnknownRecord>("unknowns")
        .expect("unknowns arena");
    let ids = stubs
        .iter()
        .filter(|stub| stub.id.as_str().starts_with("nx:source:part#"))
        .map(|stub| stub.id.as_str().to_owned())
        .collect();
    let retained = decoded
        .source_fidelity()
        .retained_records()
        .iter()
        .map(|(id, record)| {
            (
                id.as_str().to_owned(),
                record.offset(),
                record.data().unwrap_or_default().to_vec(),
            )
        })
        .collect();
    (ids, retained)
}

/// Assert one part give-up: a single Error `record.unreadable` naming `kind`,
/// with exactly one stub retaining exactly `expected` at `offset`.
fn assert_one_part_give_up(
    decoded: &cadmpeg_ir::codec::DecodeResult,
    bytes: &[u8],
    kind: &str,
    offset: u64,
    expected: &[u8],
) {
    let unreadable = unreadable_losses(decoded);
    assert_eq!(unreadable.len(), 1, "{:#?}", decoded.report().losses);
    assert!(
        unreadable[0].message.contains(kind),
        "{}",
        unreadable[0].message
    );
    assert_eq!(unreadable[0].severity, cadmpeg_ir::report::Severity::Error);
    let (stubs, retained) = part_home(decoded);
    assert_eq!(stubs.len(), 1);
    let kept = retained
        .iter()
        .filter(|(_, record_offset, data)| *record_offset == offset && data.as_slice() == expected)
        .collect::<Vec<_>>();
    assert_eq!(kept.len(), 1, "one stub retains exactly the part bytes");
    assert_eq!(kept[0].0, stubs[0]);
    assert_eq!(
        kept[0].1,
        u64::try_from(
            bytes
                .windows(expected.len())
                .position(|window| window == expected)
                .expect("the part bytes occur in the source")
        )
        .expect("the source offset fits u64")
    );
}

/// Payload bytes of one named directory entry in a scanned image.
fn entry_payload<'a>(bytes: &'a [u8], name: &str) -> (u64, &'a [u8]) {
    let container = container::scan_bytes(bytes).expect("the probe image scans");
    let (offset, size) = container
        .entries
        .iter()
        .find(|entry| entry.name == name)
        .and_then(crate::container::DirEntry::file_span)
        .expect("the named entry");
    let (start, size) = (
        usize::try_from(offset).expect("the entry offset fits usize"),
        usize::try_from(size).expect("the entry size fits usize"),
    );
    (
        offset,
        &bytes[start..start.checked_add(size).expect("the entry span fits")],
    )
}

#[test]
fn damaged_arrangements_give_up_configurations_part() {
    // The second arrangement misses its `Default` attribute: the entry is
    // present but unreadable, so the decode gives up its span as one
    // `nx.configurations` part instead of dropping the configurations
    // silently (#215 §4.5). The intact attribute peer still attaches.
    let bytes = damaged_arrangements_prt();
    let (offset, payload) = entry_payload(&bytes, "/Root/part/arrangements");
    let decoded = NxCodec
        .decode(&mut Cursor::new(bytes.clone()), &DecodeOptions::default())
        .expect("damaged arrangements give up their part");
    assert_one_part_give_up(&decoded, &bytes, "nx.configurations", offset, payload);
    assert!(decoded.ir().model.configurations.is_empty());
    assert_eq!(decoded.ir().model.attributes.len(), 1);
}

#[test]
fn damaged_part_attrs_give_up_part_attributes_part() {
    // The attribute misses its `type` attribute: the entry is present but
    // unreadable, so the decode gives up its span as one
    // `nx.part_attributes` part instead of dropping the attributes silently
    // (#215 §4.5). The intact arrangement peer still attaches.
    let bytes = damaged_part_attrs_prt();
    let (offset, payload) = entry_payload(&bytes, "/Root/part/attrs");
    let decoded = NxCodec
        .decode(&mut Cursor::new(bytes.clone()), &DecodeOptions::default())
        .expect("damaged attributes give up their part");
    assert_one_part_give_up(&decoded, &bytes, "nx.part_attributes", offset, payload);
    assert_eq!(decoded.ir().model.configurations.len(), 2);
    assert!(decoded.ir().model.attributes.is_empty());
}

#[test]
fn damaged_qaf_catalog_gives_up_material_texture_catalog_part() {
    // The material property misses its `unmappedLocation` attribute: the
    // entry is present but unreadable, so the decode gives up its span as
    // one `nx.material_texture_catalog` part instead of dropping the catalog
    // silently (#215 §4.5). The intact texture asset peer is still retained.
    let bytes = damaged_qaf_catalog_prt();
    let (offset, payload) = entry_payload(&bytes, "/Root/qafmetadata");
    let decoded = NxCodec
        .decode(&mut Cursor::new(bytes.clone()), &DecodeOptions::default())
        .expect("damaged catalog gives up its part");
    assert_one_part_give_up(
        &decoded,
        &bytes,
        "nx.material_texture_catalog",
        offset,
        payload,
    );
    let catalog: Vec<serde_json::Value> = decoded
        .ir()
        .native
        .namespace("nx")
        .expect("nx namespace")
        .clone()
        .arena_as("material_texture_catalog_entries")
        .expect("catalog arena");
    assert!(catalog.is_empty());
    let assets: Vec<serde_json::Value> = decoded
        .ir()
        .native
        .namespace("nx")
        .expect("nx namespace")
        .clone()
        .arena_as("material_texture_assets")
        .expect("asset arena");
    assert_eq!(assets.len(), 1);
}

#[test]
fn truncated_om_section_gives_up_om_section_part() {
    // The section declares its end past its entry payload: the
    // section-structure parse skips it, so the decode gives up its span as
    // one `nx.om_section` part instead of dropping the section's records
    // silently (#215 §4.5).
    let bytes = truncated_om_section_prt();
    let (offset, payload) = entry_payload(&bytes, "/Root/UG_PART/UG_PART");
    let decoded = NxCodec
        .decode(&mut Cursor::new(bytes.clone()), &DecodeOptions::default())
        .expect("truncated section gives up its part");
    let spans = container::scan_bytes(bytes.clone())
        .expect("the probe image scans")
        .damaged_om_section_spans()
        .expect("truncated spans");
    assert_eq!(spans.len(), 1);
    assert_eq!(spans[0].0, offset);
    assert_eq!(
        spans[0].1,
        offset + u64::try_from(payload.len()).expect("span fits u64")
    );
    assert_one_part_give_up(&decoded, &bytes, "nx.om_section", offset, payload);
}

#[test]
fn footer_partial_cut_gives_up_footer_region() {
    // The FOOTER offset lands inside the image with an incomplete directory:
    // the HEADER entries frame, and the rest of the stream is one
    // `nx.footer_directory` part give-up (#215 §4.5).
    let bytes = footer_partial_cut_prt();
    let decoded = NxCodec
        .decode(&mut Cursor::new(bytes.clone()), &DecodeOptions::default())
        .expect("partial footer gives up its region");
    let mut footer_offset = [0; 8];
    footer_offset[..6].copy_from_slice(&bytes[0x11..0x17]);
    let footer_offset = u64::from_le_bytes(footer_offset);
    assert_one_part_give_up(
        &decoded,
        &bytes,
        "nx.footer_directory",
        footer_offset,
        &bytes[usize::try_from(footer_offset).expect("the footer offset fits usize")..],
    );
}

#[test]
fn footer_directory_entries_decode_whole() {
    // A FOOTER region with entries still parses whole: nothing is missing,
    // so nothing is given up.
    let bytes = footer_directory_entries_prt();
    let decoded = NxCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("footer entries decode");
    assert!(
        unreadable_losses(&decoded).is_empty(),
        "{:#?}",
        decoded.report().losses
    );
}

#[test]
fn container_reads_the_header_directory_alone_without_a_footer_region() {
    let footed = container::scan_bytes(single_part_prt()).unwrap();
    let footerless = container::scan_bytes(without_footer(single_part_prt())).unwrap();
    let ContainerLayout::Modern {
        file_tag, footer, ..
    } = footerless.layout
    else {
        panic!("SPLMSSTR input must have modern layout facts");
    };
    assert_eq!(file_tag, 0x33_22_11);
    assert_eq!(footer, None);
    assert_eq!(footerless.entry_count(Region::Footer), 0);
    let span = |c: &Container<'_>| {
        c.entries
            .iter()
            .find(|e| e.name == "/Root/UG_PART/UG_PART")
            .and_then(DirEntry::file_span)
    };
    assert!(span(&footerless).is_some());
    assert_eq!(span(&footerless), span(&footed));
}

#[test]
fn container_reads_rmfastload_active_ids() {
    let container = container::scan_bytes(rmfastload_prt()).unwrap();
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
fn container_reads_rmfastload_table_from_product_boundary_without_range_floor() {
    let mut payload = b"UGS::Solid::Topol".to_vec();
    append_rmfastload_table(&mut payload, [0, u32::MAX, 7]);
    let file = prt_with_named_payloads(&[("/Root/FastLoad/RMFastLoad", payload)]);
    let container = container::scan_bytes(file).unwrap();
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
    assert!(container::scan_bytes(bytes.to_vec()).is_err());
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
    let container = container::scan_bytes(file).unwrap();
    let (_, table) = container
        .rmfastload_object_id_table()
        .expect("first product-bounded table");
    assert_eq!(table.object_ids.as_slice().to_vec(), [1, 2, 3]);
}

#[test]
fn external_reference_string_table_is_end_anchored() {
    let table = b"prefix\x01\x02\x00\x00\x00\x09\x00child.prt\x0c\x00nested/b.prt";
    let (_, strings) = crate::container::parse_extref_string_table(table).expect("string table");
    assert_eq!(
        strings
            .into_iter()
            .map(|(_, value)| value)
            .collect::<Vec<_>>(),
        ["child.prt", "nested/b.prt"]
    );

    let mut trailed = table.to_vec();
    trailed.push(0);
    assert!(crate::container::parse_extref_string_table(&trailed).is_none());
    assert!(crate::container::parse_extref_string_table(b"\x01\xff\xff\xff\xff").is_none());
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

    let records = crate::container::parse_extref_records(&payload);
    let indexed = crate::container::parse_extref_record_index(&payload).expect("record index");
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

    let duplicate = payload
        .windows(5)
        .rposition(|window| window == [0xe0, 0x20, 0x30, 0x40, 0x50])
        .expect("closing duplicate");
    payload[duplicate + 1] = 0x10;
    assert!(crate::container::parse_extref_records(&payload).is_empty());
    assert_eq!(
        crate::container::parse_extref_record_index(&payload)
            .expect("opaque indexed record")
            .len(),
        1
    );
}

#[test]
fn overlapping_extref_candidates_are_rejected() {
    let mut bytes = vec![b'A'; 32_775];
    for ordinal in 1..=127 {
        let marker = ordinal * 256;
        bytes[marker] = 1;
        bytes[marker + 1..marker + 5].copy_from_slice(&1_u32.to_le_bytes());
        bytes[marker + 5..marker + 7]
            .copy_from_slice(&u16::try_from(32_768 - marker).unwrap().to_le_bytes());
    }
    *bytes.last_mut().unwrap() = 0;
    assert_eq!(super::locate_extref_string_table(&bytes), None);
}

#[test]
fn overlapping_extref_candidates_resolve_in_one_pass() {
    // Every eighth byte starts a one-entry table whose string runs to the
    // payload end, so the candidates declare about 2^28 string bytes.
    let mut bytes = vec![b'A'; 65_543];
    let mut last_marker = 0;
    for marker in (8..bytes.len() - 8).step_by(8) {
        bytes[marker] = 1;
        bytes[marker + 1..marker + 5].copy_from_slice(&1_u32.to_le_bytes());
        let length = u16::try_from(bytes.len() - marker - 7).unwrap();
        bytes[marker + 5..marker + 7].copy_from_slice(&length.to_le_bytes());
        last_marker = marker;
    }
    assert_eq!(
        super::locate_extref_string_table(&bytes),
        Some((last_marker, 1, last_marker + 5))
    );
    *bytes.last_mut().unwrap() = 0;
    assert_eq!(super::locate_extref_string_table(&bytes), None);

    let file = prt_with_named_payloads(&[("/Root/ExternalReferences", bytes)]);
    let result = NxCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .unwrap();
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == crate::loss::NxLossCode::SourceScanBounded.kind()));
}

/// The search the one-pass entry count replaced: walk the entries after each
/// marker, last marker first.
fn per_marker_extref_string_table(payload: &[u8]) -> Option<(usize, usize, usize)> {
    use cadmpeg_core::decode::{bounded_len, View};

    let last_marker = payload.len().checked_sub(4)?;
    for marker in (0..last_marker).rev() {
        if payload[marker] != 1 {
            continue;
        }
        let Some(count) = View::u32_le_at(payload, marker + 1) else {
            continue;
        };
        let start = marker + 5;
        let Some(count) = bounded_len(u64::from(count), 3, payload.len() - start) else {
            continue;
        };
        let mut pos = start;
        let valid = (0..count).all(|_| {
            let end = (|| {
                let length = usize::from(View::u16_le_at(payload, pos)?);
                let end = (pos + 2).checked_add(length)?;
                let value = std::str::from_utf8(payload.get(pos + 2..end)?).ok()?;
                (!value.is_empty() && !value.chars().any(char::is_control)).then_some(end)
            })();
            end.map(|end| pos = end).is_some()
        });
        if valid && pos == payload.len() {
            return Some((marker, count, start));
        }
    }
    None
}

#[test]
fn extref_string_table_search_matches_the_per_marker_walk() {
    let valid: [&[u8]; 5] = [
        b"A",
        b"child.prt",
        b"\xc2\xb5",
        b"\xe2\x82\xac",
        b"\xf0\x9f\x98\x80",
    ];
    let invalid: [&[u8]; 9] = [
        b"\x00",
        b"\x01",
        b"\x1f",
        b"\x7f",
        b"\xc2\x85",
        b"\xff",
        b"\x80",
        b"\xed\xa0\x80",
        b"\xc0\x80",
    ];
    let mut state = 0x9e37_79b9_7f4a_7c15_u64;
    let mut next = move |bound: usize| {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        usize::try_from(state % u64::try_from(bound).unwrap()).unwrap()
    };
    let piece = move |next: &mut dyn FnMut(usize) -> usize| {
        if next(4) == 0 {
            invalid[next(invalid.len())]
        } else {
            valid[next(valid.len())]
        }
    };
    let table = |payload: &mut Vec<u8>, next: &mut dyn FnMut(usize) -> usize, long: bool| {
        let entries = next(4);
        payload.push(1);
        let declared = entries + usize::from(next(6) == 0);
        payload.extend_from_slice(&u32::try_from(declared).unwrap().to_le_bytes());
        for _ in 0..entries {
            let mut text = Vec::new();
            for _ in 0..next(4) {
                text.extend_from_slice(piece(next));
            }
            if long {
                text.resize(text.len() + 60_000 + next(5_000), b'A');
            }
            payload.extend_from_slice(&u16::try_from(text.len()).unwrap().to_le_bytes());
            payload.extend_from_slice(&text);
        }
    };

    let mut payloads = Vec::new();
    for case in 0..2_000 {
        let long = case % 250 == 0;
        let mut payload = Vec::new();
        if long {
            payload.resize(70_000, b'A');
        }
        for _ in 0..next(3) {
            for _ in 0..next(6) {
                payload.extend_from_slice(piece(&mut next));
            }
            table(&mut payload, &mut next, long);
        }
        if next(4) == 0 {
            payload.extend_from_slice(piece(&mut next));
        }
        payloads.push(payload);
    }
    let mut found = 0;
    for payload in &payloads {
        let expected = per_marker_extref_string_table(payload);
        found += usize::from(expected.is_some());
        assert_eq!(super::locate_extref_string_table(payload), expected);
    }
    assert!(found > 200 && found < payloads.len() - 200);
    assert!(payloads
        .iter()
        .any(|payload| payload.len() > super::EXTREF_ENTRY_REACH
            && per_marker_extref_string_table(payload).is_some()));
}

#[test]
fn external_reference_materialization_copies_each_string() {
    let text = "A".repeat(1000);
    let mut bytes = vec![1];
    bytes.extend(1u32.to_le_bytes());
    bytes.extend(1000u16.to_le_bytes());
    bytes.extend(text.as_bytes());
    let (_, values) = super::parse_extref_string_table(&bytes).unwrap();
    assert_eq!(values, [(7, text)]);
}
