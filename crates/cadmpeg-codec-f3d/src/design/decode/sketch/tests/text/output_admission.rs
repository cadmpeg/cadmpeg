// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn sketch_text_output_admission_refuses_insertion_before_retaining_actual_native_candidate() {
    let (bytes, meta, _, _, _, _) = super::indexed_sketch_fixture();
    // Select the actual final output insertion boundary using the existing
    // owning refusal helper. No observed numeric ceiling becomes an oracle.
    let CodecError::ResourceLimit(expected) = crate::test_support::resource_refusal_at(
        ResourceDimension::CollectionItems,
        "f3d sketch text records",
        0,
        |ctx| {
            super::super::super::decode_sketch_texts_from_stream(
                ctx,
                &bytes,
                &meta,
                "Design/BulkStream.dat",
            )
            .map(|(records, storage)| {
                drop(records);
                drop(storage);
            })
        },
    ) else {
        panic!("actual sketch output insertion has a collection boundary");
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = expected.limit;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let Err(CodecError::ResourceLimit(original)) =
        super::super::super::decode_sketch_texts_from_stream(
            &ctx,
            &bytes,
            &meta,
            "Design/BulkStream.dat",
        )
    else {
        panic!("output insertion refuses before candidate publication");
    };
    assert_eq!(original, expected);
    assert_eq!(original.dimension, ResourceDimension::CollectionItems);
    assert_eq!(original.operation, "f3d sketch text records");
    assert_eq!(ctx.resource_refusal(), Some(original));
    assert!(
        matches!(ctx.charge_work(0, "original sketch insertion refusal"),
        Err(CodecError::ResourceLimit(later)) if later == original)
    );
}
