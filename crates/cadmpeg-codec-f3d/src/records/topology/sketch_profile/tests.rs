// SPDX-License-Identifier: Apache-2.0
//! Sketch-profile operands and profile region members.

use serde_json::json;

fn sketch_profile_operand() -> super::DesignSketchProfileOperand {
    serde_json::from_value(json!({
        "scope_reference_ordinal": 0, "record_index": 7,
        "class_tag": "300", "paired_class_tag": "301",
        "asset_id": "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d",
        "entity_id": "Sketch_1", "entity_suffix": 1
    }))
    .unwrap()
}

#[test]
fn sketch_profile_operand_borrowed_wire_matches_owned_wire_bytes() {
    let mut operand = sketch_profile_operand();
    for selection in [None, Some(semantic_selection())] {
        operand.region_selection = selection;
        let owned = super::DesignSketchProfileOperandWire::from(operand.clone());
        assert_eq!(
            serde_json::to_vec(&operand).unwrap(),
            serde_json::to_vec(&owned).unwrap()
        );
    }
}

#[test]
fn sketch_profile_operand_native_retained_limit_refuses_before_clone() {
    #[derive(serde::Serialize)]
    struct NestedRecord<'a> {
        id: &'static str,
        value: &'a super::DesignSketchProfileOperand,
    }
    let operand = sketch_profile_operand();
    let record = NestedRecord {
        id: "f3d:native:sketch-profile#0",
        value: &operand,
    };
    crate::test_support::native_test::assert_borrowed_native_retained_limit(
        &record,
        "design_parameter_scopes",
        || super::SKETCH_PROFILE_OPERAND_CLONE_COUNT.with(|count| count.set(0)),
        || super::SKETCH_PROFILE_OPERAND_CLONE_COUNT.with(std::cell::Cell::get),
    );
}

#[test]
fn profile_region_member_preserves_fixed_words_and_closed_incidence_values() {
    let wire = |kind: u32, identity: u64, words: [u32; 8]| {
        format!(
            "{{\"kind\":{kind},\"curve_primary_id\":{identity},\"incidence_words\":{}}}",
            serde_json::to_string(&words).expect("incidence words")
        )
    };
    for identity in [1, u64::from(u32::MAX)] {
        for flag in [0, 1] {
            for first in [1, 2] {
                for second in [1, 2] {
                    let json = wire(3, identity, [0, 0, 0, flag, first, second, 0, 0]);
                    let member: super::DesignSketchProfileRegionMember =
                        serde_json::from_str(&json).expect("region member");
                    assert_eq!(
                        serde_json::to_string(&member).expect("region member wire"),
                        json
                    );
                }
            }
        }
    }
    for kind in [0, 1, 2, 4, u32::MAX] {
        let error = serde_json::from_str::<super::DesignSketchProfileRegionMember>(&wire(
            kind,
            1,
            [0, 0, 0, 0, 1, 1, 0, 0],
        ))
        .expect_err("fixed kind");
        assert!(error.to_string().contains("kind"));
    }
    for identity in [0, u64::from(u32::MAX) + 1, u64::MAX] {
        let error = serde_json::from_str::<super::DesignSketchProfileRegionMember>(&wire(
            3,
            identity,
            [0, 0, 0, 0, 1, 1, 0, 0],
        ))
        .expect_err("nonzero u32 identity");
        assert!(error.to_string().contains("curve_primary_id"));
    }
    for index in 0..8 {
        let mut words = [0, 0, 0, 0, 1, 1, 0, 0];
        words[index] = 3;
        let error =
            serde_json::from_str::<super::DesignSketchProfileRegionMember>(&wire(3, 1, words))
                .expect_err("invalid incidence word");
        assert!(error.to_string().contains("incidence_words"));
    }
}

fn semantic_selection() -> super::DesignSketchProfileRegionSelection {
    serde_json::from_value(json!({
        "record_index": 10, "class_tag": "302", "companion_class_tag": "303",
        "regions": [{"members": [{"kind": 3, "curve_primary_id": 1,
            "incidence_words": [0,0,0,1,1,2,0,0]}]}]
    }))
    .unwrap()
}

fn source_draft() -> super::DesignSketchProfileOperandDraft {
    let mut selection = semantic_selection();
    selection.byte_offset = Some(200);
    selection.region_count_offset = Some(236);
    selection.companion_byte_offset = Some(289);
    let region = &mut selection.regions[0];
    region.member_count_offset = Some(240);
    let member = &mut region.members[0];
    member.kind_offset = Some(244);
    member.curve_primary_id_offset = Some(248);
    member.incidence_words_offset = Some(252);
    super::DesignSketchProfileOperandDraft {
        scope_reference_ordinal: 0,
        record_index: 7,
        byte_offset: 0,
        class_tag: "300".to_owned().try_into().unwrap(),
        paired_class_tag: "301".to_owned().try_into().unwrap(),
        paired_byte_offset: 160,
        asset_id: "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d"
            .to_owned()
            .try_into()
            .unwrap(),
        asset_id_offset: 32,
        entity_id: "Sketch_1".to_owned().try_into().unwrap(),
        entity_reference_offset: 80,
        region_selection: Some(selection),
    }
}

