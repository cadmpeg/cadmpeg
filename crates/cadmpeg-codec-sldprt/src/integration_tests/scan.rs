// SPDX-License-Identifier: Apache-2.0
use crate::test_support::container::{make_block, outer_header};
use crate::SldprtCodec;
use cadmpeg_core::decode::View;
use cadmpeg_core::scan::{ManualClock, Operands, ScanClock, ScanLimits, StopReason, Suppressed};
use std::cell::Cell;
use std::time::Duration;

fn source(xml: &[u8]) -> Vec<u8> {
    let mut bytes = outer_header();
    bytes.extend(make_block(0x42, "Contents/Keywords", xml));
    bytes
}
const LIST: &[u8] = br#"<Keywords><Feature Type="BossExtrude" Suppressed="0"/><Feature Type="Fillet" Suppressed="1"/></Keywords>"#;

#[test]
fn scan_sldprt_source_order_and_suppression_are_listing_evidence() {
    let scan = SldprtCodec::scan_design(
        View::over_retained(&source(LIST)),
        ScanLimits::default(),
        &ManualClock::default(),
    );
    assert_eq!(scan.stop_reason, None);
    let census = scan.census.unwrap();
    assert!(census.complete);
    assert_eq!(census.total, Some(2));
    assert_eq!(
        census
            .rows
            .iter()
            .map(|row| row.kind.as_str())
            .collect::<Vec<_>>(),
        ["BossExtrude", "Fillet"]
    );
    assert_eq!(census.rows[0].suppressed, Suppressed::No);
    assert_eq!(census.rows[1].suppressed, Suppressed::Yes);
    assert_eq!(census.rows[1].ordinal, 1);
    assert_eq!(census.rows[1].operands, Operands::Unknown);
}

#[test]
fn scan_sldprt_existing_public_history_fixture_is_complete() {
    let bytes = include_bytes!("../../tests/golden/fixtures/body_history.sldprt");
    let scan = SldprtCodec::scan_design(
        View::over_retained(bytes),
        ScanLimits::default(),
        &ManualClock::default(),
    );
    assert_eq!(scan.stop_reason, None);
    let census = scan.census.unwrap();
    assert!(census.complete);
    assert_eq!(census.total, Some(2));
    assert_eq!(
        census
            .rows
            .iter()
            .map(|row| row.kind.as_str())
            .collect::<Vec<_>>(),
        ["BossExtrude", "EquationDrivenCurve"]
    );
}

#[test]
fn scan_sldprt_oversized_table_keeps_the_admitted_prefix() {
    let scan = SldprtCodec::scan_design(
        View::over_retained(&source(LIST)),
        ScanLimits {
            feature_count: 1,
            ..ScanLimits::default()
        },
        &ManualClock::default(),
    );
    assert_eq!(scan.stop_reason, Some(StopReason::FeatureCount));
    let census = scan.census.unwrap();
    assert_eq!(census.rows.len(), 1);
    assert_eq!(census.total, None);
    assert!(!census.complete);
}

#[test]
fn scan_sldprt_expansion_bomb_stops_before_unbounded_output() {
    let scan = SldprtCodec::scan_design(
        View::over_retained(&source(&[b' '; 16384])),
        ScanLimits {
            expanded_bytes: 128,
            ..ScanLimits::default()
        },
        &ManualClock::default(),
    );
    assert_eq!(scan.stop_reason, Some(StopReason::ExpandedBytes));
}

