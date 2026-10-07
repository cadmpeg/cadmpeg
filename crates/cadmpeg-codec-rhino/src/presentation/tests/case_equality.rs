// SPDX-License-Identifier: Apache-2.0

use crate::chunks::{ArchiveVersion, FramingError};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn temporary_user_string_case_equality_preserves_refusal() {
    let payload = super::anonymous(
        0,
        &[
            1_i32.to_le_bytes().as_slice(),
            super::anonymous(
                0,
                &[
                    super::utf16_bytes("$TEMP_OBJECT$"),
                    super::utf16_bytes("temporary"),
                ]
                .concat(),
            )
            .as_slice(),
        ]
        .concat(),
    );
    let descriptors = [crate::objects::AttributeUserdataDescriptor::Known(
        crate::objects::AttributeUserdata {
            range: 0..payload.len(),
            class_uuid: crate::objects::USER_STRING_LIST,
            item_uuid: crate::objects::USER_STRING_LIST,
            application_uuid: None,
            writer_version: None,
            payload_range: 0..payload.len(),
        },
    )];
    cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "Rhino temporary user string key case equality",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&payload, &arena, &policy).unwrap();
            let result = crate::presentation::first_user_string_records(
                &ctx,
                &payload,
                ArchiveVersion::V8,
                &[],
                &descriptors,
                0,
                &mut Vec::new(),
            )
            .map(|_| ());
            if let Err(CodecError::ResourceLimit(refusal)) = &result {
                assert_eq!(ctx.resource_refusal().as_ref(), Some(refusal));
            }
            result
        },
    );
}

#[test]
fn text_style_description_case_equality_preserves_refusal() {
    // Equal-length descriptions reach the admitted ASCII comparison scan.
    let mut bytes = vec![0x12];
    bytes.extend(7_i32.to_le_bytes());
    bytes.extend(super::utf16_bytes("DEFAULT"));
    bytes.extend([0_u8; 128]);
    bytes.extend(700_i32.to_le_bytes());
    bytes.extend(1_i32.to_le_bytes());
    bytes.extend(1.6_f64.to_le_bytes());
    bytes.extend([0x11; 16]);
    cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "Rhino text style description case equality",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
            let result = crate::presentation::parse_text_style(
                &ctx,
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
            .map(|_| ())
            .map_err(|error| match error {
                FramingError::Resource(refusal) => CodecError::ResourceLimit(refusal),
                other => panic!("valid text style returned {other:?}"),
            });
            if let Err(CodecError::ResourceLimit(refusal)) = &result {
                assert_eq!(ctx.resource_refusal().as_ref(), Some(refusal));
            }
            result
        },
    );
}
