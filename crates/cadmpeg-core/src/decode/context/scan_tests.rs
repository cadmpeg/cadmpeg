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
fn retained_concatenation_admits_exact_storage_and_both_source_passes() {
    let arena = DecodeArena::new();
    let inputs = [vec![1_u8, 2], vec![3]];
    let mut policy = DecodePolicy::service();
    // Two length visits, two copy visits and three copied bytes; three retained bytes.
    policy.limits.max_work_units = 7;
    policy.limits.max_retained_bytes = 3;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    assert_eq!(
        ctx.concat_retained(&inputs, "concat").expect("admitted"),
        [1, 2, 3]
    );
    for (work, retained) in [(6, 3), (7, 2)] {
        policy.limits.max_work_units = work;
        policy.limits.max_retained_bytes = retained;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let CodecError::ResourceLimit(first) =
            ctx.concat_retained(&inputs, "concat").expect_err("refusal")
        else {
            panic!("resource refusal")
        };
        let CodecError::ResourceLimit(second) = ctx.charge_work(1, "later").expect_err("fused")
        else {
            panic!("resource refusal")
        };
        assert_eq!(first, second);
    }
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

#[test]
fn scan_fill_copies_without_running_clone() {
    #[derive(Copy)]
    struct CopyValue<'a>(&'a std::cell::Cell<usize>);

    // Clone has an observable effect so the test detects clone-based fills.
    #[allow(
        clippy::non_canonical_clone_impl,
        clippy::expl_impl_clone_on_copy,
        reason = "Clone instrumentation detects a clone-based fill."
    )]
    impl Clone for CopyValue<'_> {
        fn clone(&self) -> Self {
            self.0.set(self.0.get() + 1);
            *self
        }
    }

    let clones = std::cell::Cell::new(0);
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("empty root is admitted");
    let values = ctx
        .alloc_filled(3, CopyValue(&clones), "fixed copies")
        .expect("admitted copies");
    assert_eq!(values.len(), 3);
    assert_eq!(clones.get(), 0);
}

#[test]
fn lossy_utf8_copy_admits_scans_prefix_validation_and_output_fragments() {
    let arena = DecodeArena::new();
    for work in [17, 18] {
        let mut policy = DecodePolicy::service();
        // Four loop visits, eight suffix-scan bytes, one prefix validation byte and five output bytes.
        policy.limits.max_work_units = work;
        policy.limits.max_retained_bytes = 5;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let result = ctx.copy_retained_lossy_utf8(&[b'a', 0xff, b'b'], "lossy");
        if work == 18 {
            assert_eq!(result.expect("admission"), "a�b");
        } else {
            let CodecError::ResourceLimit(first) = result.expect_err("refusal") else {
                panic!("refusal")
            };
            let CodecError::ResourceLimit(second) = ctx.charge_work(1, "later").expect_err("fused")
            else {
                panic!("refusal")
            };
            assert_eq!(first, second);
        }
    }
}