#[test]
fn scan_sldprt_recursive_listing_is_bounded_by_depth() {
    let bytes =
        source(br#"<Keywords><Feature id="self"><Feature ref="self"/></Feature></Keywords>"#);
    let scan = SldprtCodec::scan_design(
        View::over_retained(&bytes),
        ScanLimits {
            nesting_depth: 2,
            ..ScanLimits::default()
        },
        &ManualClock::default(),
    );
    assert_eq!(scan.stop_reason, Some(StopReason::NestingDepth));
    assert!(scan.census.is_none());
}

#[test]
fn scan_sldprt_truncated_header_keeps_census_unstarted() {
    let scan = SldprtCodec::scan_design(
        View::over_retained(&[1, 0, 0]),
        ScanLimits::default(),
        &ManualClock::default(),
    );
    assert!(matches!(scan.stop_reason, Some(StopReason::Truncated(_))));
    assert!(scan.census.is_none());
}

struct Clock(Cell<u64>);
impl ScanClock for Clock {
    fn elapsed(&self) -> Duration {
        Duration::ZERO
    }
    fn is_cancelled(&self) -> bool {
        let n = self.0.get();
        self.0.set(n + 1);
        n > 90
    }
}
#[test]
fn scan_sldprt_cancellation_interrupts_a_reader_loop() {
    let clock = Clock(Cell::new(0));
    let scan = SldprtCodec::scan_design(
        View::over_retained(&source(LIST)),
        ScanLimits::default(),
        &clock,
    );
    assert_eq!(scan.stop_reason, Some(StopReason::Cancelled));
    assert!(scan.census.is_none_or(|census| !census.complete));
    assert!(clock.0.get() > 90);
}

#[test]
fn scan_sldprt_truncated_xml_refuses_the_document() {
    let bytes = source(br#"<Keywords><Feature Type="BossExtrude"/><Feature"#);
    let scan = SldprtCodec::scan_design(
        View::over_retained(&bytes),
        ScanLimits::default(),
        &ManualClock::default(),
    );
    assert!(scan.stop_reason.is_some());
    assert!(scan.census.is_none());
}

#[test]
fn scan_sldprt_trailing_garbage_refuses_the_document() {
    let bytes = source(br#"<Keywords><Feature Type="BossExtrude"/></Keywords>garbage"#);
    let scan = SldprtCodec::scan_design(
        View::over_retained(&bytes),
        ScanLimits::default(),
        &ManualClock::default(),
    );
    assert!(matches!(scan.stop_reason, Some(StopReason::Malformed(_))));
    assert!(scan.census.is_none());
}

#[test]
fn scan_sldprt_truncated_later_block_keeps_prior_rows_incomplete() {
    let mut bytes = source(LIST);
    bytes.extend(make_block(0x42, "Contents/Keywords", LIST));
    bytes.pop();
    let scan = SldprtCodec::scan_design(
        View::over_retained(&bytes),
        ScanLimits::default(),
        &ManualClock::default(),
    );
    assert!(matches!(scan.stop_reason, Some(StopReason::Truncated(_))));
    let census = scan.census.expect("first block's admitted rows");
    assert!(!census.complete);
    assert_eq!(census.total, None);
    assert_eq!(
        census
            .rows
            .iter()
            .map(|row| row.kind.as_str())
            .collect::<Vec<_>>(),
        ["BossExtrude", "Fillet"]
    );
}

#[test]
fn scan_sldprt_truncated_non_history_block_is_skipped() {
    let mut bytes = source(LIST);
    let mut other = make_block(0x42, "Preview/Thumbnail", LIST);
    other.truncate(other.len() - 1);
    bytes.extend(other);
    assert_complete_history_listing(&bytes);
}

#[test]
fn scan_sldprt_history_block_cut_in_its_header_is_truncated() {
    let mut bytes = source(LIST);
    let marker = make_block(0x42, "Contents/Keywords", LIST);
    bytes.extend_from_slice(&marker[..8]);
    let scan = SldprtCodec::scan_design(
        View::over_retained(&bytes),
        ScanLimits::default(),
        &ManualClock::default(),
    );
    assert!(matches!(scan.stop_reason, Some(StopReason::Truncated(_))));
    let census = scan.census.expect("first block's admitted rows");
    assert!(!census.complete);
}

fn assert_complete_history_listing(bytes: &[u8]) {
    let scan = SldprtCodec::scan_design(
        View::over_retained(bytes),
        ScanLimits::default(),
        &ManualClock::default(),
    );
    assert_eq!(scan.stop_reason, None);
    let census = scan.census.expect("history census");
    assert!(census.complete);
    assert_eq!(
        census
            .rows
            .iter()
            .map(|row| row.kind.as_str())
            .collect::<Vec<_>>(),
        ["BossExtrude", "Fillet"]
    );
}

#[test]
fn scan_sldprt_namespace_prefixes_preserve_local_feature_names() {
    for (xml, expected_kind) in [
        (br#"<k:Keywords xmlns:k="urn:test"><k:Feature Type="BossExtrude" Suppressed="1"/></k:Keywords>"#.as_slice(), "BossExtrude"),
        (br#"<k:Keywords xmlns:k="urn:test"><k:Configuration/><k:Dimension/><k:Extrusion Suppressed="1"/></k:Keywords>"#.as_slice(), "Extrusion"),
    ] {
        let scan = SldprtCodec::scan_design(View::over_retained(&source(xml)), ScanLimits::default(), &ManualClock::default());
        assert_eq!(scan.stop_reason, None);
        let census = scan.census.expect("namespaced census");
        assert!(census.complete);
        assert_eq!(census.total, Some(1));
        assert_eq!(census.rows[0].suppressed, Suppressed::Yes);
        assert_eq!(census.rows[0].kind, expected_kind);
    }
}

#[test]
fn scan_sldprt_namespace_closing_tags_retain_qualified_framing() {
    let bytes =
        source(br#"<k:Keywords xmlns:k="urn:test" xmlns:j="urn:test"><k:Feature/></j:Keywords>"#);
    let scan = SldprtCodec::scan_design(
        View::over_retained(&bytes),
        ScanLimits::default(),
        &ManualClock::default(),
    );
    assert!(matches!(scan.stop_reason, Some(StopReason::Malformed(_))));
    assert!(scan.census.is_none());
}

fn compound_keywords(xml: &[u8]) -> Vec<u8> {
    use cadmpeg_test_support::bytes::{put_u16, put_u32};
    let data_sectors = xml.len().div_ceil(512);
    let fat_sectors = (data_sectors + 1).div_ceil(127);
    assert!(fat_sectors <= 109);
    let first_fat = data_sectors + 1;
    let mut bytes = cadmpeg_test_support::compound::compound_fixture();
    bytes.truncate(1024);
    bytes.resize((first_fat + fat_sectors + 1) * 512, 0);
    put_u32(
        &mut bytes,
        44,
        u32::try_from(fat_sectors).expect("FAT count"),
    );
    for index in 0..109 {
        let sector = if index < fat_sectors {
            u32::try_from(first_fat + index).expect("FAT sector")
        } else {
            u32::MAX
        };
        put_u32(&mut bytes, 76 + index * 4, sector);
    }
    let directory = &mut bytes[512..1024];
    for (index, name, kind, child, start, size) in [
        (1, "Contents", 1, 2, 0xffff_fffe, 0),
        (
            2,
            "Keywords",
            2,
            u32::MAX,
            1,
            u64::try_from(xml.len()).expect("XML extent"),
        ),
    ] {
        let entry = &mut directory[index * 128..(index + 1) * 128];
        entry.fill(0);
        let mut units = 0;
        for (at, unit) in name.encode_utf16().enumerate() {
            put_u16(entry, at * 2, unit);
            units += 1;
        }
        put_u16(
            entry,
            64,
            u16::try_from((units + 1) * 2).expect("directory name"),
        );
        entry[66] = kind;
        entry[67] = 1;
        put_u32(entry, 68, u32::MAX);
        put_u32(entry, 72, u32::MAX);
        put_u32(entry, 76, child);
        put_u32(entry, 116, start);
        entry[120..128].copy_from_slice(&size.to_le_bytes());
    }
    bytes[1024..1024 + xml.len()].copy_from_slice(xml);
    let fat = &mut bytes[(first_fat + 1) * 512..];
    fat.fill(0xff);
    put_u32(fat, 0, 0xffff_fffe);
    for index in 1..=data_sectors {
        let next = if index == data_sectors {
            0xffff_fffe
        } else {
            u32::try_from(index + 1).expect("next data sector")
        };
        put_u32(fat, index * 4, next);
    }
    for index in first_fat..first_fat + fat_sectors {
        put_u32(fat, index * 4, 0xffff_fffd);
    }
    bytes
}

#[test]
fn scan_sldprt_compound_utf16_conversion_respects_retained_storage() {
    let mut xml = vec![0xff, 0xfe];
    for unit in "<Keywords><Feature Type=\""
        .encode_utf16()
        .chain(std::iter::repeat_n(0x0800, 1024 * 1024))
        .chain("\"/></Keywords>".encode_utf16())
    {
        xml.extend_from_slice(&unit.to_le_bytes());
    }
    let bytes = compound_keywords(&xml);
    let scan = SldprtCodec::scan_design(
        View::over_retained(&bytes),
        ScanLimits {
            retained_bytes: 7 * 1024 * 1024,
            ..ScanLimits::default()
        },
        &ManualClock::default(),
    );
    assert_eq!(scan.stop_reason, Some(StopReason::RetainedBytes));
    assert!(scan.census.is_none());
}

#[test]
fn scan_sldprt_first_block_and_three_mib_tail_complete_at_default_limits() {
    let mut bytes = source(LIST);
    let accepted = u64::try_from(bytes.len()).expect("source length");
    bytes.resize(bytes.len() + 3 * 1024 * 1024, 0xa5);
    let scan = SldprtCodec::scan_design(
        View::over_retained(&bytes),
        ScanLimits::default(),
        &ManualClock::default(),
    );
    assert_eq!(scan.stop_reason, None);
    assert!(scan.bytes_read < accepted + 1024);
    let census = scan.census.expect("first block listing");
    assert!(census.complete);
    assert_eq!(census.total, Some(2));
}

#[test]
fn scan_sldprt_keywords_xml_matches_decoder_syntax() {
    for xml in [
        r#"<Keywords><Feature Type="A<B"/></Keywords>"#,
        "<Keywords><p:Feature/></Keywords>",
        "<Keywords><Feature>&unknown;</Feature></Keywords>",
        r#"<Keywords xmlns:a="urn:test" xmlns:b="urn:test"><Feature a:id="1" b:id="2"/></Keywords>"#,
    ] {
        assert!(roxmltree::Document::parse(xml).is_err());
        let scan = SldprtCodec::scan_design(
            View::over_retained(&source(xml.as_bytes())),
            ScanLimits::default(),
            &ManualClock::default(),
        );
        assert!(matches!(scan.stop_reason, Some(StopReason::Malformed(_))));
        assert!(scan.census.is_none());
    }
}
