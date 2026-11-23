// SPDX-License-Identifier: Apache-2.0
use crate::test_support::test_fixtures::{
    empty_asm_history_kernel_stream, primary_envelope_fixture_with_design,
};
use crate::InventorCodec;
use cadmpeg_core::decode::View;
use cadmpeg_core::scan::{ManualClock, ScanClock, ScanLimits, StopReason, Suppressed};
use std::cell::Cell;
use std::time::Duration;

fn fixture() -> Vec<u8> {
    primary_envelope_fixture_with_design(
        &empty_asm_history_kernel_stream(),
        &[
            (crate::feature::FEATURE_TYPE, vec![0; 12]),
            (crate::feature::MIRROR_FEATURE_TYPE, vec![0; 12]),
            (crate::sketch::SKETCH_TYPE, vec![0; 12]),
        ],
    )
}

#[test]
fn scan_ipt_existing_public_fixture_builder_preserves_source_order() {
    let scan = InventorCodec::scan_design(
        View::over_retained(&fixture()),
        ScanLimits::default(),
        &ManualClock::default(),
    );
    assert_eq!(scan.stop_reason, None);
    let census = scan.census.expect("scan census");
    assert!(census.complete);
    assert_eq!(census.total, Some(3));
    assert_eq!(
        census
            .rows
            .iter()
            .map(|row| row.kind.as_str())
            .collect::<Vec<_>>(),
        ["feature", "mirror", "sketch"]
    );
    assert_eq!(census.rows[0].ordinal, 0);
    assert_eq!(census.rows[1].ordinal, 1);
    assert_eq!(census.rows[0].suppressed, Suppressed::Unknown);
}

#[test]
fn scan_ipt_oversized_table_keeps_the_admitted_prefix() {
    let scan = InventorCodec::scan_design(
        View::over_retained(&fixture()),
        ScanLimits {
            feature_count: 1,
            ..ScanLimits::default()
        },
        &ManualClock::default(),
    );
    assert_eq!(scan.stop_reason, Some(StopReason::FeatureCount));
    let census = scan.census.expect("scan census");
    assert_eq!(census.rows.len(), 1);
    assert_eq!(census.total, None);
    assert!(!census.complete);
}

#[test]
fn scan_ipt_expansion_bomb_stops_at_the_expansion_ceiling() {
    let source = primary_envelope_fixture_with_design(
        &empty_asm_history_kernel_stream(),
        &[(crate::feature::FEATURE_TYPE, vec![0; 16384])],
    );
    let scan = InventorCodec::scan_design(
        View::over_retained(&source),
        ScanLimits {
            expanded_bytes: 1024,
            ..ScanLimits::default()
        },
        &ManualClock::default(),
    );
    assert_eq!(scan.stop_reason, Some(StopReason::ExpandedBytes));
}

#[test]
fn scan_ipt_self_referencing_directory_stops_at_depth_limit() {
    let mut source = crate::test_support::test_fixtures::fixture(true);
    // The existing CFB fixture's storage entry is its own child.
    cadmpeg_test_support::bytes::put_u32(&mut source, 512 + 128 + 76, 1);
    let scan = InventorCodec::scan_design(
        View::over_retained(&source),
        ScanLimits {
            nesting_depth: 1,
            ..ScanLimits::default()
        },
        &ManualClock::default(),
    );
    assert_eq!(scan.stop_reason, Some(StopReason::NestingDepth));
    assert!(scan.census.is_none());
}

#[test]
fn scan_ipt_truncated_header_keeps_census_unstarted() {
    let scan = InventorCodec::scan_design(
        View::over_retained(&[0xd0, 0xcf, 0x11]),
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
        n > 140
    }
}
#[test]
fn scan_ipt_cancels_during_container_reader_steps() {
    let clock = Clock(Cell::new(0));
    let scan = InventorCodec::scan_design(
        View::over_retained(&fixture()),
        ScanLimits::default(),
        &clock,
    );
    assert_eq!(scan.stop_reason, Some(StopReason::Cancelled));
    assert!(scan.census.is_none_or(|census| !census.complete));
    assert!(clock.0.get() > 140);
}

#[test]
fn scan_ipt_zstd_metadata_and_listing_are_complete() {
    let source =
        crate::test_support::test_fixtures::primary_envelope_fixture_with_design_compression(
            &empty_asm_history_kernel_stream(),
            &[
                (crate::feature::FEATURE_TYPE, vec![0; 12]),
                (crate::feature::MIRROR_FEATURE_TYPE, vec![0; 12]),
            ],
            [2, 2],
        );
    let scan = InventorCodec::scan_design(
        View::over_retained(&source),
        ScanLimits::default(),
        &ManualClock::default(),
    );
    assert_eq!(scan.stop_reason, None);
    let census = scan.census.expect("feature census");
    assert!(census.complete);
    assert_eq!(census.total, Some(2));
    assert_eq!(
        census
            .rows
            .iter()
            .map(|row| row.kind.as_str())
            .collect::<Vec<_>>(),
        ["feature", "mirror"]
    );
}

#[test]
fn scan_ipt_suppression_comes_from_the_feature_header() {
    let first = crate::test_support::test_fixtures::content(1);
    let mut second = crate::test_support::test_fixtures::content(2);
    cadmpeg_test_support::bytes::put_u32(
        &mut second,
        crate::layout::pm_dc_content_header::FLAGS,
        0x0002_0204,
    );
    let source = primary_envelope_fixture_with_design(
        &empty_asm_history_kernel_stream(),
        &[
            (crate::feature::FEATURE_TYPE, first),
            (crate::feature::MIRROR_FEATURE_TYPE, second),
        ],
    );
    let scan = InventorCodec::scan_design(
        View::over_retained(&source),
        ScanLimits::default(),
        &ManualClock::default(),
    );
    assert_eq!(scan.stop_reason, None);
    let census = scan.census.expect("feature census");
    assert!(census.complete);
    assert_eq!(census.rows[0].suppressed, Suppressed::No);
    assert_eq!(census.rows[1].suppressed, Suppressed::Yes);
}
