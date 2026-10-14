// SPDX-License-Identifier: Apache-2.0

use super::{AnnotationBuilder, Exactness, StreamHandle};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
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
fn annotation_removal_uses_no_owned_or_temporary_storage() {
    let mut builder = fixture();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
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
fn retention_keeps_selected_entries_and_a_failing_predicate_changes_nothing() {
    let ctx = cadmpeg_test_support::service_decode_context();
    for provenance in [false, true] {
        let mut builder = fixture();
        let before = builder.annotations().clone();
        let mut calls = 0;
        let keep = |id: &str| {
            calls += 1;
            if id == SECOND {
                Err(CodecError::malformed("predicate refusal"))
            } else {
                Ok(false)
            }
        };
        let result = if provenance {
            builder.state.annotations.retain_provenance(&ctx, keep)
        } else {
            builder.retain_exactness(&ctx, keep).map(|_| ())
        };
        assert!(matches!(result, Err(CodecError::Malformed(_))));
        assert_eq!(calls, 2);
        assert_eq!(builder.annotations(), &before);
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
    }
}
