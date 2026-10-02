// SPDX-License-Identifier: MIT OR Apache-2.0
use crate::curves::GeometryError;
use crate::surfaces::reconstruct_knots;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

const STORED: [f64; 7] = [0., 1., 2., 3., 4., 5., 6.];

#[test]
fn raw_knot_reconstruction_admits_caller_storage_and_copy_work() {
    for dimension in [ResourceDimension::RetainedBytes, ResourceDimension::CollectionItems, ResourceDimension::WorkUnits] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = 71,
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = 8,
            ResourceDimension::WorkUnits => policy.limits.max_work_units = 8,
            _ => panic!("test dimension"),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let Err(GeometryError::Codec(CodecError::ResourceLimit(limit))) = reconstruct_knots(&ctx, &STORED, 3, 6) else {
            panic!("reconstruction must preserve the caller refusal");
        };
        assert_eq!(limit.dimension, dimension);
        assert_eq!(limit.operation, "Rhino NURBS reconstructed knots");
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit));
    }
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 72;
    policy.limits.max_collection_items = 9;
    policy.limits.max_work_units = 9;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    assert_eq!(reconstruct_knots(&ctx, &STORED, 3, 6).expect("exact limits"), [-1., 0., 1., 2., 3., 4., 5., 6., 7.]);
    ctx.finish_session().expect("each operation paid once");
}

#[test]
fn raw_knot_reconstruction_preserves_work_across_successive_calls() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 17;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    assert_eq!(reconstruct_knots(&ctx, &STORED, 3, 6).expect("first call"), [-1., 0., 1., 2., 3., 4., 5., 6., 7.]);
    let Err(GeometryError::Codec(CodecError::ResourceLimit(limit))) = reconstruct_knots(&ctx, &STORED, 3, 6) else {
        panic!("second call must use the same work account");
    };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!(limit.used, 9);
    assert_eq!(limit.additional, 9);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit));
}
