// SPDX-License-Identifier: Apache-2.0
use std::cell::Cell;
use std::time::Duration;

use super::*;
use crate::scan::{DesignScan, InflateFraming, Readability, Suppressed};

#[test]
fn scan_charges_before_a_source_read_and_seek_checks_cancellation() {
    let clock = ManualClock::default();
    let budget = ScanBudget::new(
        ScanLimits {
            source_bytes: 3,
            ..ScanLimits::default()
        },
        &clock,
    );
    let mut reader = ScanReader::source(View::over_retained(&[1, 2, 3, 4]), &budget);
    assert!(matches!(reader.u32(), Err(StopReason::SourceBytes)));
    assert_eq!(reader.position(), 0);
    assert_eq!(budget.bytes_read(), 0);
    clock.cancel();
    assert_eq!(reader.seek(1), Err(StopReason::Cancelled));
    assert_eq!(reader.position(), 0);
}

#[test]
fn scan_distinguishes_unstarted_empty_and_bounded_prefix() {
    let clock = ManualClock::default();
    let budget = ScanBudget::new(
        ScanLimits {
            feature_count: 1,
            ..ScanLimits::default()
        },
        &clock,
    );
    let mut scan = DesignScan::new(Some("test"));
    assert!(scan.census.is_none());
    scan.feature(&budget, "Extrude", Suppressed::No)
        .expect("scan test input");
    let result = scan.feature(&budget, "Revolve", Suppressed::Unknown);
    scan.finish(&budget, result);
    let census = scan.census.expect("scan test input");
    assert_eq!(census.rows.len(), 1);
    assert_eq!(census.rows[0].kind, "Extrude");
    assert_eq!(census.total, None);
    assert!(!census.complete);
    assert_eq!(scan.stop_reason, Some(StopReason::FeatureCount));
    let mut empty = DesignScan::new(Some("test"));
    empty.complete();
    assert_eq!(empty.census.expect("scan test input").total, Some(0));
    assert_eq!(empty.readability, Readability::KnownYes);
}

#[test]
fn scan_retained_depth_and_time_limits_refuse_before_work() {
    let clock = ManualClock::default();
    let budget = ScanBudget::new(
        ScanLimits {
            retained_bytes: 0,
            nesting_depth: 1,
            ..ScanLimits::default()
        },
        &clock,
    );
    assert_eq!(budget.retain(1), Err(StopReason::RetainedBytes));
    assert_eq!(budget.depth(2), Err(StopReason::NestingDepth));
    clock.set_elapsed(Duration::from_secs(5));
    assert_eq!(budget.check(), Err(StopReason::ElapsedTime));
}

struct StepClock(Cell<u64>);
impl ScanClock for StepClock {
    fn elapsed(&self) -> Duration {
        Duration::ZERO
    }
    fn is_cancelled(&self) -> bool {
        let visits = self.0.get();
        self.0.set(visits + 1);
        visits >= 8
    }
}

fn compressed() -> Vec<u8> {
    use std::io::Write;
    let mut writer = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
    writer.write_all(&[0; 16384]).expect("scan test input");
    writer.finish().expect("scan test input")
}

#[test]
fn scan_expansion_bomb_stops_at_the_expansion_ceiling() {
    let clock = ManualClock::default();
    let budget = ScanBudget::new(
        ScanLimits {
            expanded_bytes: 100,
            ..ScanLimits::default()
        },
        &clock,
    );
    assert_eq!(
        budget.inflate(View::over_retained(&compressed()), InflateFraming::Zlib),
        Err(StopReason::ExpandedBytes)
    );
}

#[test]
fn scan_inflate_checks_cancellation_between_bounded_steps() {
    let clock = StepClock(Cell::new(0));
    let budget = ScanBudget::new(ScanLimits::default(), &clock);
    assert_eq!(
        budget.inflate(View::over_retained(&compressed()), InflateFraming::Zlib),
        Err(StopReason::Cancelled)
    );
}

#[test]
fn scan_exact_expansion_budget_accepts_a_complete_stream() {
    let clock = ManualClock::default();
    let budget = ScanBudget::new(
        ScanLimits {
            expanded_bytes: 16384,
            ..ScanLimits::default()
        },
        &clock,
    );
    let (bytes, consumed) = budget
        .inflate(View::over_retained(&compressed()), InflateFraming::Zlib)
        .expect("scan test input");
    assert_eq!(bytes.len(), 16384);
    assert_eq!(consumed, compressed().len());
}

#[test]
fn scan_retained_members_have_distinct_error_address_spaces() {
    let clock = ManualClock::default();
    let budget = ScanBudget::new(ScanLimits::default(), &clock);
    let first = budget
        .view(&[1])
        .expect("scan test input")
        .req_u32_le()
        .expect_err("short field");
    let second = budget
        .view(&[2])
        .expect("scan test input")
        .req_u32_le()
        .expect_err("short field");
    assert_ne!(first.location.space, second.location.space);
    assert_eq!(first.location.offset, 0);
    assert_eq!(second.location.offset, 0);
}

#[test]
fn scan_keeps_the_resource_reason_through_dependency_error_wrapping() {
    let clock = ManualClock::default();
    let budget = ScanBudget::new(
        ScanLimits {
            source_bytes: 0,
            ..ScanLimits::default()
        },
        &clock,
    );
    assert_eq!(budget.source(1), Err(StopReason::SourceBytes));
    let mut scan = DesignScan::new(Some("test"));
    scan.finish(
        &budget,
        Err(StopReason::Malformed("wrapped reader error".into())),
    );
    assert_eq!(scan.stop_reason, Some(StopReason::SourceBytes));
}

fn raw_zstd(plain: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0x28, 0xb5, 0x2f, 0xfd, 0xe0];
    bytes.extend_from_slice(
        &u64::try_from(plain.len())
            .expect("frame length")
            .to_le_bytes(),
    );
    let header = (u32::try_from(plain.len()).expect("raw block length") << 3) | 1;
    bytes.extend_from_slice(&header.to_le_bytes()[..3]);
    bytes.extend_from_slice(plain);
    bytes
}

#[test]
fn scan_zstd_raw_frame_is_complete_at_the_exact_expansion_limit() {
    let clock = ManualClock::default();
    let budget = ScanBudget::new(
        ScanLimits {
            expanded_bytes: 16384,
            ..ScanLimits::default()
        },
        &clock,
    );
    let plain = [7; 16384];
    let compressed = raw_zstd(&plain);
    let (output, consumed) = budget
        .inflate(View::over_retained(&compressed), InflateFraming::Zstd)
        .expect("bounded frame");
    assert_eq!(output, plain);
    assert_eq!(consumed, compressed.len());
}

#[test]
fn scan_zstd_declared_expansion_is_refused_before_decoder_allocation() {
    let clock = ManualClock::default();
    let budget = ScanBudget::new(
        ScanLimits {
            expanded_bytes: 100,
            ..ScanLimits::default()
        },
        &clock,
    );
    let compressed = raw_zstd(&[7; 16384]);
    assert_eq!(
        budget.inflate(View::over_retained(&compressed), InflateFraming::Zstd),
        Err(StopReason::ExpandedBytes)
    );
}

#[test]
fn scan_zstd_checks_cancellation_between_output_steps() {
    let clock = StepClock(Cell::new(0));
    let budget = ScanBudget::new(ScanLimits::default(), &clock);
    let compressed = raw_zstd(&[7; 16384]);
    assert_eq!(
        budget.inflate(View::over_retained(&compressed), InflateFraming::Zstd),
        Err(StopReason::Cancelled)
    );
}
