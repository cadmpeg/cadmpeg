// SPDX-License-Identifier: Apache-2.0
use super::*;
use cadmpeg_core::scan::{ManualClock, ScanBudget, ScanLimits, StopReason};
use std::io::{Cursor, Write};

fn fixture() -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    zip.start_file("geometry.bin", options)
        .expect("scan fixture");
    for _ in 0..32 {
        zip.write_all(&[0; 4096]).expect("scan fixture");
    }
    zip.start_file("listing", options).expect("scan fixture");
    zip.write_all(b"feature rows").expect("scan fixture");
    zip.finish().expect("scan fixture").into_inner()
}

#[test]
fn scan_zip_skips_geometry_payloads_under_a_small_source_budget() {
    let bytes = fixture();
    let clock = ManualClock::default();
    let budget = ScanBudget::new(
        ScanLimits {
            source_bytes: 4096,
            ..ScanLimits::default()
        },
        &clock,
    );
    let mut zip = ScanZip::new(View::over_retained(&bytes), &budget, true).expect("scan fixture");
    let entries = zip.entries().expect("scan fixture");
    let listing = entries
        .iter()
        .find(|entry| entry.name == "listing")
        .expect("scan fixture");
    assert_eq!(
        zip.open(listing.ordinal).expect("scan fixture"),
        b"feature rows"
    );
    assert!(budget.bytes_read() <= 4096);
}

#[test]
fn scan_zip_refuses_metadata_storage_before_archive_allocation() {
    let bytes = fixture();
    let clock = ManualClock::default();
    let budget = ScanBudget::new(
        ScanLimits {
            retained_bytes: 0,
            ..ScanLimits::default()
        },
        &clock,
    );
    assert!(matches!(
        ScanZip::new(View::over_retained(&bytes), &budget, true),
        Err(StopReason::RetainedBytes)
    ));
}

#[test]
fn scan_zip_reads_zstandard_with_a_checked_checksum() {
    use cadmpeg_test_support::zip_fixture::{
        archive, zstd_golden_plaintext, RawEntry, ZSTD, ZSTD_GOLDEN_FRAME,
    };
    let plain = zstd_golden_plaintext();
    let source = archive(&[RawEntry::compressed(
        "listing",
        ZSTD,
        ZSTD_GOLDEN_FRAME,
        &plain,
    )]);
    let clock = ManualClock::default();
    let budget = ScanBudget::new(ScanLimits::default(), &clock);
    let mut zip =
        ScanZip::new(View::over_retained(&source), &budget, true).expect("Zstandard archive");
    assert_eq!(zip.open(0).expect("listing expansion"), plain);
}

#[test]
fn scan_zip_refuses_an_encryption_extra_with_a_clear_flag() {
    use cadmpeg_test_support::zip_fixture::{archive, RawEntry, STORED};
    let source =
        archive(&[RawEntry::stored("listing", b"readable plaintext")
            .aes_without_the_encrypted_bit(STORED)]);
    let clock = ManualClock::default();
    let budget = ScanBudget::new(ScanLimits::default(), &clock);
    match ScanZip::new(View::over_retained(&source), &budget, true) {
        Ok(mut zip) => assert!(matches!(zip.open(0), Err(StopReason::Unsupported(_)))),
        Err(reason) => assert!(matches!(reason, StopReason::Unsupported(_))),
    }
}

#[test]
fn scan_zip64_admits_the_wide_directory_before_opening_a_member() {
    use crate::layout::end_record;
    use cadmpeg_test_support::zip_fixture::{archive, RawEntry};
    let mut source = archive(&[RawEntry::stored("listing", b"feature rows")]);
    let old = source.split_off(source.len() - end_record::LEN);
    let directory_size =
        u64::from(View::u32_le_at(&old, end_record::DIRECTORY_SIZE).expect("directory size"));
    let directory_position = u64::from(
        View::u32_le_at(&old, end_record::DIRECTORY_POSITION).expect("directory position"),
    );
    let wide_position = u64::try_from(source.len()).expect("wide end position");
    source.extend_from_slice(b"PK\x06\x06");
    source.extend_from_slice(&44_u64.to_le_bytes());
    source.extend_from_slice(&45_u16.to_le_bytes());
    source.extend_from_slice(&45_u16.to_le_bytes());
    source.extend_from_slice(&[0; 8]);
    source.extend_from_slice(&1_u64.to_le_bytes());
    source.extend_from_slice(&1_u64.to_le_bytes());
    source.extend_from_slice(&directory_size.to_le_bytes());
    source.extend_from_slice(&directory_position.to_le_bytes());
    source.extend_from_slice(b"PK\x06\x07");
    source.extend_from_slice(&0_u32.to_le_bytes());
    source.extend_from_slice(&wide_position.to_le_bytes());
    source.extend_from_slice(&1_u32.to_le_bytes());
    let mut end = old;
    cadmpeg_test_support::bytes::put_u16(&mut end, end_record::DISK_ENTRIES, u16::MAX);
    cadmpeg_test_support::bytes::put_u16(&mut end, end_record::ENTRIES, u16::MAX);
    cadmpeg_test_support::bytes::put_u32(&mut end, end_record::DIRECTORY_SIZE, u32::MAX);
    cadmpeg_test_support::bytes::put_u32(&mut end, end_record::DIRECTORY_POSITION, u32::MAX);
    source.extend_from_slice(&end);
    let clock = ManualClock::default();
    let budget = ScanBudget::new(ScanLimits::default(), &clock);
    let mut zip =
        ScanZip::new(View::over_retained(&source), &budget, true).expect("ZIP64 directory");
    assert_eq!(zip.open(0).expect("listing"), b"feature rows");
}

