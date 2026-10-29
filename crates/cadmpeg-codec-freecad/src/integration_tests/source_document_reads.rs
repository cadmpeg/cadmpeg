// SPDX-License-Identifier: Apache-2.0
//! Selected root metadata flows into native facts through exact source reads.

use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;

#[test]
fn selected_root_metadata_is_read_but_shadowed_aliases_and_extra_values_stay_b1() {
    let document = "<Document SchemaVersion='4' FileVersion='7' fileVersion='8' ProgramVersion='Ω &amp; version' programVersion='discarded' Vendor='unknown'><Objects Count='0'/><ObjectData Count='0'/></Document>";
    let bytes = crate::test_support::test_archive::archive(document);
    let decoded = crate::FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let native = decoded.ir().native.namespace("fcstd").unwrap();
    let facts = native
        .arena_as::<crate::native::DocumentFacts>("document")
        .unwrap();
    assert_eq!(facts[0].file_version.value(), Some(7));
    assert_eq!(facts[0].file_version.as_str(), "7");
    assert_eq!(facts[0].program_version.as_deref(), Some("Ω & version"));
}

#[test]
fn an_invalid_file_version_is_kept_with_no_numeric_read_credit_and_no_alias_substitution() {
    let document = "<Document SchemaVersion='4' FileVersion='bad' fileVersion='9' ProgramVersion='authored'><Objects Count='0'/><ObjectData Count='0'/></Document>";
    let bytes = crate::test_support::test_archive::archive(document);
    let decoded = crate::FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let facts = decoded
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::DocumentFacts>("document")
        .unwrap();
    assert_eq!(facts[0].file_version.as_str(), "bad");
    assert_eq!(facts[0].file_version.value(), None);
    assert_eq!(facts[0].program_version.as_deref(), Some("authored"));
}

#[test]
fn absent_root_metadata_keeps_existing_native_defaults_without_source_reads() {
    let document =
        "<Document SchemaVersion='4'><Objects Count='0'/><ObjectData Count='0'/></Document>";
    let bytes = crate::test_support::test_archive::archive(document);
    let decoded = crate::FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let facts = decoded
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::DocumentFacts>("document")
        .unwrap();
    assert_eq!(facts[0].file_version.as_str(), "0");
    assert_eq!(facts[0].program_version, None);
}
