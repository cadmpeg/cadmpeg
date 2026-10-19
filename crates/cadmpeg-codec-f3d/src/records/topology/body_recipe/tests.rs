// SPDX-License-Identifier: Apache-2.0
//! Semantic body-recipe records and actual source frame admission.

use super::{
    DesignBodyRecipeOperand, DesignBodyRecipeOperandDraft, DesignBodyRecipeOperandWire,
    DesignBodyRecipeReference, DesignOperandOwner,
};
use crate::records::identity::Located;
use serde_json::{Value, json};

fn source_draft() -> DesignBodyRecipeOperandDraft {
    DesignBodyRecipeOperandDraft {
        id: "f3d:Design:body-recipe#0".into(),
        scope_record_index: 1,
        owner: DesignOperandOwner::ScopeReference {
            scope_reference_ordinal: 0,
        },
        record_index: 2,
        byte_offset: 0,
        class_tag: "365".try_into().unwrap(),
        asset_id: "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d".try_into().unwrap(),
        asset_id_offset: 56,
        context_id: "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e".try_into().unwrap(),
        context_id_offset: 150,
        selector_tail: Some(Located {
            value: [7, 0, 0, 0],
            offset: 220,
        }),
        references: vec![DesignBodyRecipeReference {
            design_reference: 1,
            design_reference_offset: Some(25),
            form: 0,
            form_offset: Some(33),
            candidate_faces: Vec::new(),
            preceding_candidate_faces: Vec::new(),
            preceding_body_slots: Vec::new(),
        }],
        nested_record_index: 5,
        nested_record_index_offset: 38,
        recipe_id: "recipe".into(),
        resolved_face_slot: None,
        resolved_body_state_id: None,
        resolved_body_slot: None,
        resolved_body_face_slots: Vec::new(),
        next_record_index: 6,
        next_byte_offset: 240,
    }
}

fn source_record() -> DesignBodyRecipeOperand {
    DesignBodyRecipeOperand::try_new(source_draft()).unwrap()
}

#[test]
fn body_recipe_operand_borrowed_wire_matches_owned_wire_bytes() {
    let value = source_record();
    let owned = DesignBodyRecipeOperandWire::from(value.clone());
    assert_eq!(
        serde_json::to_vec(&value).unwrap(),
        serde_json::to_vec(&owned).unwrap()
    );
}

#[test]
fn body_recipe_operand_native_retained_limit_refuses_before_clone() {
    #[derive(serde::Serialize)]
    struct NestedRecord<'a> {
        id: &'static str,
        value: &'a DesignBodyRecipeOperand,
    }
    let value = source_record();
    let record = NestedRecord {
        id: "f3d:native:body-recipe#0",
        value: &value,
    };
    crate::test_support::native_test::assert_borrowed_native_retained_limit(
        &record,
        "design_parameter_scopes",
        || super::BODY_RECIPE_OPERAND_CLONE_COUNT.with(|count| count.set(0)),
        || super::BODY_RECIPE_OPERAND_CLONE_COUNT.with(std::cell::Cell::get),
    );
}

#[test]
fn body_recipe_document_keeps_semantic_chain_and_evidence_without_layout_or_bytes() {
    let record = source_record();
    let wire = serde_json::to_value(&record).unwrap();
    assert_eq!(wire["selector_tail"].as_object().unwrap().len(), 2);
    assert_eq!(wire["selector_tail"]["length"], 4);
    assert_eq!(
        wire["references"][0],
        json!({"design_reference": 1, "form": 0})
    );
    for key in [
        "byte_offset",
        "asset_id_offset",
        "context_id_offset",
        "selector_tail_offset",
        "nested_record_index_offset",
        "next_byte_offset",
    ] {
        assert!(wire.get(key).is_none(), "{key}");
        let mut invalid = wire.clone();
        invalid[key] = 0.into();
        assert!(
            serde_json::from_value::<DesignBodyRecipeOperand>(invalid).is_err(),
            "{key}"
        );
    }
    let restored: DesignBodyRecipeOperand = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(restored.record_index(), 2);
    assert_eq!(restored.nested_record_index(), 5);
    assert_eq!(restored.next_record_index(), 6);
    assert_eq!(restored.byte_offset(), None);
    assert_eq!(restored.context_id_offset(), None);
    assert_eq!(restored.next_byte_offset(), None);
    assert_eq!(restored.references()[0].design_reference_offset, None);
    assert_eq!(restored.references()[0].form_offset, None);
    assert!(restored.selector_tail.as_ref().unwrap().data().is_none());
    assert!(restored.clone().into_draft().is_err());
    assert_eq!(serde_json::to_value(&restored).unwrap(), wire);
    let mut fidelity = cadmpeg_ir::SourceFidelity::default();
    restored.retain_source(&mut fidelity).unwrap();
    assert!(fidelity.metadata.is_empty());
    assert!(fidelity.retained_records().is_empty());
}

