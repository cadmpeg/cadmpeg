// SPDX-License-Identifier: Apache-2.0
use super::*;
use cadmpeg_core::decode::{DecodeArena, DecodePolicy, LimitScope, ResourceDimension};

#[test]
fn oversized_slot_storage_refuses_only_the_actual_part_and_keeps_session_live() {
    let bytes = u32::MAX.to_le_bytes();
    let arena = DecodeArena::new();
    let (ctx, view) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let outcome = ctx
        .part_transaction(view, || {
            let mut source = view;
            let count = usize::try_from(source.req_u32_le()?).map_err(CodecError::malformed)?;
            alloc_slots::<Option<u32>>(&ctx, count, None, "test canonical color slots")
        })
        .unwrap();
    let error = outcome.result().unwrap_err();
    assert!(
        matches!(error,CodecError::ResourceLimit(limit) if limit.scope==LimitScope::Part && limit.dimension==ResourceDimension::RetainedBytes)
    );
    let small = ctx
        .part_transaction(view, || {
            alloc_slots(&ctx, 3, None::<u32>, "test small canonical slots")
        })
        .unwrap();
    assert_eq!(small.result().unwrap(), &[None, None, None]);
    ctx.finish_session().unwrap();
}
