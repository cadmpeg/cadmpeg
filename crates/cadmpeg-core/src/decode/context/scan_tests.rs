// SPDX-License-Identifier: Apache-2.0

use crate::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use crate::CodecError;

#[test]
fn scan_text_and_concat_passes_refuse_on_work() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test operation is admitted");
    assert!(
        matches!(ctx.copy_retained_lossy_utf8(b"a", "UTF-8"), Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::WorkUnits)
    );
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test operation is admitted");
    assert!(
        matches!(ctx.concat_retained(&[Vec::new()], "concat"), Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::WorkUnits)
    );
}

#[test]
fn scan_concat_views_admits_empty_view_iteration() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    let (ctx, root) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    assert!(matches!(ctx.concat_views(&[root]),
        Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::WorkUnits
            && limit.operation == "concat_views"));
}

#[test]
fn scan_fill_admits_work_and_retained_storage_before_initialization() {
    for dimension in [
        ResourceDimension::WorkUnits,
        ResourceDimension::RetainedBytes,
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::WorkUnits => policy.limits.max_work_units = 1,
            ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = 1,
            _ => panic!("test dimension"),
        }
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
        let error = ctx
            .alloc_filled(2, 7_u8, "filled storage")
            .expect_err("two slots exceed one unit");
        let CodecError::ResourceLimit(limit) = error else {
            panic!("typed resource refusal");
        };
        assert_eq!(limit.dimension, dimension);
        assert_eq!(limit.used, 0);
        assert_eq!(limit.additional, 2);
        assert_eq!(ctx.resource_refusal(), Some(limit));
    }
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("empty root is admitted");
    assert_eq!(
        ctx.alloc_filled(2, 7_u8, "filled storage")
            .expect("admitted fill"),
        [7, 7]
    );
}
