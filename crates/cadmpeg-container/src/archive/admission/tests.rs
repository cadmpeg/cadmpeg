// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::indexing_slicing)]
use crate::layout::end_record;
use crate::ArchiveSnapshot;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use std::io::{Cursor, Write};

#[test]
fn caller_directory_limits_admit_at_the_boundary_and_refuse_each_excess() {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file("synthetic.json", zip::write::SimpleFileOptions::default())
        .unwrap();
    zip.write_all(b"{}").unwrap();
    let bytes = zip.finish().unwrap().into_inner();
    let end = bytes.len() - end_record::LEN;
    let directory_size = u64::from(
        cadmpeg_core::decode::View::u32_le_at(&bytes, end + end_record::DIRECTORY_SIZE).unwrap(),
    );
    let arena = DecodeArena::new();
    let (_, view) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()).unwrap();
    assert_eq!(
        ArchiveSnapshot::admit_directory(view, 1, directory_size),
        Some(())
    );
    assert_eq!(
        ArchiveSnapshot::admit_directory(view, 0, directory_size),
        None
    );
    assert_eq!(
        ArchiveSnapshot::admit_directory(view, 1, directory_size - 1),
        None
    );
    let mut bytes = bytes;
    bytes[end + end_record::DISK..end + end_record::DISK + 2].copy_from_slice(&1u16.to_le_bytes());
    let arena = DecodeArena::new();
    let (_, view) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()).unwrap();
    assert_eq!(
        ArchiveSnapshot::admit_directory(view, 1, directory_size),
        None
    );
}

#[test]
fn zip64_directory_admission_uses_the_declared_counts_and_disks() {
    use crate::layout::{local_header, zip64_end, zip64_locator};
    let mut bytes = vec![0u8; local_header::LEN];
    bytes[..4].copy_from_slice(b"PK\x03\x04");
    let record = bytes.len();
    bytes.extend_from_slice(&[0u8; zip64_end::LEN]);
    bytes[record..record + 4].copy_from_slice(b"PK\x06\x06");
    bytes[record + zip64_end::RECORD_SIZE..record + zip64_end::RECORD_SIZE + 8]
        .copy_from_slice(&44u64.to_le_bytes());
    let locator = bytes.len();
    bytes.extend_from_slice(&[0u8; zip64_locator::LEN]);
    bytes[locator..locator + 4].copy_from_slice(b"PK\x06\x07");
    bytes[locator + zip64_locator::END_RECORD_POSITION
        ..locator + zip64_locator::END_RECORD_POSITION + 8]
        .copy_from_slice(&u64::try_from(record).unwrap().to_le_bytes());
    bytes[locator + zip64_locator::DISKS..locator + zip64_locator::DISKS + 4]
        .copy_from_slice(&1u32.to_le_bytes());
    let end = bytes.len();
    bytes.extend_from_slice(&[0u8; end_record::LEN]);
    bytes[end..end + 4].copy_from_slice(b"PK\x05\x06");
    for (case, expected) in [
        ("valid", true),
        ("count", false),
        ("disks", false),
        ("record", false),
    ] {
        let mut bytes = bytes.clone();
        match case {
            "count" => bytes[record + zip64_end::ENTRIES..record + zip64_end::ENTRIES + 8]
                .copy_from_slice(&1u64.to_le_bytes()),
            "disks" => bytes[locator + zip64_locator::DISKS..locator + zip64_locator::DISKS + 4]
                .copy_from_slice(&2u32.to_le_bytes()),
            "record" => bytes[record + zip64_end::RECORD_SIZE..record + zip64_end::RECORD_SIZE + 8]
                .copy_from_slice(&4097u64.to_le_bytes()),
            _ => {}
        }
        let arena = DecodeArena::new();
        let (_, view) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()).unwrap();
        assert_eq!(
            ArchiveSnapshot::admit_directory(view, 0, 4096).is_some(),
            expected,
            "{case}"
        );
    }
}