#[test]
fn scan_zip_rejects_variable_fields_outside_the_declared_directory_before_allocation() {
    use crate::layout::{central_header, end_record};
    use cadmpeg_test_support::bytes::{put_u16, put_u32};
    for field in [
        central_header::NAME_LENGTH,
        central_header::EXTRA_LENGTH,
        central_header::COMMENT_LENGTH,
    ] {
        let mut source = vec![0; central_header::LEN];
        source[..4].copy_from_slice(b"PK\x01\x02");
        put_u16(&mut source, field, u16::MAX);
        source.resize(central_header::LEN + usize::from(u16::MAX), b'a');
        let mut end = vec![0; end_record::LEN];
        end[..4].copy_from_slice(b"PK\x05\x06");
        put_u16(&mut end, end_record::DISK_ENTRIES, 1);
        put_u16(&mut end, end_record::ENTRIES, 1);
        put_u32(
            &mut end,
            end_record::DIRECTORY_SIZE,
            u32::try_from(central_header::LEN).expect("fixed header"),
        );
        source.extend_from_slice(&end);
        let clock = ManualClock::default();
        let budget = ScanBudget::new(
            ScanLimits {
                retained_bytes: 2048,
                ..ScanLimits::default()
            },
            &clock,
        );
        assert!(matches!(
            ScanZip::new(View::over_retained(&source), &budget, true),
            Err(StopReason::Malformed(_))
        ));
        assert!(budget.bytes_read() < 1024);
    }
}

#[test]
fn scan_zip_keeps_prefixed_archive_and_legacy_name_identity() {
    use crate::layout::{central_header, end_record, local_header};
    use cadmpeg_test_support::zip_fixture::{archive, RawEntry};
    let mut zip = archive(&[RawEntry::stored("x", b"feature rows")]);
    let tail = zip.len() - end_record::LEN;
    let central = usize::try_from(
        View::u32_le_at(&zip[tail..], end_record::DIRECTORY_POSITION).expect("directory position"),
    )
    .expect("offset");
    zip[local_header::LEN] = 0x82;
    zip[central + central_header::LEN] = 0x82;
    let mut source = b"embedded ZIP prefix".to_vec();
    source.extend(zip);
    let clock = ManualClock::default();
    let budget = ScanBudget::new(ScanLimits::default(), &clock);
    let mut zip =
        ScanZip::new(View::over_retained(&source), &budget, true).expect("prefixed archive");
    assert_eq!(zip.entries().expect("directory")[0].name, "é");
    assert_eq!(zip.open(0).expect("listing"), b"feature rows");
}

#[test]
fn scan_zip64_preserves_a_wide_size_equal_to_the_narrow_sentinel() {
    use crate::layout::{central_header, end_record};
    use cadmpeg_test_support::bytes::{put_u16, put_u32};
    use cadmpeg_test_support::zip_fixture::{archive, RawEntry};
    let mut source = archive(&[RawEntry::stored("listing", b"feature rows")]);
    let tail = source.len() - end_record::LEN;
    let central = usize::try_from(
        View::u32_le_at(&source[tail..], end_record::DIRECTORY_POSITION).expect("directory"),
    )
    .expect("offset");
    let directory_size =
        View::u32_le_at(&source[tail..], end_record::DIRECTORY_SIZE).expect("directory extent");
    put_u32(
        &mut source,
        central + central_header::EXPANDED_LENGTH,
        u32::MAX,
    );
    put_u16(&mut source, central + central_header::EXTRA_LENGTH, 12);
    let mut extra = vec![1, 0, 8, 0];
    extra.extend_from_slice(&u64::from(u32::MAX).to_le_bytes());
    let extra_at = central + central_header::LEN + "listing".len();
    source.splice(extra_at..extra_at, extra);
    put_u32(
        &mut source,
        tail + 12 + end_record::DIRECTORY_SIZE,
        directory_size + 12,
    );
    let clock = ManualClock::default();
    let budget = ScanBudget::new(ScanLimits::default(), &clock);
    let mut zip =
        ScanZip::new(View::over_retained(&source), &budget, true).expect("wide directory");
    let entries = zip.entries().expect("declared size");
    assert_eq!(entries[0].declared_size, u64::from(u32::MAX));
}
