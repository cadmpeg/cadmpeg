// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::record_identity::{Located, RecordTypeId};
use crate::test_support::test_fixtures::{content_at, parse};

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
    let wire = serde_json::to_value(&parsed).expect("serialize property");
    assert_eq!(
        serde_json::from_value::<PmDcFeaturePropertyPayload>(wire).expect("deserialize property"),
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
        let wire = serde_json::to_value(&extended).expect("serialize extended attachment");
        assert_eq!(
            serde_json::from_value::<PmDcFeaturePropertyPayload>(wire).expect("extended wire"),
            extended
        );
    }
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
    let mut wire = serde_json::to_value(&located).expect("native wire");
    wire["kind"]["definition"]["width"]["index"] = serde_json::json!(0x8000_0000_u32);
    assert!(serde_json::from_value::<PmDcFeatureProperty>(wire).is_err());
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
    types[2] = type_id_string(super::super::EDGE_COLLECTION_TYPE);
    assert!(!valid_references(&located, |ordinal| types
        .get(usize::try_from(ordinal).expect("fixture ordinal"))
        .map(String::as_str)));
}
