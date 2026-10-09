// SPDX-License-Identifier: Apache-2.0

use super::{container, decode_mesh_container_charged, CodecError, GUID, TRIANGLE_CORNERS, TRIANGLE_VERTICES};
use cadmpeg_core::decode::{DecodePolicy, View};

#[test]
fn paramesh_chunk_errors_release_the_live_name_table_storage() {
    let bytes = container(GUID, &TRIANGLE_VERTICES, &TRIANGLE_CORNERS);
    let count = usize::try_from(View::u64_le_at(&bytes, super::super::PROTOBUF_COUNT_AT).unwrap()).unwrap();
    let table_start = super::super::PROTOBUF_AT + count;
    let table_count = usize::try_from(View::u64_le_at(&bytes, table_start).unwrap()).unwrap();
    let table_end = table_start + 12 + table_count;
    let mut repeated = bytes[..table_end].to_vec();
    repeated.extend_from_slice(&bytes[table_start..table_end]);
    let mut undefined = bytes[..table_end].to_vec();
    undefined.extend_from_slice(&0u64.to_le_bytes());
    undefined.extend_from_slice(&5u32.to_le_bytes());
    let mut truncated = bytes[..table_end].to_vec();
    truncated.extend_from_slice(&1u64.to_le_bytes());
    truncated.extend_from_slice(&super::CHUNK_STREAM.to_le_bytes());
    for (bytes, expected) in [
        (repeated, "paramesh container repeats its name table"),
        (undefined, "paramesh container holds an undefined chunk kind"),
        (truncated, "paramesh chunk body is truncated"),
    ] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = 16 * 1024;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_decompressed_bytes_total = 0;
        crate::test_support::with_decode_policy(&policy, |ctx| {
            let result = ctx.with_scoped_storage("test malformed paramesh candidate", || {
                decode_mesh_container_charged(ctx, &bytes)
            });
            assert!(matches!(result, Err(CodecError::Malformed(message)) if message == expected));
            assert_eq!(ctx.resource_refusal(), None);
            // Every temporary registry/name-table/index backing is gone.
            let storage = ctx.reserve_scoped(policy.limits.max_materialized_bytes,
                "reuse released paramesh temporary backing").unwrap();
            drop(storage);
            assert_eq!(ctx.resource_refusal(), None);
        });
    }
}
