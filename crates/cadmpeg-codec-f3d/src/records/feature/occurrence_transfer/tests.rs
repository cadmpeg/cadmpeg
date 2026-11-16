// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::records::feature::scope::DesignFeatureKind;
use crate::records::identity::ReferenceRun;

fn scope() -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#1000",
        DesignFeatureKind::CopyPaste,
        21,
    );
    scope
        .try_edit(|draft| {
            draft.byte_offset = 1000;
            draft.frame_length = 525;
            draft.paired_byte_offset = 1525;
            draft.reference_count_offset = 1397;
            draft.reference_members = ReferenceRun::unlocated(vec![14]);
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    scope.occurrence_transfer = Some(DesignOccurrenceTransfer {
        relation_record_index: 14,
        occurrence_record_index: 10,
        component_record_index: 11,
        target_owner: DesignPasteOwner {
            collection: 8,
            context: 6,
            component: 3,
        },
        target_transform: SketchPlacementMatrix::IDENTITY,
        target_transform_offset: Some(1190),
        source: DesignOccurrenceTransferSource::Copy {
            source_transform: SketchPlacementMatrix::IDENTITY,
            source_transform_offset: 1034,
        },
    });
    scope
}

fn intent_scope() -> DesignParameterScope {
    let mut scope = scope();
    scope.occurrence_transfer_intent = Some(DesignOccurrenceTransferIntent {
        mode: DesignOccurrenceTransferMode::Copy,
        relation_record_index: Some(14),
        occurrence_record_index: Some(10),
        component_record_index: Some(11),
        source_owner_component_record_index: None,
        target_owner_component_record_index: Some(3),
        source_transform: Some(SketchPlacementMatrix::IDENTITY),
        target_transform: Some(SketchPlacementMatrix::IDENTITY),
    });
    scope
}

#[test]
fn occurrence_transfer_wire_admits_valid_scope_and_rejects_inconsistent_fields() {
    let scope = scope();
    let wire = serde_json::to_value(&scope).unwrap();
    let admitted: DesignParameterScope = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(admitted.occurrence_transfer, scope.occurrence_transfer);
    for (field, value) in [
        ("target_transform_offset", serde_json::json!(1)),
        ("relation_record_index", serde_json::json!(15)),
        ("occurrence_record_index", serde_json::json!(0)),
        (
            "component_record_index",
            serde_json::json!(4_294_967_296_u64),
        ),
    ] {
        let mut changed = wire.clone();
        changed["occurrence_transfer"][field] = value;
        assert!(
            serde_json::from_value::<DesignParameterScope>(changed).is_err(),
            "{field}"
        );
    }
    let mut changed = wire;
    changed["occurrence_transfer"]["source"]["source_transform_offset"] = 1.into();
    assert!(serde_json::from_value::<DesignParameterScope>(changed).is_err());
}

#[test]
fn occurrence_transfer_links_require_existing_owners_and_prior_records() {
    let scope = scope();
    let value = scope.occurrence_transfer.as_ref().unwrap();
    assert!(value.links_valid(&scope, |index| Some(u64::from(index)), |_, _| true));
    for missing in [8, 10] {
        assert!(!value.links_valid(
            &scope,
            |index| (index != missing).then_some(u64::from(index)),
            |_, _| true
        ));
    }
    assert!(!value.links_valid(&scope, |_| Some(1000), |_, _| true));
    assert!(!value.links_valid(
        &scope,
        |index| Some(if index == 6 { 9 } else { u64::from(index) }),
        |_, _| true
    ));
    assert!(!value.links_valid(&scope, |index| Some(u64::from(index)), |_, _| false));
    assert!(!value.links_valid(
        &scope,
        |index| Some(if index == 10 { 15 } else { u64::from(index) }),
        |_, _| true
    ));
}

#[test]
fn occurrence_transfer_intent_wire_keeps_unresolved_optionals_and_rejects_bad_indices() {
    let scope = intent_scope();
    let wire = serde_json::to_value(&scope).unwrap();
    let admitted: DesignParameterScope = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(
        admitted.occurrence_transfer_intent,
        scope.occurrence_transfer_intent
    );
    for (field, value) in [
        ("relation_record_index", serde_json::json!(0)),
        ("occurrence_record_index", serde_json::json!(0)),
        ("component_record_index", serde_json::json!(0)),
        (
            "component_record_index",
            serde_json::json!(4_294_967_296_u64),
        ),
    ] {
        let mut changed = wire.clone();
        changed["occurrence_transfer_intent"][field] = value;
        assert!(
            serde_json::from_value::<DesignParameterScope>(changed).is_err(),
            "{field}"
        );
    }
    let mut changed = wire.clone();
    changed["occurrence_transfer_intent"]["source_owner_component_record_index"] = 3.into();
    assert!(serde_json::from_value::<DesignParameterScope>(changed).is_err());

    let mut changed = wire.clone();
    changed
        .as_object_mut()
        .unwrap()
        .remove("occurrence_transfer");
    changed["kind"] = "CutPaste".into();
    changed["occurrence_transfer_intent"]["mode"] = "reparent".into();
    assert!(serde_json::from_value::<DesignParameterScope>(changed).is_err());

    let mut unresolved = intent_scope();
    let intent = unresolved.occurrence_transfer_intent.as_mut().unwrap();
    intent.relation_record_index = None;
    intent.occurrence_record_index = None;
    intent.component_record_index = None;
    intent.target_owner_component_record_index = None;
    intent.source_transform = None;
    intent.target_transform = None;
    let unresolved_wire = serde_json::to_value(unresolved).unwrap();
    assert!(serde_json::from_value::<DesignParameterScope>(unresolved_wire).is_ok());
}
