// SPDX-License-Identifier: Apache-2.0
use std::cell::Cell;
use std::time::Duration;

use crate::test_support::archive_session::deflated_archive;
use crate::test_support::{lp_ascii, streams_test::design_metastream_with_records};
use crate::F3dCodec;
use cadmpeg_core::decode::View;
use cadmpeg_core::scan::{ManualClock, ScanClock, ScanLimits, StopReason, Suppressed};

const FIRST: &str = "DD405BC2-D673-44F0-8833-5CB2A1C186C7";
const SECOND: &str = "D1728651-3640-4CCF-8083-AF7703013978";

fn fixture(items: &[u64]) -> Vec<u8> {
    fixture_with(items, None, "Timeline")
}

fn fixture_with(items: &[u64], inline: Option<&str>, payload: &str) -> Vec<u8> {
    let mut bulk = Vec::new();
    lp_ascii(&mut bulk, "256");
    bulk.extend_from_slice(&35_u64.to_le_bytes());
    lp_ascii(&mut bulk, payload);
    bulk.extend_from_slice(&[0, 0, 1]);
    bulk.extend_from_slice(&17_u64.to_le_bytes());
    bulk.extend_from_slice(&[0, 0]);
    bulk.extend_from_slice(&u32::try_from(items.len()).unwrap().to_le_bytes());
    for id in items {
        bulk.push(1);
        bulk.extend_from_slice(&id.to_le_bytes());
        if let Some(guid) = inline {
            lp_ascii(&mut bulk, guid);
        }
        bulk.extend_from_slice(&[0, 0]);
    }
    let meta = design_metastream_with_records(
        &[
            (
                crate::design::decode::meta::FEATURE_TIMELINE_TYPE_GUID,
                crate::design::decode::meta::FEATURE_TIMELINE_BASE_TYPE_GUID,
                2,
                "Fusion",
                &[35],
            ),
            (FIRST, "", 1, "Fusion", &[101]),
            (SECOND, "", 1, "Fusion", &[102]),
        ],
        &[(35, 0)],
    );
    deflated_archive(
        "",
        &[
            ("Design1/MetaStream.dat", &meta),
            ("Design1/BulkStream.dat", &bulk),
            (
                "Breps.BlobParts/unused.smbh",
                b"geometry bytes remain unopened",
            ),
        ],
    )
}

#[test]
fn scan_f3d_source_order_and_complete_table() {
    let scan = F3dCodec::scan_design(
        View::over_retained(&fixture(&[102, 101])),
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
        ["Mirror", "Extrude"]
    );
    assert_eq!(census.rows[0].ordinal, 0);
    assert_eq!(census.rows[1].ordinal, 1);
    assert_eq!(census.rows[0].suppressed, Suppressed::Unknown);
}

#[test]
fn scan_f3d_oversized_table_keeps_a_bounded_prefix() {
    let scan = F3dCodec::scan_design(
        View::over_retained(&fixture(&[102, 101])),
        ScanLimits {
            feature_count: 1,
            ..ScanLimits::default()
        },
        &ManualClock::default(),
    );
    assert_eq!(scan.stop_reason, Some(StopReason::FeatureCount));
    let census = scan.census.unwrap();
    assert!(!census.complete);
    assert_eq!(census.total, None);
    assert_eq!(census.rows.len(), 1);
}

#[test]
fn scan_f3d_expansion_bomb_is_bounded() {
    let source = deflated_archive("", &[("Design1/MetaStream.dat", &[0; 16384])]);
    let scan = F3dCodec::scan_design(
        View::over_retained(&source),
        ScanLimits {
            expanded_bytes: 100,
            ..ScanLimits::default()
        },
        &ManualClock::default(),
    );
    assert_eq!(scan.stop_reason, Some(StopReason::ExpandedBytes));
    assert!(scan.census.is_none());
}

#[test]
fn scan_f3d_self_reference_respects_nesting_depth() {
    let scan = F3dCodec::scan_design(
        View::over_retained(&fixture(&[35])),
        ScanLimits {
            nesting_depth: 1,
            ..ScanLimits::default()
        },
        &ManualClock::default(),
    );
    assert_eq!(scan.stop_reason, Some(StopReason::NestingDepth));
    assert!(!scan.census.unwrap().complete);
}

