// SPDX-License-Identifier: Apache-2.0
//! Source ledgers remain typed fidelity facts, without synthetic source images.

use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;

#[test]
fn whole_source_ledgers_keep_their_facts_without_json_images_or_fabricated_streams() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="0"/><ObjectData Count="0"/></Document>"#;
    let bytes = crate::test_support::test_archive::archive_entries(&[
        ("Document.xml", document.as_bytes()),
        ("peer.bin", b"abc"),
    ]);
    let decoded = crate::FcstdCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .unwrap();
    let fidelity = decoded.source_fidelity();
    assert_eq!(fidelity.freecad_ledgers().len(), 1);
    let ledgers = &fidelity.freecad_ledgers()["file"];
    assert_eq!(
        ledgers.byte_coverage.physical_byte_len,
        u64::try_from(bytes.len()).unwrap()
    );
    assert_eq!(ledgers.byte_coverage.logical_entry_count, 2);
    assert_eq!(
        ledgers.byte_coverage.logical_byte_len,
        u64::try_from(document.len() + 3).unwrap()
    );
    assert!(ledgers.byte_coverage.exact);
    assert_eq!(ledgers.physical_ledger.first().unwrap().span.start(), 0);
    assert_eq!(
        ledgers.physical_ledger.last().unwrap().span.end(),
        u64::try_from(bytes.len()).unwrap()
    );
    assert!(ledgers
        .physical_ledger
        .windows(2)
        .all(|pair| pair[0].span.end() == pair[1].span.start()));
    let peer = ledgers
        .logical_ledger
        .iter()
        .find(|span| span.entry == "peer.bin")
        .unwrap();
    assert_eq!((peer.span.start(), peer.span.end()), (0, 3));
    assert_eq!(peer.classification.as_str(), "named_opaque");
    assert_eq!(
        peer.classification.owner(),
        Some("fcstd:native:entry#peer.bin")
    );
    for name in ["physical-ledger", "logical-ledger", "byte-coverage"] {
        assert!(fidelity
            .retained_record(&crate::native::native_id("source-layout", name))
            .is_none());
    }
    assert!(fidelity
        .retained_records()
        .values()
        .all(|record| record.stream() != "fcstd:source:layout#archive"));
    assert!(ledgers
        .physical_ledger
        .iter()
        .all(|span| !fidelity.record_layout().contains_key(&span.id)));
    assert!(ledgers
        .logical_ledger
        .iter()
        .all(|span| !fidelity.record_layout().contains_key(&span.id)));
    let restored: cadmpeg_ir::SourceFidelity =
        serde_json::from_value(serde_json::to_value(fidelity).unwrap()).unwrap();
    assert_eq!(restored.freecad_ledgers(), fidelity.freecad_ledgers());
}
