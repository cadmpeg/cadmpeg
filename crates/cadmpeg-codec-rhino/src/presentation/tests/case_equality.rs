// SPDX-License-Identifier: Apache-2.0

use crate::chunks::ArchiveVersion;

#[test]
fn text_style_description_keyword_is_ascii_case_insensitive() {
    let mut bytes = vec![0x12];
    bytes.extend(7_i32.to_le_bytes());
    bytes.extend(super::utf16_bytes("DEFAULT"));
    bytes.extend([0_u8; 128]);
    bytes.extend(700_i32.to_le_bytes());
    bytes.extend(1_i32.to_le_bytes());
    bytes.extend(1.6_f64.to_le_bytes());
    bytes.extend([0x11; 16]);
    let style = crate::presentation::parse_text_style(
        &cadmpeg_test_support::service_decode_context(),
        &bytes,
        crate::presentation::TextStyleParseInput {
            range: 0..bytes.len(),
            archive: ArchiveVersion::V8,
            writer_version: Some(201_802_231),
            apple_runtime: false,
            source_offset: 42,
        },
        &mut Vec::new(),
    )
    .expect("valid legacy style");
    assert_eq!(style.font_description, "DEFAULT");
    assert!(style.font.postscript_name.is_empty());
}
