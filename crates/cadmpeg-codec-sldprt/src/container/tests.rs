// SPDX-License-Identifier: Apache-2.0
//! Outer-container detect, scan, inspect, and partition-selection tests.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_core::decode::InspectOptions;
use cadmpeg_ir::codec::{Codec, CodecBackend, Confidence};

use crate::container::{self, role};
use crate::test_support::*;
use crate::SldprtCodec;

use super::{looks_like_compound_file, looks_like_sldprt};

#[test]
fn generic_compound_prefix_is_a_weak_container_signal() {
    let prefix = [0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1, 0, 0, 0, 0];

    assert!(looks_like_compound_file(&prefix));
    assert!(!looks_like_sldprt(&prefix));
}

fn synthetic_compound_with_storage(name: &str) -> Vec<u8> {
    const FREE: u32 = 0xffff_ffff;
    const END: u32 = 0xffff_fffe;
    const FAT: u32 = 0xffff_fffd;
    const SECTOR_SIZE: usize = 512;

    let mut file = vec![0_u8; SECTOR_SIZE * 3];
    file[..8].copy_from_slice(&[0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1]);
    file[24..26].copy_from_slice(&0x003e_u16.to_le_bytes());
    file[26..28].copy_from_slice(&3_u16.to_le_bytes());
    file[28..30].copy_from_slice(&0xfffe_u16.to_le_bytes());
    file[30..32].copy_from_slice(&9_u16.to_le_bytes());
    file[32..34].copy_from_slice(&6_u16.to_le_bytes());
    file[44..48].copy_from_slice(&1_u32.to_le_bytes());
    file[48..52].copy_from_slice(&0_u32.to_le_bytes());
    file[56..60].copy_from_slice(&4096_u32.to_le_bytes());
    file[60..64].copy_from_slice(&END.to_le_bytes());
    file[68..72].copy_from_slice(&END.to_le_bytes());
    for index in 0..109 {
        let offset = 76 + index * 4;
        file[offset..offset + 4].copy_from_slice(&FREE.to_le_bytes());
    }
    file[76..80].copy_from_slice(&1_u32.to_le_bytes());

    let directory = &mut file[SECTOR_SIZE..SECTOR_SIZE * 2];
    for entry in directory.chunks_exact_mut(128) {
        entry[68..80].fill(0xff);
    }
    write_compound_directory_entry(directory, 0, "Root Entry", 5, 1);
    write_compound_directory_entry(directory, 1, name, 1, FREE);
    let fat = &mut file[SECTOR_SIZE * 2..SECTOR_SIZE * 3];
    fat.fill(0xff);
    fat[..4].copy_from_slice(&END.to_le_bytes());
    fat[4..8].copy_from_slice(&FAT.to_le_bytes());
    file
}

fn write_compound_directory_entry(
    directory: &mut [u8],
    index: usize,
    name: &str,
    object_type: u8,
    child: u32,
) {
    const NO_STREAM: u32 = 0xffff_ffff;
    let entry = &mut directory[index * 128..(index + 1) * 128];
    let name = name.encode_utf16().collect::<Vec<_>>();
    for (offset, unit) in name.iter().enumerate() {
        entry[offset * 2..offset * 2 + 2].copy_from_slice(&unit.to_le_bytes());
    }
    entry[64..66].copy_from_slice(&((name.len() as u16 + 1) * 2).to_le_bytes());
    entry[66] = object_type;
    entry[67] = 1;
    entry[68..72].copy_from_slice(&NO_STREAM.to_le_bytes());
    entry[72..76].copy_from_slice(&NO_STREAM.to_le_bytes());
    entry[76..80].copy_from_slice(&child.to_le_bytes());
    entry[116..120].copy_from_slice(&NO_STREAM.to_le_bytes());
}

#[test]
fn detect_high_on_marker_after_header() {
    let f = synthetic_sldprt();
    assert_eq!(SldprtCodec.detect(&f), Confidence::High);
    assert_eq!(
        SldprtCodec.detect(b"\x00\x01\x02\x03 no marker here"),
        Confidence::No
    );
}

#[test]
fn compound_detection_distinguishes_solidworks_and_generic_signals() {
    let file = synthetic_compound_with_storage("ISolidWorksInformation");
    assert_eq!(SldprtCodec.detect(&file), Confidence::High);

    let generic_compound_document = [0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1];
    assert_eq!(
        SldprtCodec.detect(&generic_compound_document),
        Confidence::No
    );
}

