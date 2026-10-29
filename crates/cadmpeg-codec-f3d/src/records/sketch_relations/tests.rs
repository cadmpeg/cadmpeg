// SPDX-License-Identifier: Apache-2.0

mod constraint_state;

use crate::records::sketch_relations::SketchRelation;

#[test]
fn native_rectangular_pattern_refuses_nonunit_direction() {
    let value = serde_json::json!({
        "evaluated_count": 2,
        "count_parameter": 3,
        "direction": [2.0, 0.0, 0.0],
        "evaluated_distance": 1.0,
        "distance_parameter": 4
    });
    assert!(
        serde_json::from_value::<crate::records::sketch_relations::SketchPatternDirection>(value)
            .is_err()
    );
}

const RELATION_WIRE: &str = r#"{"id":"relation","record_index":1,"class_tag":"000","owner_reference":1,"owner_entity_id":"owner","auxiliary_references":[],"members":[1,2],"resolved_members":[],"state":0,"member_relation_ordinals":[3,5],"entity_genesis":null,"pattern":null,"return_members":[2,1],"resolved_return_members":[]}"#;

#[test]
fn sketch_relation_borrowed_wire_matches_owned_wire_bytes() {
    let resolved = RELATION_WIRE.replace(
        r#""resolved_members":[]"#,
        r#""resolved_members":[{"kind":"point","record_index":1,"persistent_id":10},{"kind":"record","record_index":2}]"#,
    ).replace(
        r#""resolved_return_members":[]"#,
        r#""resolved_return_members":[{"kind":"record","record_index":2},{"kind":"point","record_index":1,"persistent_id":10}]"#,
    );
    for wire in [RELATION_WIRE, resolved.as_str()] {
        let relation: SketchRelation = serde_json::from_str(wire).unwrap();
        let owned = super::SketchRelationSerde::from(relation.clone());
        assert_eq!(
            serde_json::to_vec(&relation).unwrap(),
            serde_json::to_vec(&owned).unwrap()
        );
    }
}

#[test]
fn sketch_relation_native_retained_limit_refuses_before_clone() {
    let mut relation: SketchRelation = serde_json::from_str(RELATION_WIRE).unwrap();
    relation.id = "f3d:native:sketch-relation#0".into();
    crate::test_support::native_test::assert_borrowed_native_retained_limit(
        &relation,
        "sketch_relations",
        || super::SKETCH_RELATION_CLONE_COUNT.with(|count| count.set(0)),
        || super::SKETCH_RELATION_CLONE_COUNT.with(std::cell::Cell::get),
    );
}

