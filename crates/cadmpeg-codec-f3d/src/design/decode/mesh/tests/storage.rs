// SPDX-License-Identifier: Apache-2.0

use super::{mesh_texture_table_record, parse_texture_table, synthetic_mesh_graph, texture_table_frame};
use super::super::MeshTextureEntry;
use cadmpeg_core::decode::{u64_from_index, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn mesh_texture_table_return_transfers_its_live_vector_storage() {
    const RESOURCE: &str = "10000000-0000-4000-8000-000000000001";
    let graph = synthetic_mesh_graph(false);
    let bytes = mesh_texture_table_record(261, 101, &[(RESOURCE, 2)], &[(RESOURCE, 113)]);
    let frame = texture_table_frame(&graph, &bytes);
    let entry_size = std::mem::size_of::<MeshTextureEntry>();
    assert!((2..=1024).contains(&entry_size));
    // The first amortized push allocates four slots. Class tag and GUID
    // text retain their own storage before the vector's final transfer.
    let vector_bytes = u64_from_index(4 * entry_size);
    let text_bytes = u64_from_index("261".len() + RESOURCE.len());
    let required = text_bytes + vector_bytes;
    for cap in [required, required - 1] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = cap;
        crate::test_support::with_decode_policy(&policy, |ctx| {
            let result = parse_texture_table(ctx, &bytes, frame);
            if cap == required {
                let table = result.expect("all live returned storage fits");
                assert_eq!(table.textures.capacity(), 4);
                let [entry] = table.textures.as_slice() else {
                    panic!("one texture entry");
                };
                assert_eq!(entry.resource_guid.as_str(), RESOURCE);
                assert_eq!(entry.ordinal, 0);
                assert_eq!(entry.flags, 2);
                let filename = entry.filename.expect("joined filename reference");
                assert_eq!(filename.ordinal, 0);
                assert_eq!(filename.record_index, 113);
                assert_eq!(ctx.resource_refusal(), None);
            } else {
                let CodecError::ResourceLimit(limit) = result.unwrap_err() else {
                    panic!("the final live vector transfer must refuse");
                };
                assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
                assert_eq!(limit.operation, "test texture storage");
                assert_eq!(limit.limit, cap);
                assert_eq!(limit.used, text_bytes);
                assert_eq!(limit.additional, vector_bytes);
                assert_eq!(ctx.resource_refusal(), Some(limit.clone()));
                let CodecError::ResourceLimit(later) = ctx
                    .charge_work(0, "after texture table transfer refusal")
                    .unwrap_err()
                else {
                    panic!("the original transfer refusal remains fused");
                };
                assert_eq!(later, limit);
            }
        });
    }
}