#[test]
fn scan_f3z_nested_document_respects_depth() {
    let member = fixture(&[102, 101]);
    for name in ["member.f3d", "member.F3D"] {
        let outer = deflated_archive(name, &[(name, &member)]);
        let scan = F3dCodec::scan_design(
            View::over_retained(&outer),
            ScanLimits {
                nesting_depth: 1,
                ..ScanLimits::default()
            },
            &ManualClock::default(),
        );
        assert_eq!(scan.stop_reason, Some(StopReason::NestingDepth));
    }
}

#[test]
fn scan_f3d_truncated_header_is_a_partial_result() {
    let scan = F3dCodec::scan_design(
        View::over_retained(b"PK\x03\x04"),
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
        n > 12
    }
}
#[test]
fn scan_f3d_cancels_during_reader_steps() {
    let clock = Clock(Cell::new(0));
    let scan = F3dCodec::scan_design(
        View::over_retained(&fixture(&[102, 101])),
        ScanLimits::default(),
        &clock,
    );
    assert_eq!(scan.stop_reason, Some(StopReason::Cancelled));
    assert!(scan.census.is_none_or(|census| !census.complete));
    assert!(clock.0.get() > 12);
}

#[test]
fn scan_f3d_public_feature_fixture_without_a_timeline_stays_unknown() {
    let source = include_bytes!("../../tests/golden/fixtures/design_base_feature.f3d");
    let scan = F3dCodec::scan_design(
        View::over_retained(source),
        ScanLimits::default(),
        &ManualClock::default(),
    );
    assert!(matches!(scan.stop_reason, Some(StopReason::Unsupported(_))));
    assert!(scan.census.is_none());
}

#[test]
fn scan_f3z_self_reference_uses_the_active_document_depth() {
    let member = fixture(&[35]);
    let outer = deflated_archive("member.f3d", &[("member.f3d", &member)]);
    let scan = F3dCodec::scan_design(
        View::over_retained(&outer),
        ScanLimits {
            nesting_depth: 2,
            ..ScanLimits::default()
        },
        &ManualClock::default(),
    );
    assert_eq!(scan.stop_reason, Some(StopReason::NestingDepth));
    assert!(!scan.census.unwrap().complete);
}

fn zstd_fixture() -> Vec<u8> {
    use cadmpeg_test_support::zip_fixture::{archive, zstd_stored_frame, RawEntry, ZSTD};
    use std::io::{Cursor, Read};
    let mut zip = zip::ZipArchive::new(Cursor::new(fixture(&[102, 101]))).expect("source archive");
    let mut entries = Vec::new();
    for ordinal in 0..zip.len() {
        let mut member = zip.by_index(ordinal).expect("source member");
        let name = member.name().expect("member name").into_owned();
        let mut bytes = Vec::new();
        member.read_to_end(&mut bytes).expect("member bytes");
        entries.push(RawEntry::compressed(
            &name,
            ZSTD,
            &zstd_stored_frame(&bytes),
            &bytes,
        ));
    }
    archive(&entries)
}

#[test]
fn scan_f3d_zstd_timeline_has_a_complete_source_order_census() {
    let scan = F3dCodec::scan_design(
        View::over_retained(&zstd_fixture()),
        ScanLimits::default(),
        &ManualClock::default(),
    );
    assert_eq!(scan.stop_reason, None);
    let census = scan.census.expect("timeline census");
    assert!(census.complete);
    assert_eq!(
        census
            .rows
            .iter()
            .map(|row| row.kind.as_str())
            .collect::<Vec<_>>(),
        ["Mirror", "Extrude"]
    );
}

#[test]
fn scan_f3d_zstd_expansion_limit_preserves_an_unstarted_census() {
    let scan = F3dCodec::scan_design(
        View::over_retained(&zstd_fixture()),
        ScanLimits {
            expanded_bytes: 100,
            ..ScanLimits::default()
        },
        &ManualClock::default(),
    );
    assert_eq!(scan.stop_reason, Some(StopReason::ExpandedBytes));
    assert!(scan.census.is_none());
}

