// SPDX-License-Identifier: Apache-2.0

use crate::annotations::Annotations;
use crate::document::CadIr;
use crate::draft::tests::point;
use crate::draft::tests::point_draft;
use crate::draft::DraftError;
use crate::report::TransferLedger;

use crate::provenance::Exactness;
use crate::report::{LossNote, TransferOutcome};

fn loss_note() -> LossNote {
    serde_json::from_value(serde_json::json!({
        "code": {"scope": "shared", "kind": "pcurve_omitted"},
        "severity": "warning",
        "message": "test:source:record#omitted has no transferred pcurve"
    }))
    .expect("admitted loss fixture")
}

#[test]
fn adding_accounting_preserves_entities_and_commits_every_accounting_channel() {
    let identity = "test:model:point#accounted";
    let mut draft = point_draft(identity).with_accounting();
    assert_eq!(draft.model().points, vec![point(identity)]);
    assert_eq!(
        draft.insert(point(identity)),
        Err(DraftError::IdentityCollision(identity.into()))
    );
    draft.exactness(identity, Exactness::Derived);
    let note = loss_note();
    draft.note(note.clone());
    draft.ledger_mut().record(
        "test:source:record#point",
        TransferOutcome::Emitted {
            target: identity.into(),
        },
    );
    let staged_ledger = draft.ledger_mut().clone();

    let mut base = CadIr::empty();
    let mut annotations = Annotations::default();
    let mut notes = Vec::new();
    let mut ledger = TransferLedger::default();
    draft
        .commit(&mut base, &mut annotations, &mut notes, &mut ledger)
        .unwrap();

    assert_eq!(base.model.points, vec![point(identity)]);
    assert_eq!(
        annotations.exactness()[identity].entity(),
        Exactness::Derived
    );
    assert_eq!(notes, vec![note]);
    assert_eq!(ledger, staged_ledger);
}

#[test]
fn refused_accounted_draft_leaves_all_existing_destinations_unchanged() {
    let existing = "test:model:point#existing";
    let mut base = CadIr::empty();
    base.model.points.push(point(existing));
    let mut builder = crate::annotations::AnnotationBuilder::new();
    builder.exactness(existing, Exactness::Inferred);
    let mut annotations = builder.build();
    let mut notes = vec![loss_note()];
    let mut ledger = TransferLedger::default();
    ledger.record(
        "test:source:record#existing",
        TransferOutcome::Emitted {
            target: existing.into(),
        },
    );
    let before = (
        base.clone(),
        annotations.clone(),
        notes.clone(),
        ledger.clone(),
    );

    let mut draft = point_draft(existing).with_accounting();
    draft.exactness(existing, Exactness::Derived);
    draft.note(loss_note());
    draft.ledger_mut().record(
        "test:source:record#rejected",
        TransferOutcome::Omitted { note: None },
    );
    assert_eq!(
        draft.commit(&mut base, &mut annotations, &mut notes, &mut ledger),
        Err(DraftError::IdentityCollision(existing.into()))
    );
    assert_eq!((base, annotations, notes, ledger), before);
}
