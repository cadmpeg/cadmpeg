// SPDX-License-Identifier: Apache-2.0

use crate::records::SketchRelation;

#[test]
fn sketch_relation_runs_preserve_wire_and_reject_conflicting_resolved_indices() {
    let base = r#"{"id":"relation","record_index":1,"class_tag":"000","byte_offset":0,"state_offset":0,"owner_reference":1,"owner_entity_id":"owner","auxiliary_references":[],"auxiliary_reference_offsets":[],"members":[1,2],"resolved_members":[],"member_offsets":[25,40],"owner_reference_offset":0,"state":0,"constraint_kinds":["coincident"],"unknown_constraint_bits":0,"member_relation_ordinals":[3,5],"entity_genesis":null,"pattern":null,"return_members":[2,1],"resolved_return_members":[],"return_member_offsets":[60,75],"raw_bytes":""}"#;
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
    let mut partial_ordinals = relation.members.to_vec();
    partial_ordinals[0].relation_ordinal = None;
    assert!(crate::records::SketchRelationMembers::try_from(partial_ordinals).is_err());
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