#[test]
fn scan_f3z_child_without_a_census_keeps_the_aggregate_incomplete() {
    let first = fixture(&[102, 101]);
    let meta = design_metastream_with_records(&[(FIRST, "", 1, "Fusion", &[101])], &[(101, 0)]);
    let second = deflated_archive(
        "",
        &[
            ("Design1/MetaStream.dat", &meta),
            ("Design1/BulkStream.dat", b"feature payload"),
        ],
    );
    let source = deflated_archive("", &[("a.f3d", &first), ("b.f3d", &second)]);
    let scan = F3dCodec::scan_design(
        View::over_retained(&source),
        ScanLimits::default(),
        &ManualClock::default(),
    );
    assert!(matches!(scan.stop_reason, Some(StopReason::Malformed(_))));
    let census = scan.census.expect("first child's admitted rows");
    assert!(!census.complete);
    assert_eq!(census.total, None);
    assert_eq!(
        census
            .rows
            .iter()
            .map(|row| row.kind.as_str())
            .collect::<Vec<_>>(),
        ["Mirror", "Extrude"]
    );
}

fn assert_decoder_and_scan_leave_timeline_unread(bytes: &[u8]) {
    use std::io::{Cursor, Read, Write};

    let mut input = zip::ZipArchive::new(Cursor::new(bytes)).expect("timeline archive");
    let mut output = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = crate::zip_write::file_options(zip::CompressionMethod::Stored);
    crate::test_support::manifest_test::write_synthetic_manifests(&mut output, options);
    for name in ["Design1/MetaStream.dat", "Design1/BulkStream.dat"] {
        let mut member = Vec::new();
        input
            .by_name(name)
            .expect("timeline stream")
            .read_to_end(&mut member)
            .unwrap();
        output
            .start_file(format!("FusionAssetName[Active]/{name}"), options)
            .unwrap();
        output.write_all(&member).unwrap();
    }
    let decoder_bytes = output.finish().unwrap().into_inner();
    let mut losses = Vec::new();
    let decoded = crate::test_support::zip_test::with_scan(&decoder_bytes, |container| {
        crate::design::decode::meta::decode_feature_timelines(container, &mut losses)
    })
    .expect("an unreadable timeline record is contained");
    assert!(decoded.is_empty());
    assert_eq!(losses.len(), 1);
    let scan = F3dCodec::scan_design(
        View::over_retained(bytes),
        ScanLimits::default(),
        &ManualClock::default(),
    );
    assert!(matches!(scan.stop_reason, Some(StopReason::Malformed(_))));
    assert!(scan
        .census
        .is_none_or(|census| !census.complete && census.total.is_none()));
}

#[test]
fn scan_f3d_unregistered_inline_guid_matches_decoder_unread_record() {
    assert_decoder_and_scan_leave_timeline_unread(&fixture_with(&[101], Some(SECOND), "Timeline"));
}

#[test]
fn scan_f3d_payload_bounds_match_decoder_unread_record() {
    for payload in ["a".repeat(2001), "Timéline".into()] {
        assert_decoder_and_scan_leave_timeline_unread(&fixture_with(&[101], None, &payload));
    }
}

#[test]
fn scan_f3d_repeated_reference_matches_decoder_unread_record() {
    assert_decoder_and_scan_leave_timeline_unread(&fixture(&[101, 101]));
}

#[test]
fn scan_f3d_registered_inline_guid_preserves_kind() {
    let scan = F3dCodec::scan_design(
        View::over_retained(&fixture_with(&[101], Some(FIRST), "Timeline")),
        ScanLimits::default(),
        &ManualClock::default(),
    );
    assert_eq!(scan.stop_reason, None);
    let census = scan.census.expect("registered timeline");
    assert!(census.complete);
    assert_eq!(census.rows[0].kind, "Extrude");
}

#[test]
fn scan_f3d_parser_input_is_admitted_before_payload_validation() {
    let bytes = fixture_with(&[101], None, &"a".repeat(1024 * 1024));
    let scan = F3dCodec::scan_design(
        View::over_retained(&bytes),
        ScanLimits {
            source_bytes: 64 * 1024,
            ..ScanLimits::default()
        },
        &ManualClock::default(),
    );
    assert_eq!(scan.stop_reason, Some(StopReason::SourceBytes));
    assert!(scan.census.is_none());
}