#[test]
fn scan_classifies_blocks_cells_and_directory() {
    let f = synthetic_sldprt();
    let scan = container::scan_bytes(&f);
    assert_eq!(scan.version, 0x0000_0004);
    assert_eq!(scan.blocks.len(), 2);
    assert_eq!(scan.cache_cells.len(), 1);
    assert_eq!(scan.directory.len(), 1);

    let png = &scan.blocks[0];
    assert_eq!(png.section.as_deref(), Some("PreviewPNG"));
    assert_eq!(png.family, "png-preview");

    let ps = &scan.blocks[1];
    assert_eq!(ps.section.as_deref(), Some("Contents/Config-0-Partition"));
    assert_eq!(ps.family, "parasolid");

    assert_eq!(scan.cache_cells[0].name, "Contents/DisplayLists");
    assert_eq!(scan.cache_cells[0].logical_len, 90);
    assert_eq!(scan.directory[0].name, "[Content_Types].xml");
    assert_eq!(scan.directory[0].descriptor, [0; 14]);
    assert_eq!(scan.directory[0].trailer, [0xe5, 0x4b, 0x57, 0x5b, 0, 0]);
}

#[test]
fn parasolid_partition_selection_withholds_ambiguous_sites() {
    let source = sldprt_with_colliding_sites();
    let scan = container::scan_bytes(&source);

    assert!(container::has_parasolid_body_stream(&scan));
    assert!(container::select_active_parasolid(&scan).is_none());
}

#[test]
fn parasolid_partition_selection_uses_explicit_active_source_index() {
    let mut source = sldprt_with_colliding_sites();
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Configuration Name="First" SourceIndex="0"/><Configuration Name="Second" SourceIndex="1"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x43,
        "Contents/SolidWorks",
        br#"<swSolidWorks><swModel swConfigurationName="Second"/></swSolidWorks>"#,
    ));
    let scan = container::scan_bytes(&source);

    let (block, header) =
        container::select_active_parasolid(&scan).expect("explicit active partition");
    assert_eq!(
        block.section.as_deref(),
        Some("Contents/Config-1-Partition")
    );
    assert!(header.description.contains("partition"));
}

#[test]
fn parasolid_partition_selection_uses_the_namespaced_manifest_active_id() {
    let mut source = sldprt_with_colliding_sites();
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Configuration Name="First"/><Configuration Name="Second"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x43,
        "Contents/Features",
        br#"<?xml version="1.0"?><swSolidWorks xmlns="http://www.solidworks.com/sw2003/schema"><swModel id="model-0" swConfigurationName="First" swConfigurationId="0"/><swModel id="model-1" swConfigurationName="Second" swConfigurationId="1"/><swConfigurationList><swConfiguration swID="0" swModelRef="model-0" swMostRecentConfiguration="NO"/><swConfiguration swID="1" swModelRef="model-1" swMostRecentConfiguration="YES"/></swConfigurationList></swSolidWorks>"#,
    ));
    let scan = container::scan_bytes(&source);

    assert_eq!(
        container::manifest_active_configuration(&scan),
        Some((1, Some("Second".to_string())))
    );
    assert_eq!(
        container::active_configuration_name(&scan).as_deref(),
        Some("Second")
    );
    assert_eq!(container::active_configuration_index(&scan), Some(1));
    let (block, _) = container::select_active_parasolid(&scan).expect("manifest selects a site");
    assert_eq!(
        block.section.as_deref(),
        Some("Contents/Config-1-Partition")
    );
}

#[test]
fn parasolid_partition_selection_accepts_utf16_manifest_payloads() {
    let xml = r#"<swSolidWorks xmlns="http://www.solidworks.com/sw2003/schema"><swConfigurationList><swConfiguration swID="1" swName="Second" swMostRecentConfiguration="YES"/></swConfigurationList></swSolidWorks>"#;
    let mut payload = vec![0xff, 0xfe];
    for unit in xml.encode_utf16() {
        payload.extend_from_slice(&unit.to_le_bytes());
    }
    let mut source = sldprt_with_colliding_sites();
    source.extend(make_block(0x43, "Contents/Features", &payload));
    let scan = container::scan_bytes(&source);

    assert_eq!(container::active_configuration_index(&scan), Some(1));
    let (block, _) = container::select_active_parasolid(&scan).expect("UTF-16 manifest");
    assert_eq!(
        block.section.as_deref(),
        Some("Contents/Config-1-Partition")
    );
}

