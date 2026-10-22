// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;

use super::super::{
    parse_header, parse_property, parse_property_set_stream_with, parse_section_with, PropertyValue,
};

#[test]
fn callbacks_construct_values_from_original_nonzero_origin_views() {
    let payload = super::fixture();
    let mut bytes = b"prefix!!".to_vec();
    bytes.extend_from_slice(&payload);
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("actual source context");
    let source = root.child(8, bytes.len()).expect("actual property stream");
    let parsed = parse_property_set_stream_with(
        &ctx,
        source,
        |header| {
            assert_eq!(header.start(), 8);
            assert_eq!(header.window().len(), 48);
            ctx.record("header", header, || parse_header(header))
        },
        |ctx, section, fmtid| {
            assert_eq!(section.start(), 56);
            assert_eq!(section.end(), bytes.len());
            ctx.record("section", section, || {
                parse_section_with(ctx, section, fmtid, |ctx, raw, id, name, code_page| {
                    assert!(raw.start() >= section.start() + 32);
                    assert!(raw.end() <= section.end());
                    ctx.record(&format!("property-{id}"), raw, || {
                        parse_property(ctx, raw, id, name, code_page).map(Some)
                    })
                })
                .map(Some)
            })
        },
    )
    .expect("actual callbacks parse metadata");
    assert_eq!(parsed.sections.len(), 1);
    assert_eq!(parsed.sections[0].properties.len(), 3);
    assert!(matches!(
        &parsed.sections[0].properties[1].value,
        PropertyValue::String { value, .. } if value == "Synthetic title"
    ));
    assert!(parsed.sections[0].properties[2]
        .raw
        .window()
        .ends_with(b"synthetic\0\0\0"));
    let captures = ctx.take_record_captures().expect("one final extraction");
    assert_eq!(captures.len(), 5);
    for capture in captures {
        let (_, _, offset, _, reads) = capture.into_parts();
        assert!(offset >= 8);
        assert!(!reads.is_empty());
    }
}

#[test]
fn callback_error_is_not_reparsed_or_converted_to_an_issue() {
    let bytes = super::fixture();
    let arena = DecodeArena::new();
    let (ctx, source) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("source context");
    let result = parse_property_set_stream_with(&ctx, source, parse_header, |_, section, _| {
        assert_eq!(section.start(), 48);
        Err(CodecError::malformed(
            "source-owned section callback refused",
        ))
    });
    assert!(
        matches!(result, Err(CodecError::Malformed(detail)) if detail == "source-owned section callback refused")
    );
}

#[test]
fn omitted_property_keeps_the_exact_readable_property_peer() {
    let bytes = super::fixture();
    let arena = DecodeArena::new();
    let (ctx, source) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("source context");
    let parsed =
        parse_property_set_stream_with(&ctx, source, parse_header, |ctx, section, fmtid| {
            parse_section_with(ctx, section, fmtid, |ctx, raw, id, name, code_page| {
                if id == 2 {
                    return Ok(None);
                }
                parse_property(ctx, raw, id, name, code_page).map(Some)
            })
            .map(Some)
        })
        .expect("independent property callbacks");
    assert_eq!(
        parsed.sections[0]
            .properties
            .iter()
            .map(|p| p.id)
            .collect::<Vec<_>>(),
        [1, 17]
    );
    assert!(matches!(
        &parsed.sections[0].properties[1].value,
        PropertyValue::Binary { value, .. } if value.window() == b"\x89PNG\r\n\x1a\nsynthetic"
    ));
}
