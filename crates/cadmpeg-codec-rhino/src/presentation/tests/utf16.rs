// SPDX-License-Identifier: Apache-2.0
//! Legacy font-face UTF-16 replacement and admission.

use crate::chunks::ArchiveVersion;
use crate::presentation::{parse_text_style, TextStyleParseInput};

fn replacement_face_bytes() -> Vec<u8> {
    let mut bytes = super::legacy_text_style_bytes();
    let face_start = 1 + 4 + super::utf16_bytes("Helvetica Neue").len();
    bytes[face_start..face_start + 128].fill(0);
    bytes[face_start..face_start + 2].copy_from_slice(&0xd800_u16.to_le_bytes());
    bytes
}

#[test]
fn legacy_font_face_replacement_decodes_the_replacement_character() {
    let bytes = replacement_face_bytes();
    let parsed = parse_text_style(
        &cadmpeg_test_support::service_decode_context(),
        &bytes,
        TextStyleParseInput {
            range: 0..bytes.len(),
            archive: ArchiveVersion::V8,
            writer_version: Some(201_802_231),
            apple_runtime: false,
            source_offset: 42,
        },
        &mut Vec::new(),
    )
    .expect("legacy replacement field");
    assert_eq!(parsed.font.windows_logfont_name, "�");
    assert_eq!(parsed.font.postscript_name, "Helvetica Neue");
}
