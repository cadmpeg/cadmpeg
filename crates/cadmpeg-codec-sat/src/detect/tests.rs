// SPDX-License-Identifier: Apache-2.0
//! Detection and inspect tests for bare ASM streams.

use cadmpeg_ir::codec::{Codec, CodecBackend, Confidence};
use std::io::Cursor;

use crate::test_support::text_sphere_stream;
use crate::SatCodec;

#[test]
fn detection_is_content_based() {
    assert_eq!(SatCodec.detect(b"ASM BinaryFile8\x00"), Confidence::High);
    assert_eq!(SatCodec.detect(b"ACIS BinaryFile\x00"), Confidence::High);
    assert_eq!(
        SatCodec.detect(b"23200 0 2 2 \n16 Autodesk Neutron"),
        Confidence::Medium
    );
    assert_eq!(
        SatCodec.detect(b"700 0 6 0           \n30 Autodesk"),
        Confidence::Medium
    );
    // Numeric text without the four-word first line is not a stream.
    assert_eq!(SatCodec.detect(b"123 456\n789"), Confidence::No);
    assert_eq!(SatCodec.detect(b"ISO-10303-21;\nHEADER;"), Confidence::No);
    assert_eq!(SatCodec.detect(b"{\"ir_version\":\"5\"}"), Confidence::No);
}

#[test]
fn inspect_reports_the_stream_kind_and_header_facts() {
    let summary = SatCodec
        .inspect(
            &mut Cursor::new(text_sphere_stream(1.0)),
            &cadmpeg_core::decode::InspectOptions::default(),
        )
        .unwrap();
    assert_eq!(summary.format(), "sat");
    assert_eq!(summary.entries.len(), 1);
    assert_eq!(summary.entries[0].role, "brep-text");
    assert_eq!(
        summary.entries[0]
            .attributes
            .get("acis_save_format_version"),
        Some(&"23200".to_string())
    );
    assert_eq!(
        summary.entries[0].attributes.get("terminator"),
        Some(&"End-of-ASM-data".to_string())
    );
}
