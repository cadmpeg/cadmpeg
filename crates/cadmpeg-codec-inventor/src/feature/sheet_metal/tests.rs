// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::record_identity::{Located, RecordTypeId};
use crate::test_support::test_fixtures::{content_at, parse};
use cadmpeg_core::serde_erase::json;

use crate::feature::FeatureInventory;
use cadmpeg_ir::features::evaluation::FeatureEvaluation;
use cadmpeg_ir::features::{FeatureDefinition, FeatureId, FeatureOperation};
use std::collections::BTreeMap;

fn header(version: u8) -> Vec<u8> {
    let stamps: &[i32] = match version {
        15..=22 => &[],
        23 => &[-1],
        _ => &[-1, 8],
    };
    content_at(17, version, stamps)
}

fn reference_bytes(bytes: &mut Vec<u8>, count: u32) {
    for index in 1..=count {
        bytes.extend_from_slice(&(0x8000_0000 | index).to_le_bytes());
    }
}

fn decode(
    bytes: &[u8],
    kind: [u8; 16],
    version: u8,
) -> Result<PmDcFeaturePropertyPayload, CodecError> {
    parse(bytes, |ctx, view| {
        property_parser(kind).expect("registered property")(ctx, view, version)
    })
}

fn exact_record(bytes: &[u8], kind: [u8; 16], version: u8) -> PmDcFeaturePropertyPayload {
    let parsed = decode(bytes, kind, version).expect("complete record");
    for end in 0..bytes.len() {
        assert!(
            decode(&bytes[..end], kind, version).is_err(),
            "truncation at {end}"
        );
    }
    let mut extra = bytes.to_vec();
    extra.push(0);
    assert!(decode(&extra, kind, version).is_err());
    let wire = json::to_value(&parsed).expect("serialize property");
    assert_eq!(
        json::from_value::<PmDcFeaturePropertyPayload>(wire).expect("deserialize property"),
        parsed
    );
    parsed
}

#[test]
fn attachment_keeps_order_qualifiers_states_and_optional_reference() {
    for version in [19, 20, 23, 29, 31] {
        let mut bytes = header(version);
        reference_bytes(&mut bytes, 10);
        bytes.extend_from_slice(&[0, 1, 2]);
        bytes.extend_from_slice(&0x8000_000b_u32.to_le_bytes());
        let parsed = exact_record(&bytes, ATTACHMENT_TYPE, version);
        let expected = SheetMetalProperty::Attachment {
            control: PmDcReference::from_packed(0x8000_0001),
            thickness: PmDcReference::from_packed(0x8000_0002),
            first_mode: PmDcReference::from_packed(0x8000_0003),
            directions: [
                PmDcReference::from_packed(0x8000_0004),
                PmDcReference::from_packed(0x8000_0005),
            ],
            radius: PmDcReference::from_packed(0x8000_0006),
            second_mode: PmDcReference::from_packed(0x8000_0007),
            relief_width: PmDcReference::from_packed(0x8000_0008),
            minimum_remnant: PmDcReference::from_packed(0x8000_0009),
            first_option: PmDcReference::from_packed(0x8000_000a),
            states: NativeBytes::new([0, 1, 2]),
            second_option: PmDcReference::from_packed(0x8000_000b),
            extension: None,
        };
        assert_eq!(
            parsed.kind,
            PmDcFeaturePropertyKind::SheetMetal {
                definition: expected.clone()
            }
        );
        bytes.extend_from_slice(&12_u32.to_le_bytes());
        let extended = decode(&bytes, ATTACHMENT_TYPE, version).expect("extended attachment");
        let mut expected_extended = expected;
        if let SheetMetalProperty::Attachment { extension, .. } = &mut expected_extended {
            *extension = Some(PmDcReference::from_packed(12));
        }
        assert_eq!(
            extended.kind,
            PmDcFeaturePropertyKind::SheetMetal {
                definition: expected_extended
            }
        );
        for end in bytes.len() - 3..bytes.len() {
            assert!(decode(&bytes[..end], ATTACHMENT_TYPE, version).is_err());
        }
        let wire = json::to_value(&extended).expect("serialize extended attachment");
        assert_eq!(
            json::from_value::<PmDcFeaturePropertyPayload>(wire).expect("extended wire"),
            extended
        );
    }
}

