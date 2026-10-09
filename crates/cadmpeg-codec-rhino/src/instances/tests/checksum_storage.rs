// SPDX-License-Identifier: Apache-2.0

use std::ops::Range;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use crate::chunks::{ArchiveVersion, BoundedReader, FramingError};
use crate::instances::LinkSource;
use crate::loss::Diagnostics;
use crate::test_support::test_dump::{crc_chunk_excluding, long_chunk, v6_definition_payload};

const DEFINITION_TEXT_BYTES: usize =
    "modern definition".len() + "description".len() + "https://example.test".len() + "tag".len();
const RANGE_BYTES: usize = std::mem::size_of::<Range<usize>>();

#[test]
fn definition_checksum_storage_releases_before_retained_fields() {
    let data = v6_definition_payload(ArchiveVersion::V8, [7; 16], &[], 0, false, false);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // Four Range slots coexist with all four exact UTF-8 field buffers.
    policy.limits.max_materialized_bytes =
        u64::try_from(DEFINITION_TEXT_BYTES + 4 * RANGE_BYTES).unwrap();
    let (ctx, _) = DecodeContext::from_root_bytes(&data, &arena, &policy).unwrap();
    let mut field_storage = ctx.reserve_scoped(0, "instance test fields").unwrap();
    let mut warnings = Diagnostics::new();
    let definition = field_storage.with_storage(|| {
        crate::instances::parse_v6(
            &ctx, &data, 0..data.len(), 0..data.len(), ArchiveVersion::V8, &mut warnings,
        )
    }).unwrap();
    assert_eq!(definition.name, "modern definition");
    assert_eq!(definition.description, "description");
    assert_eq!(definition.url, "https://example.test");
    assert_eq!(definition.url_tag, "tag");
    assert_eq!(definition.id, crate::wire::Uuid::from_canonical([7; 16]));
    assert!(definition.members.is_empty());
    assert!(warnings.is_empty());
    let (next, next_storage) = ctx.temporary_vec::<Range<usize>>(4, "next actual checksum buffer")
        .expect("freed checksum slots are reusable while definition fields remain live");
    assert!(next.capacity() >= 4);
    drop(next);
    drop(next_storage);
    drop(definition);
    drop(warnings);
    drop(field_storage);
    ctx.finish_session().unwrap();
}

#[test]
fn definition_checksum_storage_keeps_original_live_peak_refusal() {
    let data = v6_definition_payload(ArchiveVersion::V8, [7; 16], &[], 0, false, false);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes =
        u64::try_from(DEFINITION_TEXT_BYTES + 4 * RANGE_BYTES - 1).unwrap();
    let (ctx, _) = DecodeContext::from_root_bytes(&data, &arena, &policy).unwrap();
    let mut field_storage = ctx.reserve_scoped(0, "instance test fields").unwrap();
    let error = field_storage.with_storage(|| {
        crate::instances::parse_v6(
            &ctx, &data, 0..data.len(), 0..data.len(), ArchiveVersion::V8, &mut Diagnostics::new(),
        )
    }).unwrap_err();
    let FramingError::Resource(limit) = error else {
        panic!("original live field allocation refusal");
    };
    assert_eq!(limit.operation, "Rhino instance URL tag");
    assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(limit.used, u64::try_from(DEFINITION_TEXT_BYTES - "tag".len() + 4 * RANGE_BYTES).unwrap());
    assert_eq!(limit.additional, u64::try_from("tag".len()).unwrap());
    drop(field_storage);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit));
}

#[test]
fn linked_checksum_storage_releases_both_nested_buffers() {
    let data = v6_definition_payload(ArchiveVersion::V8, [7; 16], &[], 3, true, true);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    let field_bytes = DEFINITION_TEXT_BYTES + "/full/source.3dm".len() + "source.3dm".len();
    // Outer four slots, linked four slots, and its old one-slot reallocation buffer.
    policy.limits.max_materialized_bytes = u64::try_from(field_bytes + 9 * RANGE_BYTES).unwrap();
    let (ctx, _) = DecodeContext::from_root_bytes(&data, &arena, &policy).unwrap();
    let mut field_storage = ctx.reserve_scoped(0, "instance test fields").unwrap();
    let mut warnings = Diagnostics::new();
    let definition = field_storage.with_storage(|| {
        crate::instances::parse_v6(
            &ctx, &data, 0..data.len(), 0..data.len(), ArchiveVersion::V8, &mut warnings,
        )
    }).unwrap();
    let LinkSource::Structured(reference) = &definition.link else {
        panic!("structured linked definition");
    };
    assert_eq!(reference.full_path, "/full/source.3dm");
    assert_eq!(reference.relative_path, "source.3dm");
    assert_eq!(definition.linked_depth, 2);
    assert_eq!(definition.linked_appearance, 2);
    assert!(warnings.is_empty());
    let (next, next_storage) = ctx.temporary_vec::<Range<usize>>(9, "next actual checksum buffer")
        .expect("both nested child buffers have ended while linked fields remain live");
    assert!(next.capacity() >= 9);
    drop(next);
    drop(next_storage);
    drop(definition);
    drop(warnings);
    drop(field_storage);
    ctx.finish_session().unwrap();
}

#[test]
fn reference_checksum_storage_releases_arrays_and_parent_buffer() {
    let archive = ArchiveVersion::V8;
    let child = long_chunk(archive, 0x4000_0000, &[]);
    let mut implementation_body = 1_i32.to_le_bytes().to_vec();
    implementation_body.extend(0_i32.to_le_bytes());
    implementation_body.extend(1_i32.to_le_bytes());
    let first_start = implementation_body.len();
    implementation_body.extend(&child);
    let first = first_start..implementation_body.len();
    implementation_body.extend(1_i32.to_le_bytes());
    let second_start = implementation_body.len();
    implementation_body.extend(&child);
    let second = second_start..implementation_body.len();
    implementation_body.push(1);
    let parent_start = implementation_body.len();
    implementation_body.extend(&child);
    let parent = parent_start..implementation_body.len();
    let implementation = crc_chunk_excluding(archive, 0x4000_8000, &implementation_body, &[first, second, parent]);
    let mut body = 1_i32.to_le_bytes().to_vec();
    body.extend(0_i32.to_le_bytes());
    body.push(1);
    let implementation_start = body.len();
    body.extend(implementation);
    let implementation_range = implementation_start..body.len();
    let data = crc_chunk_excluding(archive, 0x4000_8000, &body, &[implementation_range]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // The first amortized reservation holds all three actual child ranges.
    policy.limits.max_materialized_bytes = u64::try_from(4 * RANGE_BYTES).unwrap();
    let (ctx, _) = DecodeContext::from_root_bytes(&data, &arena, &policy).unwrap();
    let mut field_storage = ctx.reserve_scoped(0, "instance test fields").unwrap();
    let mut reader = BoundedReader::new(&data, 0, data.len()).unwrap();
    let mut warnings = Diagnostics::new();
    let range = field_storage.with_storage(|| {
        crate::instances::reference_settings(&ctx, &data, &mut reader, archive, &mut warnings)
    }).unwrap();
    assert_eq!(range, 0..data.len());
    assert_eq!(reader.position(), data.len());
    assert!(warnings.is_empty());
    let (next, next_storage) = ctx.temporary_vec::<Range<usize>>(4, "next actual checksum buffer")
        .expect("array and parent checksum buffer is no longer live");
    assert!(next.capacity() >= 4);
    drop(next);
    drop(next_storage);
    drop(warnings);
    drop(field_storage);
    ctx.finish_session().unwrap();
}
