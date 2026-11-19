// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::View;
use cadmpeg_core::scan::{ManualClock, ScanLimits, StopReason};

#[test]
fn scan_registry_unrecognized_input_keeps_census_unstarted() {
    let scan = crate::scan_design(
        View::over_retained(b"unrecognized source"),
        ScanLimits::default(),
        &ManualClock::default(),
    );
    assert_eq!(scan.stop_reason, Some(StopReason::Unrecognized));
    assert!(scan.census.is_none());
    assert_eq!(scan.bytes_read, 19);
}

#[test]
fn scan_registry_cancellation_precedes_detection() {
    let clock = ManualClock::default();
    clock.cancel();
    let scan = crate::scan_design(
        View::over_retained(b"source"),
        ScanLimits::default(),
        &clock,
    );
    assert_eq!(scan.stop_reason, Some(StopReason::Cancelled));
    assert_eq!(scan.bytes_read, 0);
}

#[cfg(feature = "step")]
#[test]
fn scan_registry_unsupported_reader_does_not_decode() {
    let bytes = b"ISO-10303-21;\nHEADER;\nFILE_SCHEMA(('AUTOMOTIVE_DESIGN'));\nENDSEC;\nDATA; deliberately unreadable geometry";
    let scan = crate::scan_design(
        View::over_retained(bytes),
        ScanLimits::default(),
        &ManualClock::default(),
    );
    assert_eq!(scan.format.as_deref(), Some("step"));
    assert!(matches!(scan.stop_reason, Some(StopReason::Unsupported(_))));
    assert!(scan.census.is_none());
    assert_eq!(scan.bytes_read, u64::try_from(bytes.len()).unwrap());
}

#[cfg(feature = "sldprt")]
#[test]
fn scan_registry_dispatches_a_public_history_fixture() {
    let bytes =
        include_bytes!("../../../cadmpeg-codec-sldprt/tests/golden/fixtures/body_history.sldprt");
    let scan = crate::scan_design(
        View::over_retained(bytes),
        ScanLimits::default(),
        &ManualClock::default(),
    );
    assert_eq!(scan.format.as_deref(), Some("sldprt"));
    assert_eq!(scan.stop_reason, None);
    assert!(scan.census.unwrap().complete);
}

#[test]
fn scan_registry_compound_without_a_scan_reader_is_unsupported() {
    let bytes = cadmpeg_test_support::compound::compound_fixture();
    let scan = crate::scan_design(
        View::over_retained(&bytes),
        ScanLimits::default(),
        &ManualClock::default(),
    );
    assert!(matches!(scan.stop_reason, Some(StopReason::Unsupported(_))));
    assert!(scan.census.is_none());
    assert!(scan.bytes_read < u64::try_from(bytes.len()).expect("fixture length"));
}

#[cfg(all(feature = "inventor", feature = "sldprt"))]
#[test]
fn scan_registry_compound_probes_follow_codec_registration() {
    use cadmpeg_container::compound::scan::ScanCompoundEntry;
    let entries = [
        ScanCompoundEntry {
            directory_id: 1,
            path: "RSeStorage/RSeSegInfo".into(),
            declared_size: 0,
        },
        ScanCompoundEntry {
            directory_id: 2,
            path: "ISolidWorksInformation".into(),
            declared_size: 0,
        },
    ];
    let clock = ManualClock::default();
    let budget = cadmpeg_core::scan::ScanBudget::new(ScanLimits::default(), &clock);
    let catalog = crate::InputCatalog::with_builtins();
    let mut matches = Vec::new();
    for descriptor in catalog.descriptors() {
        if descriptor
            .scan_compound_match(&entries, &budget)
            .expect("bounded name probe")
        {
            matches.push(descriptor.format_id().as_str());
        }
    }
    assert_eq!(matches, ["inventor", "sldprt"]);
}

#[test]
fn scan_registry_neutral_reader_is_unsupported() {
    let scan = crate::scan_design(
        View::over_retained(b"{}"),
        ScanLimits::default(),
        &ManualClock::default(),
    );
    assert_eq!(scan.format.as_deref(), Some("cadir"));
    assert!(matches!(scan.stop_reason, Some(StopReason::Unsupported(_))));
    assert!(scan.census.is_none());
}
