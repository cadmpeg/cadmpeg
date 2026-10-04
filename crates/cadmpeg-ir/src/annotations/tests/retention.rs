// SPDX-License-Identifier: Apache-2.0

use super::{AnnotationBuilder, Exactness, StreamHandle};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

const FIRST: &str = "test:model:point#first";
const SECOND: &str = "test:model:point#second";

fn fixture() -> AnnotationBuilder {
    let mut builder = AnnotationBuilder::new();
    let stream = StreamHandle::new(
        &cadmpeg_test_support::service_decode_context(),
        crate::stream_name!("source"),
        "fixture stream handle",
    )
    .unwrap();
    builder
        .note(
            &cadmpeg_test_support::service_decode_context(),
            FIRST,
            &stream,
            7,
            Some("first"),
        )
        .unwrap();
    builder
        .note(
            &cadmpeg_test_support::service_decode_context(),
            SECOND,
            &stream,
            9,
            Some("second"),
        )
        .unwrap();
    builder
        .exactness(
            &cadmpeg_test_support::service_decode_context(),
            FIRST,
            Exactness::Derived,
        )
        .unwrap();
    builder
        .exactness(
            &cadmpeg_test_support::service_decode_context(),
            SECOND,
            Exactness::Inferred,
        )
        .unwrap();
    builder
}

#[test]
fn annotation_removal_refuses_work_before_changing_either_table() {
    let mut builder = fixture();
    let before = builder.annotations().clone();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let Err(CodecError::ResourceLimit(limit)) = builder.remove_entity(&ctx, FIRST) else {
        panic!("identity removal must refuse work");
    };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!(limit.operation, "remove source provenance");
    assert_eq!(builder.annotations(), &before);
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit)
    );
}

#[test]
fn annotation_removal_uses_no_owned_or_temporary_storage() {
    let mut builder = fixture();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    builder.remove_entity(&ctx, FIRST).unwrap();
    assert!(!builder.annotations().provenance.contains_key(FIRST));
    assert!(!builder.annotations().exactness().contains_key(FIRST));
    assert_eq!(builder.annotations().provenance[SECOND].offset, 9);
    assert_eq!(
        builder.annotations().provenance[SECOND].tag.as_deref(),
        Some("second")
    );
    assert_eq!(
        builder.annotations().exactness()[SECOND].entity(),
        Exactness::Inferred
    );
    builder.remove_entity(&ctx, "absent").unwrap();
    ctx.finish_session().unwrap();
}

#[test]
fn annotation_retention_refuses_decision_storage_before_callbacks() {
    for dimension in [
        ResourceDimension::CollectionItems,
        ResourceDimension::MaterializedBytes,
        ResourceDimension::WorkUnits,
    ] {
        for provenance in [false, true] {
            let mut builder = fixture();
            let before = builder.annotations().clone();
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            match dimension {
                ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
                ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = 0,
                ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
                _ => unreachable!(),
            }
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut calls = 0;
            let keep = |_id: &str| {
                calls += 1;
                Ok(false)
            };
            let result = if provenance {
                builder.state.annotations.retain_provenance(&ctx, keep)
            } else {
                builder.retain_exactness(&ctx, keep).map(|_| ())
            };
            let Err(CodecError::ResourceLimit(limit)) = result else {
                panic!("decision admission must retain the caller refusal");
            };
            assert_eq!(limit.dimension, dimension);
            assert_eq!(calls, 0);
            assert_eq!(builder.annotations(), &before);
            assert!(
                matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit)
            );
        }
    }
}

#[test]
fn annotation_retention_keeps_callback_allocations_retained() {
    for provenance in [false, true] {
        let mut builder = fixture();
        let before = builder.annotations().clone();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let keep = |_id: &str| {
            let _output =
                ctx.copy_retained_text("retained callback output", "annotation callback output")?;
            Ok(false)
        };
        let result = if provenance {
            builder.state.annotations.retain_provenance(&ctx, keep)
        } else {
            builder.retain_exactness(&ctx, keep).map(|_| ())
        };
        let Err(CodecError::ResourceLimit(limit)) = result else {
            panic!("callback output must retain its storage dimension");
        };
        assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
        assert_eq!(limit.operation, "annotation callback output");
        assert_eq!(builder.annotations(), &before);
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit)
        );
    }
}

#[test]
fn annotation_retention_keeps_tables_on_late_predicate_refusal_and_releases_scratch() {
    for provenance in [false, true] {
        let mut builder = fixture();
        let before = builder.annotations().clone();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 2;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut calls = 0;
        let keep = |id: &str| {
            calls += 1;
            if id == SECOND {
                ctx.charge_work(1, "annotation predicate refusal")?;
            }
            Ok(false)
        };
        let result = if provenance {
            builder.state.annotations.retain_provenance(&ctx, keep)
        } else {
            builder.retain_exactness(&ctx, keep).map(|_| ())
        };
        let Err(CodecError::ResourceLimit(limit)) = result else {
            panic!("late predicate admission must retain the caller refusal");
        };
        assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
        assert_eq!(limit.operation, "annotation predicate refusal");
        assert_eq!(calls, 2);
        assert_eq!(builder.annotations(), &before);
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit)
        );

        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = 2;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        if provenance {
            builder
                .state
                .annotations
                .retain_provenance(&ctx, |id| Ok(id == SECOND))
                .unwrap();
            assert_eq!(builder.annotations().provenance.len(), 1);
            assert_eq!(
                builder.annotations().provenance[SECOND],
                before.provenance[SECOND]
            );
            assert_eq!(builder.annotations().exactness(), before.exactness());
        } else {
            builder
                .retain_exactness(&ctx, |id| Ok(id == SECOND))
                .unwrap();
            assert_eq!(builder.annotations().exactness().len(), 1);
            assert_eq!(
                builder.annotations().exactness()[SECOND],
                before.exactness()[SECOND]
            );
            assert_eq!(builder.annotations().provenance, before.provenance);
        }
        let storage = ctx
            .reserve_scoped(2, "annotation decisions released")
            .unwrap();
        drop(storage);
        ctx.finish_session().unwrap();
    }
}
