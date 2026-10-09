// SPDX-License-Identifier: Apache-2.0
//! Decimal archive and logical span identity candidates.

use crate::container::{push_logical_span, scan};
use crate::native::{LogicalClassification, LogicalSpan};
use crate::test_support::{entry_record, refusal_at, with_service_context};
use cadmpeg_core::container::ContainerRole;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension, View};
use cadmpeg_core::CodecError;

#[test]
fn archive_span_decimal_ordinal_is_admitted_and_preserves_zero_based_identity() {
    let bytes = super::archive("<Document SchemaVersion=\"4\" FileVersion=\"1\"/>");
    refusal_at(ResourceDimension::WorkUnits, &bytes, "FCStd archive span ordinal", |ctx| {
        scan(ctx, View::over_retained(&bytes)).map(|_| ())
    });
    with_service_context(&bytes, |ctx| {
        let scanned = scan(ctx, View::over_retained(&bytes)).unwrap();
        assert!(!scanned.ledger.is_empty());
        for (ordinal, span) in scanned.ledger.iter().enumerate() {
            assert_eq!(span.id, format!("fcstd:native:archive-span#{ordinal}"));
        }
        assert_eq!(ctx.resource_refusal(), None);
    });
}

#[test]
fn logical_span_decimal_ordinal_refuses_before_identity_and_keeps_original_fuse() {
    let entry = entry_record("fcstd:native:entry#extra".into(), "extra".into(),
        ContainerRole::Auxiliary, Vec::new(), vec![0]);
    for dimension in [ResourceDimension::WorkUnits, ResourceDimension::MaterializedBytes] {
        refusal_at(dimension, &[], "FCStd logical span ordinal", |ctx| {
            let mut output = Vec::new();
            let error = push_logical_span(ctx, &mut output, &entry, 0, 1,
                LogicalClassification::Structural).unwrap_err();
            assert!(output.is_empty());
            let CodecError::ResourceLimit(original) = error else {
                panic!("ordinal admission must refuse")
            };
            assert_eq!(ctx.resource_refusal(), Some(original));
            assert!(matches!(push_logical_span(ctx, &mut output, &entry, 0, 1,
                LogicalClassification::Structural), Err(CodecError::ResourceLimit(actual))
                if actual == original));
            Err::<(), _>(CodecError::ResourceLimit(original))
        });
    }
}

#[test]
fn logical_span_decimal_ordinal_preserves_identity_order_and_empty_range() {
    let entry = entry_record("fcstd:native:entry#extra".into(), "extra".into(),
        ContainerRole::Auxiliary, Vec::new(), vec![0, 0]);
    with_service_context(&[], |ctx| {
        let mut output: Vec<LogicalSpan> = Vec::new();
        push_logical_span(ctx, &mut output, &entry, 0, 0,
            LogicalClassification::Structural).unwrap();
        assert!(output.is_empty());
        push_logical_span(ctx, &mut output, &entry, 0, 1,
            LogicalClassification::Structural).unwrap();
        push_logical_span(ctx, &mut output, &entry, 1, 2,
            LogicalClassification::Structural).unwrap();
        assert_eq!(output[0].id, "fcstd:native:logical-span#0");
        assert_eq!(output[1].id, "fcstd:native:logical-span#1");
        assert_eq!(output[0].entry, "extra");
        assert_eq!((output[0].span.start(), output[0].span.end()), (0, 1));
        assert_eq!((output[1].span.start(), output[1].span.end()), (1, 2));
        assert_eq!(ctx.resource_refusal(), None);
    });
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut output = Vec::new();
    push_logical_span(&ctx, &mut output, &entry, 0, 0,
        LogicalClassification::Structural).unwrap();
    assert!(output.is_empty());
    assert_eq!(ctx.resource_refusal(), None);
}
