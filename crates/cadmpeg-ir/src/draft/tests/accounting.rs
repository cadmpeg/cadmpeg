// SPDX-License-Identifier: Apache-2.0

use crate::annotations::Annotations;
use crate::document::CadIr;
use crate::draft::tests::point;
use crate::draft::tests::point_draft;
use crate::draft::DraftError;
use crate::provenance::Exactness;

#[test]
fn adding_accounting_preserves_entities_and_commits_exactness() {
    let identity = "test:model:point#accounted";
    let mut draft = point_draft(identity).with_accounting();
    assert_eq!(draft.model().points, vec![point(identity)]);
    assert_eq!(
        draft.insert(
            point(identity),
            &cadmpeg_test_support::service_decode_context()
        ),
        Err(DraftError::IdentityCollision(identity.into()))
    );
    draft
        .exactness(
            &cadmpeg_test_support::service_decode_context(),
            identity,
            Exactness::Derived,
        )
        .unwrap();
    let mut base = CadIr::empty();
    let mut annotations = Annotations::default();
    draft
        .commit(
            &mut base,
            &mut annotations,
            &cadmpeg_test_support::service_decode_context(),
        )
        .unwrap()
        .unwrap();

    assert_eq!(base.model.points, vec![point(identity)]);
    assert_eq!(
        annotations.exactness()[identity].entity(),
        Exactness::Derived
    );
}

#[test]
fn refused_accounted_draft_leaves_all_existing_destinations_unchanged() {
    let existing = "test:model:point#existing";
    let mut base = CadIr::empty();
    base.model.points.push(point(existing));
    let mut builder = crate::annotations::AnnotationBuilder::new();
    builder
        .exactness(
            &cadmpeg_test_support::service_decode_context(),
            existing,
            Exactness::Inferred,
        )
        .unwrap();
    let mut annotations = builder.build();
    let before = (base.clone(), annotations.clone());

    let mut draft = point_draft(existing).with_accounting();
    draft
        .exactness(
            &cadmpeg_test_support::service_decode_context(),
            existing,
            Exactness::Derived,
        )
        .unwrap();
    assert_eq!(
        draft
            .commit(
                &mut base,
                &mut annotations,
                &cadmpeg_test_support::service_decode_context()
            )
            .unwrap(),
        Err(DraftError::IdentityCollision(existing.into()))
    );
    assert_eq!((base, annotations), before);
}

#[test]
fn rejected_accounted_draft_does_not_retain_the_annotation_copy() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let identity = "test:model:point#existing";
    let mut base = CadIr::empty();
    base.model.points.push(point(identity));
    let mut builder = crate::annotations::AnnotationBuilder::new();
    builder
        .exactness(
            &cadmpeg_test_support::service_decode_context(),
            identity,
            Exactness::Inferred,
        )
        .unwrap();
    let mut annotations = builder.build();
    let before = (base.clone(), annotations.clone());
    let mut draft = point_draft(identity).with_accounting();
    draft
        .exactness(
            &cadmpeg_test_support::service_decode_context(),
            identity,
            Exactness::Derived,
        )
        .unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = u64::try_from(identity.len()).unwrap();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert_eq!(
        draft.commit(&mut base, &mut annotations, &ctx).unwrap(),
        Err(DraftError::IdentityCollision(identity.into()))
    );
    assert_eq!((base, annotations), before);
    ctx.finish_session().unwrap();
}

#[test]
fn draft_exactness_retention_keeps_selected_entries_and_a_failing_predicate_changes_nothing() {
    use cadmpeg_core::CodecError;
    let first = "test:model:point#first";
    let second = "test:model:point#second";
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut draft = point_draft(first).with_accounting();
    draft.exactness(&ctx, first, Exactness::Derived).unwrap();
    draft.exactness(&ctx, second, Exactness::Inferred).unwrap();
    let before = draft.accounting.exactness.clone();
    let mut calls = 0;
    let error = draft
        .retain_exactness(&ctx, |id| {
            calls += 1;
            if id == second {
                Err(CodecError::malformed("predicate refusal"))
            } else {
                Ok(false)
            }
        })
        .unwrap_err();
    assert!(matches!(error, CodecError::Malformed(_)));
    assert_eq!(calls, 2);
    assert_eq!(draft.accounting.exactness, before);
    draft.retain_exactness(&ctx, |id| Ok(id == second)).unwrap();
    assert_eq!(
        draft.accounting.exactness,
        std::collections::BTreeMap::from([(second.into(), Exactness::Inferred)])
    );
}
