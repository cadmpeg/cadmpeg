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

#[cfg(any(feature = "fcstd", feature = "pvz"))]
fn compressed_listing(name: &str, xml: &[u8]) -> Vec<u8> {
    use std::io::{Cursor, Write};
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    archive
        .start_file(
            name,
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated),
        )
        .expect("member");
    archive.write_all(xml).expect("XML");
    archive.finish().expect("directory").into_inner()
}

#[cfg(feature = "fcstd")]
#[test]
fn scan_registry_freecad_detection_uses_listing_without_expansion() {
    let mut xml = br#"<Document SchemaVersion="4">"#.to_vec();
    xml.extend_from_slice(&[b' '; 4096]);
    xml.extend_from_slice(b"</Document>");
    scan_compressed_listing_without_payload("Document.xml", &xml, "fcstd");
}

#[cfg(feature = "pvz")]
#[test]
fn scan_registry_pvz_detection_uses_listing_without_expansion() {
    let mut xml = br#"<PV_FILE type="PVS" version="0302">"#.to_vec();
    xml.extend_from_slice(&[b' '; 4096]);
    xml.extend_from_slice(b"</PV_FILE>");
    scan_compressed_listing_without_payload("model.pvs", &xml, "pvz");
}

#[cfg(any(feature = "fcstd", feature = "pvz"))]
fn scan_compressed_listing_without_payload(name: &str, xml: &[u8], format: &str) {
    let source = compressed_listing(name, xml);
    for source_bytes in [ScanLimits::default().source_bytes, 96] {
        let scan = crate::scan_design(
            View::over_retained(&source),
            ScanLimits {
                source_bytes,
                expanded_bytes: 0,
                ..ScanLimits::default()
            },
            &ManualClock::default(),
        );
        assert_eq!(scan.format.as_deref(), Some(format));
        assert!(matches!(scan.stop_reason, Some(StopReason::Unsupported(_))));
        assert!(scan.census.is_none());
        assert!(scan.bytes_read < u64::try_from(source.len()).expect("ZIP extent"));
    }
}

#[cfg(feature = "fcstd")]
#[test]
fn scan_registry_prefixed_zip_detection_keeps_payload_unexpanded() {
    let mut source = b"executable prefix".to_vec();
    source.extend(compressed_listing(
        "Document.xml",
        br#"<Document SchemaVersion="4"/>"#,
    ));
    let scan = crate::scan_design(
        View::over_retained(&source),
        ScanLimits {
            expanded_bytes: 0,
            ..ScanLimits::default()
        },
        &ManualClock::default(),
    );
    assert_eq!(scan.format.as_deref(), Some("fcstd"));
    assert!(matches!(scan.stop_reason, Some(StopReason::Unsupported(_))));
    assert!(scan.census.is_none());
}
