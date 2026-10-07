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
fn accounted_draft_retained_transfer_refusal_leaves_model_and_annotations_unchanged() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let identity = "test:model:point#new";
    cadmpeg_test_support::refusal::resource_limit_at(ResourceDimension::RetainedBytes,
        "draft annotation transaction", |cap| {
            let mut draft = point_draft(identity).with_accounting();
            draft.exactness(&cadmpeg_test_support::service_decode_context(), identity, Exactness::Derived).unwrap();
            let mut base = CadIr::empty();
            let mut annotations = Annotations::default();
            let before = (base.clone(), annotations.clone());
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let result = draft.commit(&mut base, &mut annotations, &ctx);
            if let Err(CodecError::ResourceLimit(limit)) = &result {
                assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
                assert_eq!(limit.operation, "draft annotation transaction");
                assert_eq!((base, annotations), before);
                assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == *limit));
            }
            result
        });
}

#[test]
fn draft_exactness_retention_refuses_storage_before_predicates() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let identity = "test:model:point#exactness";
    for dimension in [
        ResourceDimension::CollectionItems,
        ResourceDimension::MaterializedBytes,
    ] {
        let mut draft = point_draft(identity).with_accounting();
        draft
            .exactness(
                &cadmpeg_test_support::service_decode_context(),
                identity,
                Exactness::Derived,
            )
            .unwrap();
        let before = draft.accounting.exactness.clone();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
            ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = 0,
            _ => unreachable!(),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut calls = 0;
        let Err(CodecError::ResourceLimit(limit)) = draft.retain_exactness(&ctx, |_| {
            calls += 1;
            Ok(false)
        }) else {
            panic!("decision storage must refuse");
        };
        assert_eq!(limit.dimension, dimension);
        assert_eq!(calls, 0);
        assert_eq!(draft.accounting.exactness, before);
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit)
        );
    }
}

#[test]
fn draft_exactness_retention_preserves_earlier_entries_on_predicate_refusal() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let first = "test:model:point#first";
    let second = "test:model:point#second";
    let mut draft = point_draft(first).with_accounting();
    draft
        .exactness(
            &cadmpeg_test_support::service_decode_context(),
            first,
            Exactness::Derived,
        )
        .unwrap();
    draft
        .exactness(
            &cadmpeg_test_support::service_decode_context(),
            second,
            Exactness::Inferred,
        )
        .unwrap();
    let before = draft.accounting.exactness.clone();
    cadmpeg_test_support::refusal::resource_limit_at(ResourceDimension::WorkUnits,
        "exactness predicate refusal", |cap| {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = cap;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut calls = 0;
    let mut candidate = point_draft(first).with_accounting();
    candidate.accounting.exactness = before.clone();
    let result = candidate.retain_exactness(&ctx, |id| {
        calls += 1;
        if id == second {
            ctx.charge_work(1, "exactness predicate refusal")?;
        }
        Ok(false)
    });
    if let Err(CodecError::ResourceLimit(limit)) = &result {
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!(limit.operation, "exactness predicate refusal");
    assert_eq!(calls, 2);
    assert_eq!(candidate.accounting.exactness, before);
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == *limit)
    );

    }
    result
    });

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 2;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    draft.retain_exactness(&ctx, |id| Ok(id == second)).unwrap();
    assert_eq!(
        draft.accounting.exactness,
        std::collections::BTreeMap::from([(second.into(), Exactness::Inferred)])
    );
    let storage = ctx
        .reserve_scoped(2, "exactness decisions released")
        .unwrap();
    drop(storage);
    ctx.finish_session().unwrap();
}

#[test]
fn session_identity_transfer_refusal_preserves_model_and_annotations() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let identity = "test:model:point#transfer";
    cadmpeg_test_support::refusal::resource_limit_at(ResourceDimension::WorkUnits, "committed identity transfer moves", |cap| {
        let mut draft = point_draft(identity).with_accounting();
        draft.exactness(&cadmpeg_test_support::service_decode_context(), identity, Exactness::Derived).unwrap();
        let mut annotations = Annotations::default();
        let before = (CadIr::empty(), annotations.clone());
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut session = crate::draft::CommitSession::new(CadIr::empty(), &ctx, None)?;
        let result = session.commit(draft, &mut annotations);
        if let Err(CodecError::ResourceLimit(limit)) = &result {
            assert_eq!((session.document().clone(), annotations), before);
            drop(session);
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == *limit));
        }
        result
    });
}