#[test]
fn explicit_source_index_precedes_the_manifest_partition_id() {
    let mut source = sldprt_with_colliding_sites();
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Configuration Name="First"/><Configuration Name="Second" SourceIndex="0"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x43,
        "Contents/Features",
        br#"<swSolidWorks xmlns="http://www.solidworks.com/sw2003/schema"><swModel id="model-0" swConfigurationName="First"/><swModel id="model-1" swConfigurationName="Second"/><swConfigurationList><swConfiguration swID="1" swModelRef="model-1" swMostRecentConfiguration="YES"/></swConfigurationList></swSolidWorks>"#,
    ));
    let scan = container::scan_bytes(&source);

    assert_eq!(container::active_configuration_index(&scan), Some(0));
    let (block, _) = container::select_active_parasolid(&scan).expect("explicit source index");
    assert_eq!(
        block.section.as_deref(),
        Some("Contents/Config-0-Partition")
    );
}

#[test]
fn non_unique_manifest_activity_does_not_select_one_of_multiple_partitions() {
    for manifest in [
        br#"<swSolidWorks xmlns="http://www.solidworks.com/sw2003/schema"><swConfigurationList><swConfiguration swID="0" swMostRecentConfiguration="NO"/><swConfiguration swID="1" swMostRecentConfiguration="NO"/></swConfigurationList></swSolidWorks>"#.as_slice(),
        br#"<swSolidWorks xmlns="http://www.solidworks.com/sw2003/schema"><swConfigurationList><swConfiguration swID="0" swMostRecentConfiguration="YES"/><swConfiguration swID="1" swMostRecentConfiguration="YES"/></swConfigurationList></swSolidWorks>"#.as_slice(),
    ] {
        let mut source = sldprt_with_colliding_sites();
        source.extend(make_block(0x43, "Contents/Features", manifest));
        let scan = container::scan_bytes(&source);
        assert_eq!(container::manifest_active_configuration(&scan), None);
        assert_eq!(container::active_configuration_index(&scan), None);
        assert!(container::select_active_parasolid(&scan).is_none());
    }
}

#[test]
fn manifest_activity_is_read_only_from_the_features_stream() {
    let mut source = sldprt_with_colliding_sites();
    source.extend(make_block(
        0x42,
        "Contents/SolidWorks",
        br#"<swSolidWorks><swConfigurationList><swConfiguration swID="1" swMostRecentConfiguration="YES"/></swConfigurationList></swSolidWorks>"#,
    ));
    let scan = container::scan_bytes(&source);

    assert_eq!(container::manifest_active_configuration(&scan), None);
    assert_eq!(container::active_configuration_index(&scan), None);
    assert!(container::select_active_parasolid(&scan).is_none());
}

#[test]
fn parasolid_partition_selection_never_uses_a_deltas_section() {
    let mut source = outer_header();
    source.extend(make_block(
        0x21,
        "Contents/Config-0-Deltas",
        &parasolid_with_body("partition body", "SCH_SW_33103_11000", &triangle_body()),
    ));
    let scan = container::scan_bytes(&source);

    assert!(container::has_parasolid_body_stream(&scan));
    assert!(container::select_active_parasolid(&scan).is_none());
}

#[test]
fn inspect_enumerates_every_structure() {
    let f = synthetic_sldprt();
    let mut cur = Cursor::new(f);
    let summary = SldprtCodec
        .inspect(&mut cur, &InspectOptions::default())
        .unwrap();
    assert_eq!(summary.format, "sldprt");
    assert_eq!(summary.container_kind, "sldprt-blocks");
    assert_eq!(
        summary
            .entries
            .iter()
            .filter(|e| e.role == role::BLOCK)
            .count(),
        2
    );
    assert!(summary.entries.iter().any(|e| e.role == role::CACHE_CELL));
    assert!(summary
        .entries
        .iter()
        .any(|e| e.role == role::DIRECTORY_ENTRY));
    assert!(summary
        .notes
        .iter()
        .any(|n| n.contains("active Parasolid B-rep candidate")));
}
