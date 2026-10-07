// SPDX-License-Identifier: MIT OR Apache-2.0
use crate::curves::GeometryError;
use crate::surfaces::reconstruct_knots;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

const STORED: [f64; 7] = [0., 1., 2., 3., 4., 5., 6.];

#[test]
fn raw_knot_reconstruction_admits_caller_storage_and_copy_work() {
    for dimension in [
        ResourceDimension::RetainedBytes,
        ResourceDimension::CollectionItems,
        ResourceDimension::WorkUnits,
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = 71,
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = 8,
            ResourceDimension::WorkUnits => policy.limits.max_work_units = 8,
            _ => panic!("test dimension"),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let Err(GeometryError::Codec(CodecError::ResourceLimit(limit))) =
            reconstruct_knots(&ctx, &STORED, 3, 6)
        else {
            panic!("reconstruction must preserve the caller refusal");
        };
        assert_eq!(limit.dimension, dimension);
        assert_eq!(limit.operation, "Rhino NURBS reconstructed knots");
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
        );
    }
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 72;
    policy.limits.max_collection_items = 9;
    policy.limits.max_work_units = 9;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    assert_eq!(
        reconstruct_knots(&ctx, &STORED, 3, 6).expect("exact limits"),
        [-1., 0., 1., 2., 3., 4., 5., 6., 7.]
    );
    ctx.finish_session().expect("each operation paid once");
}

#[test]
fn raw_knot_reconstruction_preserves_work_across_successive_calls() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 17;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    assert_eq!(
        reconstruct_knots(&ctx, &STORED, 3, 6).expect("first call"),
        [-1., 0., 1., 2., 3., 4., 5., 6., 7.]
    );
    let Err(GeometryError::Codec(CodecError::ResourceLimit(limit))) =
        reconstruct_knots(&ctx, &STORED, 3, 6)
    else {
        panic!("second call must use the same work account");
    };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!(limit.used, 9);
    assert_eq!(limit.additional, 9);
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
    );
}

#[test]
fn periodic_knot_scans_preserve_first_and_later_caller_refusals() {
    for checked in [false, true] {
        // Seven scale visits plus two comparisons. Raw knots also need one
        // end probe for the combined finite-value and scale search.
        let visits = if checked { 9 } else { 10 };
        for cap in 0..visits {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let result = if checked {
                let knots =
                    STORED.map(|value| cadmpeg_ir::scalar::FiniteReal::new(value).expect("finite"));
                crate::surfaces::periodic_knots_checked(&ctx, &knots, 3, 6)
            } else {
                crate::surfaces::periodic_knots(&ctx, &STORED, 3, 6)
            };
            let Err(CodecError::ResourceLimit(limit)) = result else {
                panic!("every visited knot and interval needs admission");
            };
            assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
            // The checked lane admits all seven source items before reading.
            assert_eq!(limit.used, if checked && cap < 7 { 0 } else { cap });
            assert_eq!(limit.additional, if checked && cap < 7 { 7 } else { 1 });
            let operation = if cap < if checked { 7 } else { 8 } {
                "Rhino periodic knot scale"
            } else {
                "Rhino periodic knot comparison"
            };
            assert_eq!(limit.operation, operation);
            assert!(
                matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
            );
        }
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = visits;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let result = if checked {
            let knots =
                STORED.map(|value| cadmpeg_ir::scalar::FiniteReal::new(value).expect("finite"));
            crate::surfaces::periodic_knots_checked(&ctx, &knots, 3, 6)
        } else {
            crate::surfaces::periodic_knots(&ctx, &STORED, 3, 6)
        };
        assert!(result.expect("exact visits"));
        ctx.finish_session().expect("no allocation or extra pass");
    }
}

#[test]
fn periodic_knot_scan_admits_before_reading_and_keeps_early_exit_order() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 3;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let reads = std::cell::Cell::new(0);
    let result = crate::surfaces::periodic_knots_by(
        &ctx,
        &STORED,
        3,
        6,
        |value| {
            reads.set(reads.get() + 1);
            *value
        },
        true,
    );
    assert_eq!(reads.get(), 3);
    assert!(matches!(result, Err(CodecError::ResourceLimit(_))));

    let arena = DecodeArena::new();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    assert!(!crate::surfaces::periodic_knots(&ctx, &STORED, 2, 6).expect("fixed header rejection"));
    ctx.finish_session().expect("no scan");

    let arena = DecodeArena::new();
    policy.limits.max_work_units = 15;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    assert!(
        !crate::surfaces::periodic_knots(&ctx, &[0., 0., 0., 1., 2., 3., 3.], 3, 6)
            .expect("first unequal interval")
    );
    ctx.finish_session()
        .expect("early mismatch stops the paired scan");
}
