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
            .map(crate::native::EntryRecord::name)
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["Document.xml", "peer.bin"])
    );
    let parts = crate::test_support::source_fidelity::given_up(decoded.source_fidelity());
    let [(_, record)] = parts.as_slice() else {
        panic!("one given-up part: {parts:?}")
    };
    assert_eq!(record.data(), Some(b"\x07".as_slice()));
    assert_eq!(
        crate::source_archive::image(decoded.source_fidelity()).unwrap(),
        bytes
    );
    assert!(!decoded
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arenas()
        .contains_key("unreadable_entries"));
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
    use cadmpeg_ir::document::ArenaClass;
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
        matches!(output.report().synthesis_cause(), Some(SynthesisCause::SourceUnrepresented { classes })
        if classes.contains(&ArenaClass::Model))
    );
}
