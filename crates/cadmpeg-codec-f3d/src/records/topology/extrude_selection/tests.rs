// SPDX-License-Identifier: Apache-2.0
//! Extrude selection groups: member runs, scalars and derived offsets.

#[test]
fn extrude_selection_group_members_preserve_wire_and_reject_unequal_offsets() {
    let wire = r#"{"id":"group","scope_record_index":7,"scope_reference_ordinal":0,"record_index":9,"byte_offset":0,"class_tag":"277","member_count_offset":32,"members":[10,11],"member_offsets":[37,48],"opaque_index":1,"opaque_index_offset":58,"opaque_scalar":0.0,"opaque_scalar_offset":62,"variant":false,"paired_class_tag":"259","paired_byte_offset":111}"#;
    let group: super::DesignExtrudeSelectionGroup =
        serde_json::from_str(wire).expect("selection group");
    assert_eq!(serde_json::to_string(&group).expect("selection wire"), wire);
    for offsets in ["[]", "[37]", "[37,48,59]"] {
        let invalid = wire.replace(
            "\"member_offsets\":[37,48]",
            &format!("\"member_offsets\":{offsets}"),
        );
        let error = serde_json::from_str::<super::DesignExtrudeSelectionGroup>(&invalid)
            .expect_err("unequal member arrays")
            .to_string();
        assert!(error.contains("members"));
        assert!(error.contains("member_offsets"));
    }
}

#[test]
fn extrude_group_rejects_invalid_run_and_scalar_admission() {
    let wire = serde_json::json!({"id": "group", "scope_record_index": 7, "scope_reference_ordinal": 0,
        "record_index": 9, "byte_offset": 0, "class_tag": "277", "member_count_offset": 32,
        "members": [10, 11], "member_offsets": [37, 48], "opaque_index": 1,
        "opaque_index_offset": 58, "opaque_scalar": -1.0, "opaque_scalar_offset": 62,
        "variant": false, "paired_class_tag": "259", "paired_byte_offset": 111});
    let group: super::DesignExtrudeSelectionGroup = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(&group).unwrap(), wire);
    for (field, value) in [
        ("members", serde_json::json!([10, 10])),
        ("member_offsets", serde_json::json!([37, 49])),
        ("opaque_index", serde_json::json!(0)),
        ("member_count_offset", serde_json::json!(33)),
        ("opaque_index_offset", serde_json::json!(59)),
        ("opaque_scalar_offset", serde_json::json!(63)),
        ("paired_byte_offset", serde_json::json!(112)),
        ("byte_offset", serde_json::json!(u64::MAX)),
    ] {
        let mut invalid = wire.clone();
        invalid[field] = value;
        assert!(
            serde_json::from_value::<super::DesignExtrudeSelectionGroup>(invalid).is_err(),
            "{field}"
        );
    }
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut invalid = super::DesignExtrudeSelectionGroupWire::from(group.clone());
        invalid.opaque_scalar = value;
        assert!(super::DesignExtrudeSelectionGroup::try_from(invalid).is_err());
    }
    for members in [vec![], vec![10, 10]] {
        let mut changed = group.clone();
        assert!(changed.try_set_members(members).is_err());
        assert_eq!(changed, group);
    }
}
