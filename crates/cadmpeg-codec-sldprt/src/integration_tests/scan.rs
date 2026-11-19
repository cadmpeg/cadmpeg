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
    assert!(!scan.census.unwrap().complete);
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
fn scan_sldprt_truncated_record_preserves_prior_rows() {
    let bytes = source(br#"<Keywords><Feature Type="BossExtrude"/><Feature"#);
    let scan = SldprtCodec::scan_design(
        View::over_retained(&bytes),
        ScanLimits::default(),
        &ManualClock::default(),
    );
    assert!(scan.stop_reason.is_some());
    let census = scan.census.unwrap();
    assert!(!census.complete);
    assert_eq!(census.rows[0].kind, "BossExtrude");
}

#[test]
fn scan_sldprt_trailing_garbage_keeps_the_census_incomplete() {
    let bytes = source(br#"<Keywords><Feature Type="BossExtrude"/></Keywords>garbage"#);
    let scan = SldprtCodec::scan_design(
        View::over_retained(&bytes),
        ScanLimits::default(),
        &ManualClock::default(),
    );
    assert!(matches!(scan.stop_reason, Some(StopReason::Malformed(_))));
    let census = scan.census.expect("admitted listing prefix");
    assert!(!census.complete);
    assert_eq!(census.total, None);
    assert_eq!(census.rows[0].kind, "BossExtrude");
}
