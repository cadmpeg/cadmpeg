// SPDX-License-Identifier: Apache-2.0
//! Shared source-member failure homes through public decode and output planning.

use crate::test_support::test_archive::broken_optional_member_archive as archive;
use cadmpeg_ir::{Codec, DecodeOptions};
use cadmpeg_test_support::zip_fixture::{self, RawEntry};
use std::collections::BTreeSet;
use std::io::Cursor;

#[test]
fn the_real_codec_composes_shared_unknowns_and_retains_successful_member_records() {
    let bytes = archive();
    let decoded = crate::FcstdCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .unwrap();
    let entries = decoded
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::EntryRecord>("entries")
        .unwrap();
    assert_eq!(
        entries
            .iter()
            .map(|entry| entry.name())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["Document.xml", "peer.bin"])
    );
    let unknowns = decoded.ir().native_unknowns("fcstd").unwrap();
    let [unknown] = unknowns.as_slice() else {
        panic!("one unknown: {unknowns:?}")
    };
    assert_eq!(
        decoded
            .source_fidelity()
            .retained_record(unknown.id.as_str())
            .unwrap()
            .data(),
        Some(b"\x07".as_slice())
    );
    assert_eq!(
        crate::source_archive::image(decoded.source_fidelity()).unwrap(),
        bytes
    );
    assert!(decoded
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<serde_json::Value>("unreadable_entries")
        .unwrap()
        .is_empty());
    assert!(crate::test_support::validate_native(decoded.ir()).is_empty());
    assert_eq!(
        decoded
            .report()
            .losses
            .iter()
            .filter(|note| note.code.local_code() == "record.unreadable")
            .count(),
        1
    );
}

#[test]
fn source_repacking_cannot_silently_omit_a_shared_given_up_member() {
    use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder, SynthesisCause};
    use cadmpeg_ir::report::export::WritePath;
    let bytes = zip_fixture::archive(&[
        RawEntry::stored("Document.xml", b"<Document SchemaVersion='4' FileVersion='1'><Objects Count='0'/><ObjectData Count='0'/></Document>"),
        RawEntry::compressed("broken.bin", zip_fixture::DEFLATE, b"\x07", b"not decoded"),
    ]);
    let decoded = crate::FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let output = crate::FcstdCodec
        .plan(
            EncodeInput::new(decoded.ir(), Some(decoded.source_fidelity())),
            TargetRequest::Inherit,
        )
        .unwrap();
    assert!(matches!(
        output.report().write_path(),
        WritePath::Synthesized { .. }
    ));
    assert!(
        matches!(output.report().synthesis_cause(), Some(SynthesisCause::PatchDeclined { reason })
        if reason.contains("cannot preserve unreadable ZIP payloads"))
    );
}
