// SPDX-License-Identifier: Apache-2.0
use super::*;
use cadmpeg_core::scan::{ManualClock, ScanLimits};

#[test]
fn scan_compound_opens_selected_prefixes_after_navigation() {
    let source = cadmpeg_test_support::compound::compound_fixture();
    let clock = ManualClock::default();
    let budget = ScanBudget::new(ScanLimits::default(), &clock);
    let compound = ScanCompound::new(View::over_retained(&source), &budget).expect("scan fixture");
    let entry = &compound.entries()[0];
    assert_eq!(entry.path, "Payload");
    assert_eq!(entry.declared_size, 4096);
    assert_eq!(
        compound.prefix(entry, 16).expect("scan fixture"),
        [0x5a; 16]
    );
    assert!(budget.bytes_read() < u64::try_from(source.len()).expect("scan fixture"));
}

#[test]
fn scan_compound_source_limit_precedes_a_stream_read() {
    let source = cadmpeg_test_support::compound::compound_fixture();
    let clock = ManualClock::default();
    let budget = ScanBudget::new(
        ScanLimits {
            source_bytes: 1536,
            ..ScanLimits::default()
        },
        &clock,
    );
    let compound = ScanCompound::new(View::over_retained(&source), &budget).expect("scan fixture");
    assert_eq!(
        compound.open(&compound.entries()[0]),
        Err(StopReason::SourceBytes)
    );
    assert_eq!(budget.bytes_read(), 1536);
}
