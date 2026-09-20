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