#[test]
fn body_recipe_untyped_selector_tail_is_retained_outside_the_document() {
    let record = source_record();
    let mut fidelity = cadmpeg_ir::SourceFidelity::default();
    record.retain_source(&mut fidelity).unwrap();
    assert_eq!(
        fidelity.metadata["f3d:Design:body-recipe#0.context_id_offset"],
        "150"
    );
    assert_eq!(
        fidelity.metadata["f3d:Design:body-recipe#0.references.0.form_offset"],
        "33"
    );
    let retained = fidelity
        .retained_record("f3d:Design:body-recipe#0:selector-tail")
        .unwrap();
    assert_eq!(retained.data(), Some(&[7, 0, 0, 0][..]));
    assert_eq!(retained.offset(), 220);
    assert!(
        fidelity
            .unrepresented()
            .contains(&cadmpeg_ir::document::ArenaClass::Design)
    );
    let mut fixed = source_draft();
    fixed.class_tag = "367".try_into().unwrap();
    fixed.selector_tail.as_mut().unwrap().value = [1, 0, 0, 0];
    let mut fixed_fidelity = cadmpeg_ir::SourceFidelity::default();
    DesignBodyRecipeOperand::try_new(fixed)
        .unwrap()
        .retain_source(&mut fixed_fidelity)
        .unwrap();
    assert!(fixed_fidelity.unrepresented().is_empty());
    assert_eq!(fixed_fidelity.retained_records().len(), 1);
}

#[test]
fn body_recipe_semantic_wire_rejects_broken_identities_and_chain() {
    let wire = serde_json::to_value(source_record()).unwrap();
    for field in ["nested_record_index", "next_record_index"] {
        let mut invalid = wire.clone();
        invalid[field] = 0.into();
        assert!(
            serde_json::from_value::<DesignBodyRecipeOperand>(invalid).is_err(),
            "{field}"
        );
    }
    let mut zero_reference = wire.clone();
    zero_reference["references"][0]["design_reference"] = 0.into();
    assert!(serde_json::from_value::<DesignBodyRecipeOperand>(zero_reference).is_err());
    let mut overflow = wire.clone();
    overflow["record_index"] = u32::MAX.into();
    assert!(serde_json::from_value::<DesignBodyRecipeOperand>(overflow).is_err());
    let mut bad_length = wire.clone();
    bad_length["selector_tail"]["length"] = 3.into();
    assert!(serde_json::from_value::<DesignBodyRecipeOperand>(bad_length).is_err());
    for field in ["design_reference_offset", "form_offset"] {
        let mut invalid = wire.clone();
        invalid["references"][0][field] = 0.into();
        assert!(serde_json::from_value::<DesignBodyRecipeOperand>(invalid).is_err());
    }
    for field in ["asset_id", "context_id"] {
        let mut invalid = wire.clone();
        invalid[field] = "not-a-guid".into();
        assert!(serde_json::from_value::<DesignBodyRecipeOperand>(invalid).is_err());
    }
    let mut absent_tail = wire;
    absent_tail.as_object_mut().unwrap().remove("selector_tail");
    let restored: DesignBodyRecipeOperand = serde_json::from_value(absent_tail).unwrap();
    assert!(restored.selector_tail.is_none());
    let mut null_tail = serde_json::to_value(restored).unwrap();
    null_tail["selector_tail"] = Value::Null;
    assert!(serde_json::from_value::<DesignBodyRecipeOperand>(null_tail).is_err());
}

#[test]
fn body_recipe_source_admission_checks_real_frame_positions() {
    let source = source_record();
    assert_eq!(source.clone().into_draft().unwrap(), source_draft());
    for mutate in [
        |draft: &mut DesignBodyRecipeOperandDraft| draft.nested_record_index_offset = 0,
        |draft: &mut DesignBodyRecipeOperandDraft| draft.asset_id_offset = 0,
        |draft: &mut DesignBodyRecipeOperandDraft| draft.context_id_offset = 0,
        |draft: &mut DesignBodyRecipeOperandDraft| draft.next_byte_offset = 0,
        |draft: &mut DesignBodyRecipeOperandDraft| {
            draft.references[0].design_reference_offset = Some(0)
        },
        |draft: &mut DesignBodyRecipeOperandDraft| draft.references[0].form_offset = None,
        |draft: &mut DesignBodyRecipeOperandDraft| draft.references.clear(),
        |draft: &mut DesignBodyRecipeOperandDraft| {
            draft.selector_tail.as_mut().unwrap().offset = 239
        },
        |draft: &mut DesignBodyRecipeOperandDraft| draft.record_index = u32::MAX,
    ] {
        let mut invalid = source_draft();
        mutate(&mut invalid);
        assert!(DesignBodyRecipeOperand::try_new(invalid).is_err());
    }
}