#[test]
fn sketch_relation_runs_preserve_wire_and_reject_conflicting_resolved_indices() {
    let base = RELATION_WIRE;
    let resolved = base.replace(r#""resolved_members":[]"#, r#""resolved_members":[{"kind":"point","record_index":1,"persistent_id":10},{"kind":"record","record_index":2}]"#)
        .replace(r#""resolved_return_members":[]"#, r#""resolved_return_members":[{"kind":"record","record_index":2},{"kind":"point","record_index":1,"persistent_id":10}]"#);
    for wire in [base, resolved.as_str()] {
        let relation: SketchRelation = serde_json::from_str(wire).unwrap();
        assert_eq!(serde_json::to_string(&relation).unwrap(), wire);
        assert_eq!(relation.member_indices(), [1, 2]);
        assert_eq!(relation.return_member_indices(), [2, 1]);
    }
    for ordinals in ["[]", "[0,0]"] {
        let wire = base.replace("[3,5]", ordinals);
        let relation: SketchRelation = serde_json::from_str(&wire).unwrap();
        assert_eq!(serde_json::to_string(&relation).unwrap(), wire);
    }
    let relation: SketchRelation = serde_json::from_str(base).unwrap();
    let mut partial_ordinals = relation.members().to_vec();
    partial_ordinals[0].relation_ordinal = None;
    assert!(
        crate::records::sketch_relations::SketchRelationMembers::try_from(partial_ordinals)
            .is_err()
    );
    for field in ["resolved_members", "resolved_return_members"] {
        let value: serde_json::Value = serde_json::from_str(&resolved).unwrap();
        let mut mismatch = value.clone();
        mismatch[field][0]["record_index"] = serde_json::json!(3);
        assert!(serde_json::from_value::<SketchRelation>(mismatch)
            .unwrap_err()
            .to_string()
            .contains(field));
        let mut partial = value;
        partial[field].as_array_mut().unwrap().pop();
        assert!(serde_json::from_value::<SketchRelation>(partial)
            .unwrap_err()
            .to_string()
            .contains(field));
    }
}

#[test]
fn sketch_relation_owner_preserves_absence_and_nonempty_ids_on_wire_round_trip() {
    assert!(cadmpeg_core::text::NonBlankString::new("").is_none());
    for (owner, field) in [
        (None, r#""owner_entity_id":"""#),
        (Some("owner"), r#""owner_entity_id":"owner""#),
        (Some(" owner "), r#""owner_entity_id":" owner ""#),
    ] {
        let mut relation: SketchRelation = serde_json::from_str(RELATION_WIRE).unwrap();
        relation.owner_entity_id =
            owner.map(|id| cadmpeg_core::text::NonBlankString::new(id).unwrap());
        let expected = RELATION_WIRE.replace(r#""owner_entity_id":"owner""#, field);
        let wire = serde_json::to_string(&relation).unwrap();
        assert_eq!(wire, expected);
        assert_eq!(
            serde_json::from_str::<SketchRelation>(&wire).unwrap(),
            relation
        );
    }
}

fn source_relation() -> SketchRelation {
    let content: SketchRelation = serde_json::from_str(RELATION_WIRE).unwrap();
    let members = content
        .members
        .0
        .into_iter()
        .zip([25, 40])
        .map(|(mut row, offset)| {
            row.offset = Some(offset);
            row
        })
        .collect::<Vec<_>>()
        .try_into()
        .unwrap();
    let return_members = content
        .return_members
        .0
        .into_iter()
        .zip([60, 75])
        .map(|(mut row, offset)| {
            row.offset = Some(offset);
            row
        })
        .collect::<Vec<_>>()
        .try_into()
        .unwrap();
    SketchRelation::try_new(super::SketchRelationDraft {
        id: content.id,
        record_index: content.record_index,
        class_tag: content.class_tag,
        byte_offset: 0,
        state_offset: 0,
        owner_reference: content.owner_reference,
        owner_entity_id: content.owner_entity_id,
        auxiliary_references: super::ReferenceRun::located(Vec::new()),
        rectangular_counted_reference_count: None,
        members,
        owner_reference_offset: 0,
        definition: content.definition,
        entity_genesis: content.entity_genesis,
        return_members,
        raw_bytes: vec![0; 80],
    })
    .unwrap()
}
#[test]
fn source_admission_bounds_every_reference_and_preserves_failed_edits() {
    let relation = source_relation();
    for offset in [77, u32::MAX] {
        for field in [
            "member_offsets",
            "auxiliary_reference_offsets",
            "owner_reference_offset",
            "return_member_offsets",
        ] {
            let mut edited = relation.clone();
            let result = edited.try_edit(|draft| match field {
                "member_offsets" => draft.members.0[0].offset = Some(offset),
                "auxiliary_reference_offsets" => {
                    draft.auxiliary_references =
                        super::ReferenceRun::located(vec![super::Located { value: 1, offset }]);
                }
                "owner_reference_offset" => draft.owner_reference_offset = offset,
                _ => draft.return_members.0[0].offset = Some(offset),
            });
            assert!(result.unwrap_err().to_string().contains(field));
            assert_eq!(edited, relation);
        }
    }
    let mut edited = relation.clone();
    assert!(edited
        .try_edit(|draft| draft.raw_bytes.truncate(23))
        .is_err());
    assert_eq!(edited, relation);
    assert!(edited
        .try_edit(|draft| draft.auxiliary_references = super::ReferenceRun::unlocated(vec![1]))
        .is_err());
    assert_eq!(edited, relation);
    edited
        .try_edit(|draft| draft.owner_reference_offset = 76)
        .unwrap();
    assert_eq!(edited.owner_reference_offset(), Some(76));
}
