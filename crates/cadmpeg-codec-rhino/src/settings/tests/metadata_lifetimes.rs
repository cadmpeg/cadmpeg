// SPDX-License-Identifier: Apache-2.0
//! Recoverable candidates and replaced singleton values release their backing.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use crate::chunks::{ArchiveVersion, FramingError};
use crate::container::Record;
use crate::loss::Diagnostics;
use crate::settings;
use crate::test_support::test_dump::{anonymous_chunk, crc_chunk, utf16_bytes};

const NAME_BYTES: usize = 4096;

fn applications(values: &[(char, bool)]) -> (Vec<u8>, Vec<Record>) {
    let mut data = Vec::new();
    let mut records = Vec::new();
    for &(letter, complete) in values {
        let start = data.len();
        data.push(0x10);
        data.extend(utf16_bytes(&letter.to_string().repeat(NAME_BYTES)));
        if complete {
            data.extend(utf16_bytes(""));
            data.extend(utf16_bytes(""));
        }
        records.push(Record::long(settings::APPLICATION, start..data.len(), start..data.len()));
    }
    (data, records)
}

#[test]
fn failed_application_attempts_release_text_before_later_metadata() {
    let (data, records) = applications(&[('A', false), ('B', false), ('C', false), ('D', true)]);
    let tables = [super::metadata_table(settings::PROPERTIES, data.len(), records)];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // Only one decoded name is live during the sequence of rejected attempts.
    policy.limits.max_materialized_bytes = u64::try_from(NAME_BYTES).unwrap();
    let (ctx, _) = DecodeContext::from_root_bytes(&data, &arena, &policy).unwrap();
    let mut warnings = Diagnostics::new();
    let metadata = settings::parse_metadata(&ctx, &data, ArchiveVersion::V8, &tables, &mut warnings).unwrap();
    assert_eq!(metadata.properties.application.unwrap().name, "D".repeat(NAME_BYTES));
    assert_eq!(metadata.opaque_records.len(), 3);
    assert_eq!(warnings.len(), 3);
    assert_eq!(ctx.resource_refusal(), None);
    let released = ctx.reserve_scoped(u64::try_from(NAME_BYTES).unwrap(), "application scratch released").unwrap();
    drop(released);
    ctx.finish_session().unwrap();
}

#[test]
fn replacing_application_releases_each_previous_singleton() {
    let (data, records) = applications(&[('A', true), ('B', true), ('C', true), ('D', true)]);
    let tables = [super::metadata_table(settings::PROPERTIES, data.len(), records)];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // A replacement temporarily overlaps the previous name, then releases it.
    policy.limits.max_materialized_bytes = u64::try_from(2 * NAME_BYTES).unwrap();
    let (ctx, _) = DecodeContext::from_root_bytes(&data, &arena, &policy).unwrap();
    let mut warnings = Diagnostics::new();
    let metadata = settings::parse_metadata(&ctx, &data, ArchiveVersion::V8, &tables, &mut warnings).unwrap();
    assert_eq!(metadata.properties.application.unwrap().name, "D".repeat(NAME_BYTES));
    assert!(metadata.opaque_records.is_empty());
    assert_eq!(warnings.len(), 3);
    assert!(warnings.iter().all(|warning| warning.message == "duplicate singleton metadata record 0x20008024; later record wins"));
    assert_eq!(ctx.resource_refusal(), None);
    let released = ctx.reserve_scoped(u64::try_from(2 * NAME_BYTES).unwrap(), "singleton scratch released").unwrap();
    drop(released);
    ctx.finish_session().unwrap();
}

#[test]
fn failed_layer_extensions_release_the_provisional_vector() {
    let archive = ArchiveVersion::V8;
    let mut invalid_entry = 2_i32.to_le_bytes().to_vec();
    invalid_entry.extend(0_i32.to_le_bytes());
    let entry = crc_chunk(archive, 0x4000_8000, &invalid_entry);
    let mut body = 1_i32.to_le_bytes().to_vec();
    body.extend(entry);
    let data = anonymous_chunk(archive, 0, &body);
    let descriptor = crate::objects::ClassUserdata {
        class_uuid: settings::LAYER_EXTENSIONS,
        item_uuid: settings::LAYER_EXTENSIONS,
        application_uuid: None,
        version: (2, 1),
        save_context: None,
        copy_count: 1,
        transform_range: 0..0,
        range: 0..data.len(),
        payload_range: 0..data.len(),
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // The allowance fits the temporary vector, including its spare capacity.
    let vector_bytes = 4096;
    policy.limits.max_materialized_bytes = vector_bytes;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&data, &arena, &policy).unwrap();
    for _ in 0..3 {
        let error = settings::parse_layer_extensions(&ctx, &data, &descriptor, archive, None).unwrap_err();
        assert!(matches!(&error, FramingError::Structural { message, .. } if message == "layer extensions entry version is unsupported"), "{error:?}");
        assert_eq!(ctx.resource_refusal(), None);
        let released = ctx.reserve_scoped(vector_bytes, "extension scratch released").unwrap();
        drop(released);
    }
    ctx.finish_session().unwrap();
}
