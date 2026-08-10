// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::codec::{Codec, CodecEntry, Confidence, DecodeOptions};
use cadmpeg_ir::report::LossKind;

use crate::InventorCodec;

const MAGIC: [u8; 8] = [0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1];
const FREE_SECTOR: u32 = 0xffff_ffff;
const END_OF_CHAIN: u32 = 0xffff_fffe;
const FAT_SECTOR: u32 = 0xffff_fffd;
const NO_STREAM: u32 = 0xffff_ffff;
const SECTOR_SIZE: usize = 512;

#[test]
fn detects_only_structurally_corroborated_inventor_cfb() {
    let inventor = fixture(true);
    let unrelated = fixture(false);
    assert_eq!(InventorCodec.detect(&inventor), Confidence::High);
    assert_eq!(InventorCodec.detect(&unrelated), Confidence::No);
    assert_eq!(InventorCodec.detect(b"not a compound file"), Confidence::No);
    assert_eq!(InventorCodec.detect(&inventor[..400]), Confidence::No);
}

#[test]
fn inspects_the_complete_synthetic_hierarchy() {
    let mut input = std::io::Cursor::new(fixture(true));
    let summary = InventorCodec
        .inspect(&mut input, &cadmpeg_core::decode::InspectOptions::default())
        .expect("synthetic Inventor container inspects");
    assert_eq!(summary.format, "inventor");
    assert!(summary
        .entries
        .iter()
        .any(|entry| entry.name == "RSeStorage/RSeSegInfo"));
}

#[test]
fn decode_distinguishes_container_only_from_untransferred_geometry() {
    let source = fixture(true);
    let decoded = InventorCodec
        .decode(
            &mut std::io::Cursor::new(&source),
            &DecodeOptions::default(),
        )
        .expect("synthetic Inventor container decodes structurally");
    assert_eq!(decoded.report.format, "inventor");
    assert!(!decoded.report.container_only);
    assert!(decoded
        .report
        .losses
        .iter()
        .any(|loss| loss.code == LossKind::GeometryNotTransferred));
    let native_findings = crate::validate_native(&decoded.ir);
    assert_eq!(native_findings.len(), 1, "{native_findings:#?}");
    assert!(native_findings[0]
        .message
        .contains("do not select one registry grammar"));
    assert_eq!(
        decoded
            .ir
            .native
            .namespace("inventor")
            .expect("Inventor native namespace exists")
            .version,
        9
    );

    let options = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };
    let container_only = InventorCodec
        .decode(&mut std::io::Cursor::new(source), &options)
        .expect("container-only Inventor decode succeeds");
    assert_eq!(
        container_only
            .report
            .losses
            .iter()
            .map(|loss| loss.code)
            .collect::<Vec<_>>(),
        [LossKind::ContainerOnly]
    );
    let namespace = container_only
        .ir
        .native
        .namespace("inventor")
        .expect("Inventor native namespace exists");
    let bulk = namespace
        .arena_as::<crate::native::SegmentBulkRecord>("segment_bulk")
        .expect("container-only bulk records retain their outer envelopes");
    assert!(bulk.iter().all(|record| {
        record.record_state == "not_expanded"
            && record.expanded_len.is_none()
            && record.expanded_sha256.is_none()
    }));
    assert!(container_only.source_fidelity.retained_records.is_empty());
}

fn fixture(inventor: bool) -> Vec<u8> {
    let mut file = vec![0u8; SECTOR_SIZE * 3];
    file[..8].copy_from_slice(&MAGIC);
    put_u16(&mut file, 24, 0x003e);
    put_u16(&mut file, 26, 3);
    put_u16(&mut file, 28, 0xfffe);
    put_u16(&mut file, 30, 9);
    put_u16(&mut file, 32, 6);
    put_u32(&mut file, 44, 1);
    put_u32(&mut file, 48, 0);
    put_u32(&mut file, 56, 4096);
    put_u32(&mut file, 60, END_OF_CHAIN);
    put_u32(&mut file, 68, END_OF_CHAIN);
    for index in 0..109 {
        put_u32(&mut file, 76 + index * 4, FREE_SECTOR);
    }
    put_u32(&mut file, 76, 1);
    let directory = sector_mut(&mut file, 0);
    for entry in directory.chunks_exact_mut(128) {
        entry[68..80].fill(0xff);
    }
    directory_entry(directory, 0, "Root Entry", 5, 1, END_OF_CHAIN, 0);
    let storage_name = if inventor {
        "RSeStorage"
    } else {
        "OtherStorage"
    };
    directory_entry(directory, 1, storage_name, 1, 2, END_OF_CHAIN, 0);
    directory_entry(directory, 2, "RSeSegInfo", 2, NO_STREAM, END_OF_CHAIN, 0);
    let fat = sector_mut(&mut file, 1);
    fat.fill(0xff);
    put_u32(fat, 0, END_OF_CHAIN);
    put_u32(fat, 4, FAT_SECTOR);
    file
}

fn directory_entry(
    directory: &mut [u8],
    index: usize,
    name: &str,
    object_type: u8,
    child: u32,
    start_sector: u32,
    size: u64,
) {
    let entry = &mut directory[index * 128..(index + 1) * 128];
    let encoded = name.encode_utf16().collect::<Vec<_>>();
    for (offset, unit) in encoded.iter().enumerate() {
        put_u16(entry, offset * 2, *unit);
    }
    put_u16(entry, 64, ((encoded.len() + 1) * 2) as u16);
    entry[66] = object_type;
    entry[67] = 1;
    put_u32(entry, 68, NO_STREAM);
    put_u32(entry, 72, NO_STREAM);
    put_u32(entry, 76, child);
    put_u32(entry, 116, start_sector);
    entry[120..128].copy_from_slice(&size.to_le_bytes());
}

fn sector_mut(file: &mut [u8], id: usize) -> &mut [u8] {
    let start = SECTOR_SIZE * (id + 1);
    &mut file[start..start + SECTOR_SIZE]
}

fn put_u16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}