#[test]
fn contour_attachment_has_the_extended_attachment_fields_and_one_final_byte() {
    for version in [29, 31] {
        let mut bytes = header(version);
        reference_bytes(&mut bytes, 10);
        bytes.extend_from_slice(&[1, 1, 1]);
        bytes.extend_from_slice(&0x8000_000b_u32.to_le_bytes());
        bytes.extend_from_slice(&0x8000_000c_u32.to_le_bytes());
        bytes.push(7);
        let parsed = exact_record(&bytes, CONTOUR_ATTACHMENT_TYPE, version);
        let PmDcFeaturePropertyKind::SheetMetal { definition } = parsed.kind else {
            panic!("contour attachment");
        };
        assert_eq!(
            definition.references(),
            (1..=12)
                .map(|index| PmDcReference::from_packed(0x8000_0000 | index))
                .collect::<Vec<_>>()
        );
        let SheetMetalProperty::ContourAttachment {
            directions,
            radius,
            states,
            extension,
            value,
            ..
        } = definition
        else {
            panic!("contour attachment variant");
        };
        assert_eq!([directions[0].index(), directions[1].index()], [4, 5]);
        assert_eq!(radius.index(), 6);
        assert_eq!(states.as_ref(), &[1, 1, 1]);
        assert_eq!(extension.index(), 12);
        assert_eq!(value, 7);
        let mut short = header(version);
        reference_bytes(&mut short, 10);
        short.extend_from_slice(&[1, 1, 1]);
        short.extend_from_slice(&0x8000_000b_u32.to_le_bytes());
        assert!(decode(&short, CONTOUR_ATTACHMENT_TYPE, version).is_err());
    }
}

#[test]
fn contour_attachment_validation_uses_the_extended_operand_types() {
    let mut bytes = header(31);
    reference_bytes(&mut bytes, 10);
    bytes.extend_from_slice(&[1, 1, 1]);
    bytes.extend_from_slice(&0x8000_000b_u32.to_le_bytes());
    bytes.extend_from_slice(&0x8000_000c_u32.to_le_bytes());
    bytes.push(0);
    let payload = decode(&bytes, CONTOUR_ATTACHMENT_TYPE, 31).expect("contour attachment");
    let located = Located::new(
        payload,
        RecordTypeId::from_bytes(CONTOUR_ATTACHMENT_TYPE),
        cadmpeg_ir::ids::IdentityKey::try_new("synthetic").expect("key"),
        20,
    );
    let mut types: Vec<_> = ATTACHMENT_EXTENDED_REFERENCES
        .iter()
        .map(|id| type_id_string(*id))
        .collect();
    let check = |types: &[String]| {
        valid_references(&located, |ordinal| {
            types
                .get(usize::try_from(ordinal).expect("fixture ordinal"))
                .map(String::as_str)
        })
    };
    assert!(check(&types));
    types[11] = type_id_string(ATTACHMENT_TYPE);
    assert!(!check(&types));
    types[11] = type_id_string(ATTACHMENT_STATE_TYPE);
    let mut wrong_identity = located.clone();
    wrong_identity.identity.type_id = RecordTypeId::from_bytes(ATTACHMENT_TYPE);
    assert!(!valid_references(&wrong_identity, |ordinal| types
        .get(usize::try_from(ordinal).expect("fixture ordinal"))
        .map(String::as_str)));
}