#[test]
fn sketch_profile_document_has_ordered_region_members_without_source_positions() {
    let source = super::DesignSketchProfileOperand::try_new(source_draft()).unwrap();
    let wire = serde_json::to_value(&source).unwrap();
    let restored: super::DesignSketchProfileOperand = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(restored.byte_offset(), None);
    assert_eq!(restored.paired_byte_offset(), None);
    assert!(restored.clone().into_draft().is_err());
    assert_eq!(serde_json::to_value(&restored).unwrap(), wire);
    let selection = restored.region_selection.as_ref().unwrap();
    assert!(restored.region_selection_is_admitted(selection));
    assert_eq!(selection.byte_offset, None);
    assert_eq!(selection.regions[0].member_count_offset, None);
    assert_eq!(selection.regions[0].members[0].incidence_words_offset, None);
    let pointers = crate::records::source_offsets::collect(&mut source.clone());
    assert_eq!(
        pointers["region_selection.regions.0.members.0.incidence_words_offset"],
        252
    );
    assert!(crate::records::source_offsets::collect(&mut restored.clone()).is_empty());
    for field in [
        "byte_offset",
        "asset_id_offset",
        "entity_reference_offset",
        "paired_byte_offset",
    ] {
        let mut invalid = wire.clone();
        invalid[field] = 0.into();
        assert!(
            serde_json::from_value::<super::DesignSketchProfileOperand>(invalid).is_err(),
            "{field}"
        );
    }
    for field in [
        "byte_offset",
        "region_count_offset",
        "companion_byte_offset",
    ] {
        let mut invalid = wire.clone();
        invalid["region_selection"][field] = 0.into();
        assert!(
            serde_json::from_value::<super::DesignSketchProfileOperand>(invalid).is_err(),
            "{field}"
        );
    }
    for field in [
        "kind_offset",
        "curve_primary_id_offset",
        "incidence_words_offset",
    ] {
        let mut invalid = wire.clone();
        invalid["region_selection"]["regions"][0]["members"][0][field] = 0.into();
        assert!(
            serde_json::from_value::<super::DesignSketchProfileOperand>(invalid).is_err(),
            "{field}"
        );
    }
    let mut no_regions = wire.clone();
    no_regions["region_selection"]["regions"] = json!([]);
    assert!(serde_json::from_value::<super::DesignSketchProfileOperand>(no_regions).is_err());
    let mut no_members = wire.clone();
    no_members["region_selection"]["regions"][0]["members"] = json!([]);
    assert!(serde_json::from_value::<super::DesignSketchProfileOperand>(no_members).is_err());
    let mut wrong_record = wire;
    wrong_record["region_selection"]["record_index"] = 9.into();
    assert!(serde_json::from_value::<super::DesignSketchProfileOperand>(wrong_record).is_err());
}

#[test]
fn sketch_profile_source_admission_preserves_real_positions_and_rejects_broken_frames() {
    use super::{DesignSketchProfileOperand, DesignSketchProfileOperandDraft};
    let source = DesignSketchProfileOperand::try_new(source_draft()).unwrap();
    assert_eq!(source.into_draft().unwrap(), source_draft());
    for mutate in [
        |draft: &mut DesignSketchProfileOperandDraft| draft.byte_offset = draft.asset_id_offset,
        |draft: &mut DesignSketchProfileOperandDraft| draft.asset_id_offset = 0,
        |draft: &mut DesignSketchProfileOperandDraft| draft.entity_reference_offset = 32,
        |draft: &mut DesignSketchProfileOperandDraft| draft.paired_byte_offset = 80,
        |draft: &mut DesignSketchProfileOperandDraft| {
            draft.region_selection.as_mut().unwrap().region_count_offset = Some(0);
        },
        |draft: &mut DesignSketchProfileOperandDraft| {
            draft
                .region_selection
                .as_mut()
                .unwrap()
                .companion_byte_offset = None;
        },
        |draft: &mut DesignSketchProfileOperandDraft| {
            draft.region_selection.as_mut().unwrap().regions[0].member_count_offset = Some(0);
        },
        |draft: &mut DesignSketchProfileOperandDraft| {
            draft.region_selection.as_mut().unwrap().regions[0].members[0].kind_offset = None;
        },
        |draft: &mut DesignSketchProfileOperandDraft| {
            draft.region_selection.as_mut().unwrap().regions[0].members[0]
                .curve_primary_id_offset = Some(0);
        },
        |draft: &mut DesignSketchProfileOperandDraft| {
            draft.region_selection.as_mut().unwrap().regions[0].members[0].incidence_words_offset =
                Some(0);
        },
    ] {
        let mut invalid = source_draft();
        mutate(&mut invalid);
        assert!(DesignSketchProfileOperand::try_new(invalid).is_err());
    }
}
