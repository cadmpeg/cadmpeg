// SPDX-License-Identifier: Apache-2.0
//! Discarded wrapper diagnostics remain temporary.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use crate::chunks::{ArchiveVersion, FramingError};
use crate::loss::RhinoDiagnostic;
use crate::presentation::class_descriptor;
use crate::test_support::test_dump::{crc_chunk, long_chunk, short_chunk, MESH_CLASS};
use crate::wire::Uuid;

fn wrapper_with_checksum_warning() -> (Vec<u8>, u64) {
    let archive = ArchiveVersion::V8;
    let actual = crc32fast::hash(&MESH_CLASS);
    let expected = actual.wrapping_add(1);
    let mut uuid_body = MESH_CLASS.to_vec();
    uuid_body.extend(expected.to_le_bytes());
    let uuid = long_chunk(archive, 0x0002_fffb, &uuid_body);
    let data = crc_chunk(archive, 0x0002_fffc, &[]);
    let end = short_chunk(archive, 0x8002_7fff, 0);
    let header_bytes = long_chunk(archive, 0x0002_7ffa, &[]).len();
    let diagnostic = format!(
        "CRC mismatch at offset {header_bytes} for typecode 0x2fffb: expected {expected:#x}, got {actual:#x}"
    );
    let bytes = long_chunk(archive, 0x0002_7ffa, &[uuid, data, end].concat());
    (bytes, u64::try_from(diagnostic.len()).unwrap())
}

fn diagnostic_capacity_bytes() -> u64 {
    // One diagnostic uses the core four-slot initial Vec growth bound.
    let item_bytes = std::mem::size_of::<RhinoDiagnostic>();
    assert!((2..=1024).contains(&item_bytes));
    u64::try_from(4 * item_bytes).unwrap()
}

#[test]
fn discarded_class_warnings_release_exact_scratch_with_descriptors_live() {
    let (bytes, message_bytes) = wrapper_with_checksum_warning();
    let peak = diagnostic_capacity_bytes() + message_bytes;
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = peak;
    policy.limits.max_collection_items = 2;
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let first = class_descriptor(&ctx, &bytes, 0..bytes.len(), ArchiveVersion::V8).unwrap();
    let second = class_descriptor(&ctx, &bytes, 0..bytes.len(), ArchiveVersion::V8).unwrap();
    assert_eq!(first.class_uuid, Uuid::from_wire(MESH_CLASS));
    assert_eq!(second.class_uuid, first.class_uuid);
    assert_eq!(first.class_data_range, second.class_data_range);
    assert!(first.class_data_range.is_empty());
    let released = ctx.reserve_scoped(peak, "class diagnostic scratch reuse")
        .expect("returned descriptors retain no discarded warning backing");
    drop(released);
    assert_eq!(ctx.resource_refusal(), None);
    ctx.finish_session().unwrap();
}

fn assert_warning_refusal(cap: u64, operation: &'static str, used: u64, additional: u64) {
    let (bytes, _) = wrapper_with_checksum_warning();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = cap;
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let FramingError::Resource(original) = class_descriptor(
        &ctx, &bytes, 0..bytes.len(), ArchiveVersion::V8,
    ).unwrap_err() else { panic!("resource refusal"); };
    assert_eq!(original.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(original.operation, operation);
    assert_eq!(original.used, used);
    assert_eq!(original.additional, additional);
    assert_eq!(ctx.resource_refusal(), Some(original));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == original));
}

#[test]
fn discarded_class_warning_refuses_actual_vector_backing() {
    assert_warning_refusal(0, "Rhino diagnostics", 0, diagnostic_capacity_bytes());
}

#[test]
fn discarded_class_warning_refuses_actual_message_bytes() {
    let (_, message_bytes) = wrapper_with_checksum_warning();
    let vector_bytes = diagnostic_capacity_bytes();
    assert_warning_refusal(vector_bytes + message_bytes - 1,
        "Rhino diagnostic message", vector_bytes, message_bytes);
}

#[test]
fn class_descriptor_scratch_preserves_the_original_refusal() {
    let (bytes, _) = wrapper_with_checksum_warning();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let CodecError::ResourceLimit(original) = ctx.charge_work(1, "class original refusal").unwrap_err()
        else { panic!("original work refusal"); };
    assert!(matches!(class_descriptor(&ctx, &bytes, 0..bytes.len(), ArchiveVersion::V8),
        Err(FramingError::Resource(sticky)) if sticky == original));
    assert_eq!(ctx.resource_refusal(), Some(original));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == original));
}
