// SPDX-License-Identifier: Apache-2.0
//! Legacy font-face UTF-16 replacement and admission.

use crate::chunks::{ArchiveVersion, FramingError};
use crate::presentation::{parse_text_style, TextStyleParseInput};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

fn replacement_face_bytes() -> Vec<u8> {
    let mut bytes = super::legacy_text_style_bytes();
    let face_start = 1 + 4 + super::utf16_bytes("Helvetica Neue").len();
    bytes[face_start..face_start + 128].fill(0);
    bytes[face_start..face_start + 2].copy_from_slice(&0xd800_u16.to_le_bytes());
    bytes
}

#[test]
fn legacy_font_face_replacement_refuses_exact_retained_limit() {
    let bytes = replacement_face_bytes();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 16;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let result = parse_text_style(
        &ctx,
        &bytes,
        TextStyleParseInput {
            range: 0..bytes.len(),
            archive: ArchiveVersion::V8,
            writer_version: Some(201_802_231),
            apple_runtime: false,
            source_offset: 42,
        },
        &mut Vec::new(),
    );
    assert!(matches!(result, Err(FramingError::Resource(limit))
        if limit.dimension == ResourceDimension::RetainedBytes && limit.used == 14 && limit.additional == 3 && limit.operation == "Rhino legacy font face"));
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

#[test]
fn legacy_font_face_refuses_work_before_scan() {
    let bytes = replacement_face_bytes();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // The fourteen-character description has two 28-byte scans and fourteen UTF-8 output bytes.
    policy.limits.max_work_units = 2 * 2 * 14 + 14;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let result = parse_text_style(
        &ctx,
        &bytes,
        TextStyleParseInput {
            range: 0..bytes.len(),
            archive: ArchiveVersion::V8,
            writer_version: Some(201_802_231),
            apple_runtime: false,
            source_offset: 42,
        },
        &mut Vec::new(),
    );
    assert!(matches!(result, Err(FramingError::Resource(limit))
        if limit.dimension == ResourceDimension::WorkUnits && limit.operation == "Rhino legacy font face"));
}
