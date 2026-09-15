// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::document::Model;
use crate::features::{
    Feature, FeatureDefinition, FeatureOperation, FeatureTreeNodeRole, TreeChildren,
};

fn parent_draft() -> ModelDraft {
    let mut draft = ModelDraft::new();
    for (key, ordinal) in [("0-parent", 0), ("1-child", 1)] {
        draft
            .insert(Feature::new(
                format!("test:parents:feature#{key}").try_into().unwrap(),
                ordinal,
                FeatureDefinition::Operation(FeatureOperation::StoredGeometry {}),
            ))
            .unwrap();
    }
    draft
        .model_mut()
        .set_feature_regeneration_parent(
            "test:parents:feature#1-child".try_into().unwrap(),
            "test:parents:feature#0-parent".try_into().unwrap(),
        )
        .unwrap();
    draft
}

fn commit(model: Model, base: &mut CadIr, use_session: bool) -> Result<(), DraftError> {
    let mut draft = ModelDraft::new();
    *draft.model_mut() = model;
    if use_session {
        CommitSession::new(base).commit_model(draft)
    } else {
        draft.commit_model(base)
    }
}

#[test]
fn parent_admission_rejects_records_removed_or_reordered_after_the_checked_setter() {
    for mutation in ["missing parent", "missing child", "ordinal"] {
        let mut draft = parent_draft();
        match mutation {
            "missing parent" => {
                draft.model_mut().features.remove(0);
            }
            "missing child" => {
                draft.model_mut().features.remove(1);
            }
            "ordinal" => {
                draft.model_mut().features[1].ordinal = 0;
            }
            _ => unreachable!(),
        }
        for use_session in [false, true] {
            let mut base = crate::examples::unit_cube().unwrap();
            let before = base.clone();
            let error = commit(draft.model().clone(), &mut base, use_session).unwrap_err();
            assert!(
                matches!(error, DraftError::FeatureParents { ref owner, .. }
                if owner.as_str() == "test:parents:feature#1-child"),
                "{error}"
            );
            assert_eq!(base, before);
        }
    }
}

#[test]
fn parent_admission_preserves_a_predecessor_moved_to_the_destination() {
    for use_session in [false, true] {
        let mut draft = parent_draft();
        let mut base = CadIr::empty();
        base.model
            .features
            .push(draft.model_mut().features.remove(0));
        commit(draft.model().clone(), &mut base, use_session).unwrap();
        let read = CadIr::from_json(&base.to_canonical_json().unwrap()).unwrap();
        assert_eq!(read, base);
        assert_eq!(
            read.model
                .feature_regeneration_parent(&"test:parents:feature#1-child".try_into().unwrap())
                .unwrap()
                .as_str(),
            "test:parents:feature#0-parent"
        );
    }
}

#[test]
fn parent_admission_checks_tree_ownership_across_the_destination_and_draft() {
    let mut draft = parent_draft();
    let mut base = CadIr::empty();
    let mut parent = draft.model_mut().features.remove(0);
    parent
        .evaluation
        .set_definition(FeatureDefinition::Operation(FeatureOperation::TreeNode {
            role: FeatureTreeNodeRole::History,
            children: TreeChildren::new(
                vec!["test:parents:feature#1-child".try_into().unwrap()],
                None,
            )
            .unwrap(),
        }));
    base.model.features.push(parent);
    for use_session in [false, true] {
        let mut destination = base.clone();
        let error = commit(draft.model().clone(), &mut destination, use_session).unwrap_err();
        assert!(
            matches!(error, DraftError::FeatureParents { .. }),
            "{error}"
        );
        assert!(error.to_string().contains("states no regeneration parent"));
        assert_eq!(destination, base);
    }
}

#[test]
fn incomplete_commit_discards_only_parent_entries_owned_by_discarded_children() {
    let mut base = CadIr::empty();
    parent_draft()
        .with_accounting()
        .commit_incomplete(
            &mut base,
            &mut Annotations::default(),
            &mut Vec::new(),
            &mut TransferLedger::default(),
            |_, id| id != "test:parents:feature#1-child",
        )
        .unwrap();
    assert_eq!(base.model.features.len(), 1);
    assert!(base
        .model
        .feature_regeneration_parent(&"test:parents:feature#1-child".try_into().unwrap())
        .is_none());
    assert_eq!(
        CadIr::from_json(&base.to_canonical_json().unwrap()).unwrap(),
        base
    );

    let mut rejected_base = CadIr::empty();
    let before = rejected_base.clone();
    let error = parent_draft()
        .with_accounting()
        .commit_incomplete(
            &mut rejected_base,
            &mut Annotations::default(),
            &mut Vec::new(),
            &mut TransferLedger::default(),
            |_, id| id != "test:parents:feature#0-parent",
        )
        .unwrap_err();
    assert!(
        matches!(error, DraftError::FeatureParents { .. }),
        "{error}"
    );
    assert_eq!(rejected_base, before);
}
