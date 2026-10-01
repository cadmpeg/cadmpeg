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
        draft.insert(point(identity)),
        Err(DraftError::IdentityCollision(identity.into()))
    );
    draft.exactness(identity, Exactness::Derived);
    let mut base = CadIr::empty();
    let mut annotations = Annotations::default();
    draft.commit(&mut base, &mut annotations).unwrap();

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
    builder.exactness(existing, Exactness::Inferred);
    let mut annotations = builder.build();
    let before = (base.clone(), annotations.clone());

    let mut draft = point_draft(existing).with_accounting();
    draft.exactness(existing, Exactness::Derived);
    assert_eq!(
        draft.commit(&mut base, &mut annotations),
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
    builder.exactness(identity, Exactness::Inferred);
    let mut annotations = builder.build();
    let before = (base.clone(), annotations.clone());
    let mut draft = point_draft(identity).with_accounting();
    draft.exactness(identity, Exactness::Derived);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = u64::try_from(identity.len()).unwrap();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert_eq!(draft.commit_for_decode(&mut base, &mut annotations, &ctx).unwrap(),
        Err(DraftError::IdentityCollision(identity.into())));
    assert_eq!((base, annotations), before);
    ctx.finish_session().unwrap();
}

#[test]
fn accounted_draft_retained_transfer_refusal_leaves_model_and_annotations_unchanged() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let identity = "test:model:point#new";
    let mut draft = point_draft(identity).with_accounting();
    draft.exactness(identity, Exactness::Derived);
    let mut base = CadIr::empty();
    let mut annotations = Annotations::default();
    let before = (base.clone(), annotations.clone());
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // The destination point arena can be admitted; the annotation transfer cannot.
    policy.limits.max_retained_bytes = u64::try_from(4 * std::mem::size_of::<crate::topology::Point>()).unwrap();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = draft.commit_for_decode(&mut base, &mut annotations, &ctx).unwrap_err();
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
        && limit.operation == "draft annotation transaction"));
    assert_eq!((base, annotations), before);
}