#[test]
fn relief_transition_and_corner_keep_each_reference_and_scalar() {
    for version in [19, 23, 29, 31] {
        for (kind, count) in [
            (RELIEF_TYPE, 6),
            (TRANSITION_TYPE, 3),
            (CORNER_TYPE, 9),
            (EDGE_SELECTION_TYPE, 1),
        ] {
            let mut bytes = header(version);
            reference_bytes(&mut bytes, count);
            let parsed = exact_record(&bytes, kind, version);
            let PmDcFeaturePropertyKind::SheetMetal { definition } = parsed.kind else {
                panic!("sheet-metal property");
            };
            let expected: Vec<_> = (1..=count)
                .filter(|index| kind != CORNER_TYPE || *index != 8)
                .map(|index| PmDcReference::from_packed(0x8000_0000 | index))
                .collect();
            assert_eq!(definition.references(), expected);
            if let SheetMetalProperty::Relief {
                width,
                depth,
                minimum_remnant,
                seam_gap,
                ..
            } = definition
            {
                assert_eq!(
                    [
                        width.index(),
                        depth.index(),
                        minimum_remnant.index(),
                        seam_gap.index()
                    ],
                    [2, 3, 5, 6]
                );
            }
        }
    }
}

#[test]
fn native_enumerations_and_scalar_controls_do_not_invent_value_maps() {
    for version in [19, 23, 29, 31] {
        for (kind, expected_type) in [
            (SIDE_TYPE, 9_i16),
            (RELIEF_FORM_TYPE, 304),
            (RELIEF_FORM_TYPE, 501),
            (CORNER_FORM_TYPE, 8),
            (TRANSITION_FORM_TYPE, 4),
        ] {
            let mut bytes = header(version);
            bytes.extend_from_slice(&expected_type.to_le_bytes());
            bytes.extend_from_slice(&u16::MAX.to_le_bytes());
            assert!(matches!(
                exact_record(&bytes, kind, version).kind,
                PmDcFeaturePropertyKind::SheetMetal {
                    definition: SheetMetalProperty::Enumeration {
                        type_value,
                        value: u16::MAX
                    }
                } if type_value == expected_type
            ));
            let mut invalid = header(version);
            invalid.extend_from_slice(&(-7_i16).to_le_bytes());
            invalid.extend_from_slice(&0_u16.to_le_bytes());
            assert!(decode(&invalid, kind, version).is_err());
        }
        let mut bytes = header(version);
        bytes.extend_from_slice(&0xfedc_ba98_u32.to_le_bytes());
        assert!(matches!(
            exact_record(&bytes, PLATE_CONTROL_TYPE, version).kind,
            PmDcFeaturePropertyKind::SheetMetal {
                definition: SheetMetalProperty::PlateControl { value: 0xfedc_ba98 }
            }
        ));
        assert!(matches!(
            decode(&bytes, BEND_CONTROL_TYPE, version)
                .expect("base bend control")
                .kind,
            PmDcFeaturePropertyKind::SheetMetal {
                definition: SheetMetalProperty::BendControl {
                    value: 0xfedc_ba98,
                    state: None
                }
            }
        ));
        bytes.push(3);
        assert!(matches!(
            decode(&bytes, BEND_CONTROL_TYPE, version)
                .expect("extended bend control")
                .kind,
            PmDcFeaturePropertyKind::SheetMetal {
                definition: SheetMetalProperty::BendControl {
                    value: 0xfedc_ba98,
                    state: Some(3)
                }
            }
        ));
        bytes.push(4);
        assert!(decode(&bytes, BEND_CONTROL_TYPE, version).is_err());
    }
}

