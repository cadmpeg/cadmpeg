// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn reusable_basis_constructor_preserves_initialization_refusal_and_backing_lifetime() {
    let curve = super::curve();
    let support = usize::try_from(curve.degree()).unwrap() + 1;
    let bytes = u64::try_from(support * std::mem::size_of::<f64>()).unwrap();
    for work in 0..=u64::try_from(support).unwrap() {
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = bytes;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_work_units = work;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = super::super::NurbsPointEvaluator::new(&ctx, &curve);
        if work < u64::try_from(support).unwrap() {
            let limit = result.err().expect("basis initialization needs each slot");
            assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
            assert_eq!(limit.operation, "IR B-spline basis work");
            assert_eq!(limit.used, 0);
            assert_eq!(limit.additional, u64::try_from(support).unwrap());
            assert_eq!(ctx.resource_refusal(), Some(limit));
            assert_eq!(
                super::super::NurbsPointEvaluator::new(&ctx, &curve).err(),
                Some(limit)
            );
            assert!(
                matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit)
            );
        } else {
            let evaluator = result.expect("exact initialization work");
            assert_eq!(&*evaluator.basis, &[0.0; 3]);
            let super::super::NurbsPointEvaluator {
                _storage: storage, ..
            } = &evaluator;
            assert!(storage.is_some());
            drop(evaluator);
            let reuse = ctx
                .reserve_scoped_limit(bytes, "basis backing released")
                .unwrap();
            drop(reuse);
            ctx.finish_session().unwrap();
        }
    }
}
