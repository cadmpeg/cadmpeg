// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::feature::tests::{reference_list, test_feature, test_label, test_property};
use std::collections::HashMap;

fn types() -> HashMap<u32, &'static str> {
    HashMap::from([(33, WRAPPER_TYPE_ID)])
}

fn boolean(ordinal: u32, value: bool) -> PmDcFeatureProperty {
    test_property(
        ordinal,
        PmDcFeaturePropertyKind::Boolean {
            name: String::new(),
            name_value: 0,
            value,
        },
    )
}

fn properties(slot_1: bool, slot_2: bool) -> Vec<PmDcFeatureProperty> {
    vec![
        test_property(
            20,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::FaceCollection,
                items: reference_list(&[31, 32]),
            },
        ),
        boolean(21, slot_1),
        boolean(22, slot_2),
        test_property(
            23,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::BodySelection,
                items: reference_list(&[34]),
            },
        ),
    ]
}

fn source() -> PmDcFeature {
    test_feature(0, 4, &[(0, 20), (1, 21), (2, 22), (3, 23)])
}

fn read(
    source: &PmDcFeature,
    properties: &[PmDcFeatureProperty],
    types: &HashMap<u32, &'static str>,
) -> NativeReason {
    refusal(
        source,
        |reference| {
            properties
                .iter()
                .find(|property| property.identity.record_ordinal + 1 == reference.index())
        },
        |ordinal| types.get(&ordinal).copied(),
    )
}

#[test]
fn every_stored_form_stays_native_with_unresolved_controls() {
    for pair in [(true, false), (false, false), (false, true), (true, true)] {
        assert_eq!(
            read(&source(), &properties(pair.0, pair.1), &types()),
            NativeReason::Controls
        );
    }
    assert_eq!(admission(&source()), Err(NativeReason::Controls));
}

#[test]
fn delete_face_refusals_name_the_failing_slot() {
    let all = types();
    let ok = properties(true, false);
    assert_eq!(
        read(&test_feature(0, 3, &[]), &ok, &all),
        NativeReason::Slots
    );
    let collection = |family, members: &[u32]| {
        test_property(
            20,
            PmDcFeaturePropertyKind::References {
                family,
                items: reference_list(members),
            },
        )
    };
    let faces = PmDcFeatureReferenceFamily::FaceCollection;
    let mut repeated = ok.clone();
    repeated[0] = collection(faces, &[32, 32]);
    assert_eq!(read(&source(), &repeated, &all), NativeReason::Faces);
    let mut empty = ok.clone();
    empty[0] = collection(faces, &[]);
    assert_eq!(read(&source(), &empty, &all), NativeReason::Faces);
    let mut selection = ok.clone();
    selection[0] = collection(PmDcFeatureReferenceFamily::SelectionList, &[32]);
    assert_eq!(read(&source(), &selection, &all), NativeReason::Faces);
    let not_boolean = |ordinal| {
        test_property(
            ordinal,
            PmDcFeaturePropertyKind::WideEnumeration {
                type_value: 0,
                value: 0,
            },
        )
    };
    let mut slot_1 = ok.clone();
    slot_1[1] = not_boolean(21);
    assert_eq!(read(&source(), &slot_1, &all), NativeReason::Slot1);
    let mut slot_2 = ok.clone();
    slot_2[2] = not_boolean(22);
    assert_eq!(read(&source(), &slot_2, &all), NativeReason::Slot2);
    let mut absent = ok.clone();
    absent.remove(2);
    assert_eq!(read(&source(), &absent, &all), NativeReason::Slot2);
    let body_selection = |members: &[u32]| {
        test_property(
            23,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::BodySelection,
                items: reference_list(members),
            },
        )
    };
    let mut two = ok.clone();
    two[3] = body_selection(&[34, 35]);
    assert_eq!(read(&source(), &two, &all), NativeReason::Selection);
    let mut none = ok.clone();
    none[3] = body_selection(&[0]);
    assert_eq!(read(&source(), &none, &all), NativeReason::Selection);
    let mut foreign = all;
    foreign.insert(33, "other");
    assert_eq!(read(&source(), &ok, &foreign), NativeReason::Selection);
}

fn placed(source: &PmDcFeature) -> Vec<cadmpeg_ir::features::Feature> {
    use cadmpeg_ir::features::evaluation::FeatureEvaluation;
    use cadmpeg_ir::features::{Feature, FeatureContent, FeatureId, NativeFeatureKind};
    vec![Feature {
        id: FeatureId::compose(
            &cadmpeg_ir::identity_namespace!("inventor", "design", "feature"),
            cadmpeg_ir::identity_key!("generated"),
        ),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: cadmpeg_ir::features::selections::DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::default(),
        source_tag: None,
        source_text: None,
        source_content: FeatureContent::default(),
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
            FeatureOperation::Native {
                kind: NativeFeatureKind::DeleteFace,
                parameters: std::collections::BTreeMap::default(),
            },
        )),
        native_ref: Some(source.id()),
        product_definition: None,
    }]
}

fn inventory(source: PmDcFeature, class: ClassId, pair: (bool, bool)) -> FeatureInventory {
    let mut inventory = FeatureInventory {
        features: vec![source],
        labels: vec![test_label(0, 1, class, &[])],
        properties: properties(pair.0, pair.1),
        ..Default::default()
    };
    inventory.record_types.insert(
        crate::feature::tests::segment(),
        types()
            .into_iter()
            .map(|(ordinal, type_id)| (ordinal, Some(type_id.to_owned())))
            .collect(),
    );
    inventory
}

#[test]
fn a_refused_record_that_stays_native_emits_the_pinned_loss() {
    let source = source();
    let features = placed(&source);
    let inventory = inventory(source, CLASS_ID, (false, true));
    assert!(native_losses(&inventory, &[]).is_empty());
    let losses = native_losses(&inventory, &features);
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code.local_code(),
        "feature.delete-face-operands-native"
    );
    assert_eq!(
        losses[0].code.local_code(),
        InventorLossCode::FeatureDeleteFaceOperandsNative.code()
    );
    assert!(losses[0].message.contains("Boolean pair"));
    assert_eq!(losses.len(), 1);
}

#[test]
fn the_native_loss_uses_the_label_class_and_record_owner() {
    let source = source();
    let features = placed(&source);
    let mut inventory = inventory(source, CLASS_ID, (false, true));
    inventory.labels = vec![test_label(1, 1, CLASS_ID, &[])];
    assert!(native_losses(&inventory, &features).is_empty());
    inventory.labels = vec![test_label(0, 1, super::super::THREAD_CLASS_ID, &[])];
    assert!(native_losses(&inventory, &features).is_empty());
    inventory.labels = vec![test_label(0, 1, CLASS_ID, &[])];
    assert_eq!(native_losses(&inventory, &features).len(), 1);
}