#[test]
fn attachment_selection_list_uses_counted_reference_grammar() {
    let mut bytes = header(29);
    for word in [0x3000_0002_u32, 1, 1, 16, 0x8000_0007] {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    let parsed = exact_record(&bytes, ATTACHMENT_SELECTIONS_TYPE, 29);
    assert!(matches!(parsed.kind, PmDcFeaturePropertyKind::References {
        family: PmDcFeatureReferenceFamily::SheetMetalAttachmentSelections, items
    } if items.references() == [PmDcReference::from_packed(0x8000_0007)]));
    let mut invalid = header(29);
    for word in [0x3000_0002_u32, u32::MAX, 1, 16] {
        invalid.extend_from_slice(&word.to_le_bytes());
    }
    assert!(decode(&invalid, ATTACHMENT_SELECTIONS_TYPE, 29).is_err());
}

#[test]
fn sheet_metal_validation_checks_record_identity_and_operand_types() {
    let mut bytes = header(29);
    reference_bytes(&mut bytes, 6);
    let payload = decode(&bytes, RELIEF_TYPE, 29).expect("relief record");
    let located = Located::new(
        payload,
        RecordTypeId::from_bytes(RELIEF_TYPE),
        cadmpeg_ir::ids::IdentityKey::try_new("synthetic").expect("key"),
        20,
    );
    let types: Vec<_> = RELIEF_REFERENCES
        .iter()
        .map(|id| type_id_string(*id))
        .collect();
    assert!(valid_references(&located, |ordinal| types
        .get(usize::try_from(ordinal).expect("fixture ordinal fits usize"))
        .map(String::as_str)));
    assert!(!valid_references(&located, |_| None));
    assert!(!valid_references(&located, |_| Some(
        "284d8790d011f8d10008cabc0663dc09"
    )));
    let mut wrong_identity = located.clone();
    wrong_identity.identity.type_id = RecordTypeId::from_bytes(TRANSITION_TYPE);
    assert!(!valid_references(&wrong_identity, |ordinal| types
        .get(usize::try_from(ordinal).expect("fixture ordinal fits usize"))
        .map(String::as_str)));
    let mut wire = json::to_value(&located).expect("native wire");
    wire["kind"]["definition"]["width"]["index"] = cadmpeg_core::json!(0x8000_0000_u32);
    assert!(json::from_value::<PmDcFeatureProperty>(wire).is_err());
}

#[test]
fn attachment_state_has_state_header_and_ordered_operand_references() {
    for version in [19, 23, 29, 31] {
        let mut bytes = content_at(17, version, &[]);
        bytes.extend_from_slice(&(-3_i32).to_le_bytes());
        reference_bytes(&mut bytes, 7);
        let parsed = exact_record(&bytes, ATTACHMENT_STATE_TYPE, version);
        assert!(parsed.header.stamps.is_empty());
        let PmDcFeaturePropertyKind::SheetMetal { definition } = parsed.kind else {
            panic!("attachment state");
        };
        assert_eq!(
            definition.references(),
            (1..=7)
                .map(|index| PmDcReference::from_packed(0x8000_0000 | index))
                .collect::<Vec<_>>()
        );
        assert!(matches!(
            definition,
            SheetMetalProperty::AttachmentState { state: -3, .. }
        ));
    }
}

#[test]
fn attachment_state_can_reference_the_shared_hem_control_family() {
    let mut bytes = content_at(17, 29, &[]);
    bytes.extend_from_slice(&10_i32.to_le_bytes());
    reference_bytes(&mut bytes, 7);
    let payload = decode(&bytes, ATTACHMENT_STATE_TYPE, 29).expect("attachment state");
    let located = Located::new(
        payload,
        RecordTypeId::from_bytes(ATTACHMENT_STATE_TYPE),
        cadmpeg_ir::ids::IdentityKey::try_new("synthetic").expect("key"),
        20,
    );
    let mut types: Vec<_> = ATTACHMENT_STATE_REFERENCES
        .iter()
        .map(|id| type_id_string(*id))
        .collect();
    assert!(valid_references(&located, |ordinal| types
        .get(usize::try_from(ordinal).expect("fixture ordinal"))
        .map(String::as_str)));
    types[2] = type_id_string(HEM_CONTROL_TYPE);
    assert!(valid_references(&located, |ordinal| types
        .get(usize::try_from(ordinal).expect("fixture ordinal"))
        .map(String::as_str)));
    types[2] = type_id_string(CONTOUR_ATTACHMENT_TYPE);
    assert!(valid_references(&located, |ordinal| types
        .get(usize::try_from(ordinal).expect("fixture ordinal"))
        .map(String::as_str)));
    types[2] = type_id_string(super::super::EDGE_COLLECTION_TYPE);
    assert!(!valid_references(&located, |ordinal| types
        .get(usize::try_from(ordinal).expect("fixture ordinal"))
        .map(String::as_str)));
}

fn corner_loss_input() -> cadmpeg_ir::features::Feature {
    json::from_value(cadmpeg_core::json!({
        "id": "test:model:feature#corner", "ordinal": 1,
        "definition": {"kind": "operation", "value": {"kind": "native", "value": {"kind": "SheetMetalCorner", "parameters": {}}}}, "dependencies": [], "source_properties": {}, "source_content": [], "outputs": []}))
    .expect("synthetic corner feature")
}

#[test]
fn native_corner_reports_the_missing_effect_proof() {
    let mut corner = corner_loss_input();
    corner.evaluation = FeatureEvaluation::from_definition(FeatureDefinition::Operation(
        FeatureOperation::Native {
            kind: "SheetMetalCorner".into(),
            parameters: BTreeMap::new(),
        },
    ));
    let losses = native_kind_losses(&FeatureInventory::default(), &[corner]);
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        crate::loss::InventorLossCode::FeatureSheetMetalCornerEffectNative.kind()
    );
    assert!(losses[0]
        .message
        .contains("complete generated-name evidence"));
}

