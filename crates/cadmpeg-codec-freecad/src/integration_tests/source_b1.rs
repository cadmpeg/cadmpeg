// SPDX-License-Identifier: Apache-2.0
//! Genuine automatic classification for the adopted whole source inventory.

use cadmpeg_ir::{ArenaClass, Codec, DecodeOptions};
use std::io::Cursor;

#[test]
fn an_opaque_member_has_an_automatic_b1_span_and_no_per_entry_evidence_record() {
    let document = "<Document SchemaVersion='4' FileVersion='1'><Objects Count='1'><Object name='Owner' type='App::Feature'/></Objects><ObjectData Count='1'><Object name='Owner'><Properties Count='1'><Property name='Included' type='App::PropertyFileIncluded'><FileIncluded file='shared.bin'/></Property></Properties></Object></ObjectData></Document>";
    let payload = b"source bytes shared by application references";
    let bytes = crate::test_support::test_archive::archive_entries(&[
        ("Document.xml", document.as_bytes()),
        ("shared.bin", payload),
    ]);
    let decoded = crate::FcstdCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .unwrap();
    let fidelity = decoded.source_fidelity();
    let id = crate::native::native_id("entry", "shared.bin");
    let layout = &fidelity.record_layout()[&id];
    assert!(layout.reads().is_empty());
    assert!(fidelity.retained_record(&id).is_none());
    let spans = fidelity.b1_spans()[layout.stream()].spans();
    assert_eq!(spans.len(), 1);
    assert_eq!(spans[0].range(), 0..u64::try_from(payload.len()).unwrap());
    assert_eq!(
        spans[0].classes(),
        [ArenaClass::Model, ArenaClass::Design].into()
    );
    assert_eq!(fidelity.retained_records().len(), 1);
    assert_eq!(crate::source_archive::image(fidelity).unwrap(), bytes);
    let matching = decoded
        .report()
        .losses
        .iter()
        .filter(|loss| {
            loss.code.local_code() == "source.bytes-undecoded"
                && loss.provenance.as_ref().is_some_and(|provenance| {
                    provenance.stream_name() == Some(layout.stream())
                        && provenance.offset == 0
                        && provenance.byte_len() == Some(u64::try_from(payload.len()).unwrap())
                })
        })
        .count();
    assert_eq!(matching, 1);
    assert_eq!(
        crate::test_support::source_fidelity::entry_bytes(fidelity, &id),
        payload
    );
}
