// SPDX-License-Identifier: Apache-2.0
//! Edge operand wire: the resolved axis is whole or absent.

#[test]
fn edge_operand_wire_rejects_partial_resolved_axis() {
    let prefix = r#"{"id":"edge","scope_record_index":1,"scope_reference_ordinal":0,"record_index":2,"byte_offset":10,"class_tag":"346","paired_byte_offset":20,"paired_class_tag":"262","recipe_record_index":5,"recipe_record_byte_offset":30,"recipe_id":"recipe","recipe_prefix_offset":41,"recipe_prefix_bytes":"","recipe_references":[],"recipe_program_offset":50,"recipe_program":[]"#;
    let suffix = r#","next_record_index":4,"next_byte_offset":100}"#;
    let origin = serde_json::to_string(&cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)).unwrap();
    let direction = serde_json::to_string(&cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0)).unwrap();
    for fields in [
        String::new(),
        format!(",\"resolved_axis_origin\":{origin},\"resolved_axis_direction\":{direction}"),
    ] {
        let wire = format!("{prefix}{fields}{suffix}");
        let operand: super::DesignEdgeOperand = serde_json::from_str(&wire).unwrap();
        assert_eq!(serde_json::to_string(&operand).unwrap(), wire);
    }
    for (field, value) in [
        ("resolved_axis_origin", origin),
        ("resolved_axis_direction", direction),
    ] {
        let invalid = format!("{prefix},\"{field}\":{value}{suffix}");
        let error = serde_json::from_str::<super::DesignEdgeOperand>(&invalid)
            .unwrap_err()
            .to_string();
        assert!(error.contains("resolved_axis_origin"));
        assert!(error.contains("resolved_axis_direction"));
    }
}