#[test]
fn projected_unchanged_corner_reports_every_retained_control() {
    let mut corner = corner_loss_input();
    corner.evaluation = FeatureEvaluation::from_definition(FeatureDefinition::Operation(
        FeatureOperation::SheetMetalCorner {
            bend: FeatureId::mint("test:model:feature#bend").expect("bend identity"),
            effect: cadmpeg_ir::features::sheet_metal_forms::SheetMetalCornerEffect::Unchanged,
        },
    ));
    let losses = retained_control_losses([&corner]);
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        crate::loss::InventorLossCode::FeatureSheetMetalCornerControlsRetained.kind()
    );
    for control in [
        "gap",
        "size",
        "Jacobi",
        "relief-form",
        "secondary-form",
        "type-8",
        "control",
        "option",
        "raw word",
        "dimensions",
        "profile",
        "attachment",
        "result",
    ] {
        assert!(losses[0].message.contains(control));
    }
}

fn native_corner_round() -> (FeatureInventory, cadmpeg_ir::features::Feature) {
    use crate::feature::{PmDcFeatureLabelPayload, PmDcLinkedHeader};
    use crate::test_support::sheet_metal::{feature, list, reference};
    use cadmpeg_ir::ids::IdentityKey;

    let source = feature(100, &[]);
    let label = Located::new(
        PmDcFeatureLabelPayload {
            save_version_major: 29,
            header: PmDcLinkedHeader {
                header_value: 0,
                header_id: 0,
                values: [0, 0],
                flags_extension: Some(0),
                owner: reference(101),
                parent: reference(0),
                next: reference(0),
            },
            index: 1,
            participants: list(&[]),
            name: String::new(),
            name_units: None,
            class_id: super::super::CORNER_ROUND_CLASS_ID,
        },
        RecordTypeId::from_bytes(super::super::FEATURE_LABEL_TYPE),
        IdentityKey::try_new("fixture").expect("token"),
        200,
    );
    let mut native = corner_loss_input();
    native.native_ref = Some(source.id());
    native.evaluation = FeatureEvaluation::from_definition(FeatureDefinition::Operation(
        FeatureOperation::Native {
            kind: "Fillet".into(),
            parameters: BTreeMap::new(),
        },
    ));
    (
        FeatureInventory {
            features: vec![source],
            labels: vec![label],
            ..Default::default()
        },
        native,
    )
}

#[test]
fn native_corner_round_reports_its_pinned_operand_loss() {
    let (inventory, feature) = native_corner_round();
    let losses = native_kind_losses(&inventory, &[feature]);
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        crate::loss::InventorLossCode::FeatureSheetMetalCornerRoundOperandsNative.kind()
    );
    assert_eq!(
        losses[0].code.local_code(),
        "feature.sheet-metal-corner-round-operands-native"
    );
    assert!(losses[0].message.contains("1 sheet-metal corner round(s)"));
}

