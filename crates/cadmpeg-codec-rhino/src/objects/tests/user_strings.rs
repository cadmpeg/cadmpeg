// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::objects::{parse_user_string_list, UserStringSelection};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

fn list(entries: &[(&str, &str)]) -> Vec<u8> {
    let archive = ArchiveVersion::V5;
    let mut body = i32::try_from(entries.len()).unwrap().to_le_bytes().to_vec();
    for (key, value) in entries {
        let mut entry = utf16_bytes(key);
        entry.extend(utf16_bytes(value));
        body.extend(anonymous_chunk(archive, 0, &entry));
    }
    anonymous_chunk(archive, 0, &body)
}

#[test]
fn selected_user_strings_skip_only_the_first_temporary_entry() {
    let bytes = list(&[
        ("$TEMP_OBJECT$", "discarded"),
        ("$temp_object$", "second"),
        ("Name", "stored"),
    ]);
    let ctx = cadmpeg_test_support::service_decode_context();
    let parsed = parse_user_string_list(
        &ctx,
        &bytes,
        0..bytes.len(),
        ArchiveVersion::V5,
        UserStringSelection::ExcludeFirstTempObject,
    )
    .unwrap();
    assert_eq!(
        parsed.entries,
        [
            ("$temp_object$".to_owned(), "second".to_owned()),
            ("Name".to_owned(), "stored".to_owned())
        ]
    );
}

#[test]
fn discarded_user_strings_validate_without_retaining_text_or_slots() {
    let value = "😀".repeat(137);
    let bytes = list(&[("$TEMP_OBJECT$", &value)]);
    for selection in [
        UserStringSelection::ExcludeFirstTempObject,
        UserStringSelection::ValidateOnly,
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
        let parsed =
            parse_user_string_list(&ctx, &bytes, 0..bytes.len(), ArchiveVersion::V5, selection)
                .unwrap();
        assert!(parsed.entries.is_empty());
    }
    cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "validate Rhino deferred UTF-16",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
            parse_user_string_list(
                &ctx,
                &bytes,
                0..bytes.len(),
                ArchiveVersion::V5,
                UserStringSelection::ExcludeFirstTempObject,
            )
            .map(|_| ())
            .map_err(|error| match error {
                crate::chunks::FramingError::Resource(limit) => {
                    cadmpeg_core::CodecError::ResourceLimit(limit)
                }
                other => panic!("valid user strings failed: {other:?}"),
            })
        },
    );
}

#[test]
fn discarded_user_strings_still_reject_invalid_surrogates() {
    let archive = ArchiveVersion::V5;
    let mut entry = utf16_bytes("$TEMP_OBJECT$");
    entry.extend(2_u32.to_le_bytes());
    entry.extend([0x00, 0xd8, 0x00, 0x00]);
    let mut body = 1_i32.to_le_bytes().to_vec();
    body.extend(anonymous_chunk(archive, 0, &entry));
    let bytes = anonymous_chunk(archive, 0, &body);
    for selection in [
        UserStringSelection::ExcludeFirstTempObject,
        UserStringSelection::ValidateOnly,
    ] {
        let ctx = cadmpeg_test_support::service_decode_context();
        assert!(
            matches!(parse_user_string_list(&ctx, &bytes, 0..bytes.len(), archive, selection), Err(crate::chunks::FramingError::Structural { ref message, .. }) if message == "invalid UTF-16 surrogate sequence")
        );
    }
}

#[test]
fn user_string_staging_slots_are_scoped_and_strings_are_retained() {
    let bytes = list(&[("key", "value")]);
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::MaterializedBytes,
        "Rhino user-string entries",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_materialized_bytes = cap;
            // Only the three key bytes and five value bytes stay retained.
            policy.limits.max_retained_bytes = 8;
            let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
            parse_user_string_list(
                &ctx,
                &bytes,
                0..bytes.len(),
                ArchiveVersion::V5,
                UserStringSelection::All,
            )
            .map(|_| ())
            .map_err(|error| match error {
                crate::chunks::FramingError::Resource(limit) => {
                    cadmpeg_core::CodecError::ResourceLimit(limit)
                }
                other => panic!("valid user strings failed: {other:?}"),
            })
        },
    );
    let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
        panic!("staging refusal");
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = limit.additional;
    policy.limits.max_retained_bytes = 8;
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let parsed = parse_user_string_list(
        &ctx,
        &bytes,
        0..bytes.len(),
        ArchiveVersion::V5,
        UserStringSelection::All,
    )
    .unwrap();
    assert_eq!(parsed.entries, [("key".to_owned(), "value".to_owned())]);
    drop(parsed);
    ctx.reserve_scoped(limit.additional, "reclaimed user-string entries")
        .unwrap();
}