#[test]
fn corner_round_loss_requires_a_unique_class_owner_and_native_source() {
    use cadmpeg_ir::ids::IdentityKey;

    let (mut inventory, feature) = native_corner_round();
    let mut label = json::to_value(&inventory.labels[0]).expect("label");
    label["class_id"] = json::to_value(super::super::FILLET_CLASS_ID).expect("class");
    inventory.labels[0] = json::from_value(label).expect("ordinary fillet label");
    assert!(native_kind_losses(&inventory, std::slice::from_ref(&feature)).is_empty());

    let (mut inventory, feature) = native_corner_round();
    inventory.labels.push(inventory.labels[0].clone());
    assert!(native_kind_losses(&inventory, std::slice::from_ref(&feature)).is_empty());

    let (mut inventory, feature) = native_corner_round();
    inventory.labels[0].identity.segment_token = IdentityKey::try_new("other").expect("token");
    assert!(native_kind_losses(&inventory, std::slice::from_ref(&feature)).is_empty());

    let (mut inventory, feature) = native_corner_round();
    let mut label = json::to_value(&inventory.labels[0]).expect("label");
    label["header"]["owner"] =
        json::to_value(crate::test_support::sheet_metal::reference(102)).expect("owner");
    inventory.labels[0] = json::from_value(label).expect("other owner");
    assert!(native_kind_losses(&inventory, std::slice::from_ref(&feature)).is_empty());

    let (inventory, mut feature) = native_corner_round();
    feature.native_ref = None;
    assert!(native_kind_losses(&inventory, &[feature]).is_empty());

    let (inventory, mut feature) = native_corner_round();
    feature.evaluation = FeatureEvaluation::from_definition(FeatureDefinition::Operation(
        FeatureOperation::Fillet {
            groups: vec![cadmpeg_ir::features::edge_treatments::FilletGroup {
                edges: cadmpeg_ir::features::selections::EdgeSelection::Native(
                    "synthetic-edges".to_owned(),
                ),
                radius: cadmpeg_ir::features::edge_treatments::RadiusSpec::Constant {
                    radius: cadmpeg_ir::scalar::PositiveLength::new(3.0).expect("radius"),
                },
                tangency_weight: None,
            }]
            .try_into()
            .expect("groups"),
        },
    ));
    assert!(native_kind_losses(&inventory, &[feature]).is_empty());
}

#[test]
fn fold_mode_and_fold_list_records_use_the_enumeration_and_list_grammars() {
    for kind in [UNFOLD_MODE_TYPE, REFOLD_MODE_TYPE] {
        let mut bytes = header(31);
        bytes.extend_from_slice(&5_i16.to_le_bytes());
        bytes.extend_from_slice(&3_u16.to_le_bytes());
        assert!(matches!(
            exact_record(&bytes, kind, 31).kind,
            PmDcFeaturePropertyKind::SheetMetal {
                definition: SheetMetalProperty::Enumeration {
                    type_value: 5,
                    value: 3
                }
            }
        ));
        let mut invalid = header(31);
        invalid.extend_from_slice(&9_i16.to_le_bytes());
        invalid.extend_from_slice(&3_u16.to_le_bytes());
        assert!(decode(&invalid, kind, 31).is_err());
    }
    let mut empty = header(31);
    empty.extend_from_slice(&0x3000_0002_u32.to_le_bytes());
    empty.extend_from_slice(&0_u32.to_le_bytes());
    assert!(matches!(
        exact_record(&empty, FOLD_LIST_TYPE, 31).kind,
        PmDcFeaturePropertyKind::References {
            family: PmDcFeatureReferenceFamily::SheetMetalFoldList,
            items
        } if items.references().is_empty()
    ));
    let mut one = header(31);
    for word in [0x3000_0002_u32, 1, 1, 16, 0x8000_0005] {
        one.extend_from_slice(&word.to_le_bytes());
    }
    assert!(matches!(
        exact_record(&one, FOLD_LIST_TYPE, 31).kind,
        PmDcFeaturePropertyKind::References {
            family: PmDcFeatureReferenceFamily::SheetMetalFoldList,
            items
        } if items.references() == [PmDcReference::from_packed(0x8000_0005)]
    ));
}

#[test]
fn a_native_face_plate_names_its_first_refused_check() {
    let (inventory, mut feature) = native_corner_round();
    feature.evaluation = FeatureEvaluation::from_definition(FeatureDefinition::Operation(
        FeatureOperation::Native {
            kind: "Plate".into(),
            parameters: BTreeMap::new(),
        },
    ));
    let source = inventory.features[0].id();
    let refusals = std::collections::BTreeMap::from([(source, "direction-normal")]);
    let losses = face_losses(&refusals, std::slice::from_ref(&feature));
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code.local_code(),
        "feature.sheet-metal-face-refused"
    );
    assert!(losses[0].message.ends_with("is direction-normal."));
    // A typed feature has no face note.
    let mut typed = feature;
    typed.evaluation = FeatureEvaluation::from_definition(FeatureDefinition::Operation(
        FeatureOperation::SheetMetalCorner {
            bend: FeatureId::mint("test:model:feature#bend").expect("bend"),
            effect: cadmpeg_ir::features::sheet_metal_forms::SheetMetalCornerEffect::Unchanged,
        },
    ));
    assert!(face_losses(&refusals, &[typed]).is_empty());
}

#[test]
fn a_native_plate_without_another_note_names_its_projection_stage() {
    let (mut inventory, mut feature) = native_corner_round();
    let mut label = json::to_value(&inventory.labels[0]).expect("label");
    label["class_id"] = json::to_value(super::super::PLATE_CLASS_ID).expect("class");
    inventory.labels[0] = json::from_value(label).expect("plate label");
    feature.evaluation = FeatureEvaluation::from_definition(FeatureDefinition::Operation(
        FeatureOperation::Native {
            kind: "Plate".into(),
            parameters: BTreeMap::new(),
        },
    ));
    // A plate without a bend and without a face refusal.
    let losses = unexplained_losses(&inventory, std::slice::from_ref(&feature), |_| false);
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code.local_code(),
        "feature.sheet-metal-native-unexplained"
    );
    assert!(losses[0].message.ends_with("the projection stage is face."));
    // A refusal explains the plate.
    assert!(unexplained_losses(&inventory, std::slice::from_ref(&feature), |_| true).is_empty());
    // A record that no label classifies and no flange owns is not sheet metal.
    inventory.labels.clear();
    assert!(unexplained_losses(&inventory, &[feature], |_| false).is_empty());
}

#[test]
fn a_missing_edge_inventory_reports_its_reason_for_selecting_plates() {
    use crate::test_support::sheet_metal::feature;
    let mut inventory = FeatureInventory::default();
    inventory.edge_inputs.unavailable = Some(crate::feature::edge_selection::Unavailable::Archive);
    // No plate selects edges: no note.
    assert!(native_kind_losses(&inventory, &[]).is_empty());
    let mut slots = vec![0; 13];
    slots[12] = 5;
    inventory.features.push(feature(100, &slots));
    let losses = native_kind_losses(&inventory, &[]);
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code.local_code(),
        "feature.edge-inventory-unavailable"
    );
    assert!(losses[0].message.ends_with(": archive."));
}

#[test]
fn a_style_record_without_a_layout_names_its_segment_version() {
    let mut inventory = FeatureInventory::default();
    assert!(native_kind_losses(&inventory, &[]).is_empty());
    inventory.unread_style_versions.extend([30, 25]);
    let losses = native_kind_losses(&inventory, &[]);
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code.local_code(),
        "feature.sheet-metal-style-version-unread"
    );
    assert!(losses[0].message.contains("version(s) 25, 30 "));
}
