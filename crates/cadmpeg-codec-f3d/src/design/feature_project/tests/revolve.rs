// SPDX-License-Identifier: Apache-2.0

#[test]
fn revolve_projects_with_native_profile() {
    use crate::records::feature::path_features::{
        DesignPathFeatureConstruction, DesignRevolveConstruction,
    };
    use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
    use crate::records::topology::construction::DesignConstructionOperandGroup;
    use crate::records::topology::edge_identity::DesignEdgeOperand;
    use crate::records::topology::extrude_selection::DesignOperandRole;

    let mut scope =
        DesignParameterScope::empty("f3d:test:scope#12", DesignFeatureKind::Revolve, 12);
    scope
        .try_edit(|draft| {
            draft.payload = DesignPathFeatureConstruction::Revolve(DesignRevolveConstruction {
                operation: crate::records::feature::extrude::DesignExtrudeOperation::NewBody,
                operation_offset: 0,
                angle: cadmpeg_ir::scalar::PositiveAngle::new(1.0).unwrap(),
                angle_record_index: 300,
                angle_offset: 0,
                opposite_angle: None,
            })
            .into();
        })
        .unwrap();
    let group = |record_index, ordinal, member, role: DesignOperandRole| {
        serde_json::from_value::<DesignConstructionOperandGroup>(serde_json::json!({
            "id": format!("f3d:test:group#{record_index}"),
            "scope_record_index": 12,
            "scope_reference_ordinal": ordinal,
            "record_index": record_index,
            "byte_offset": 1000,
            "class_tag": "332",
            "members": [member],
            "member_offsets": [1026],
            "frame": {
                "member_count_offset": 1021,
                "opaque_index": 1,
                "opaque_index_offset": 1072,
                "opaque_scalar": 0.0,
                "opaque_scalar_offset": 1076,
                "variant": false
            },
            "role": role.raw(),
            "role_offset": 1054,
            "paired_class_tag": "259",
            "paired_byte_offset": 1125
        }))
        .unwrap()
    };
    let groups = [
        group(100, 0, 200, DesignOperandRole::PROFILE),
        group(101, 1, 201, DesignOperandRole::ROLE_0X21),
    ];
    let axis: DesignEdgeOperand = serde_json::from_value(serde_json::json!({
        "id": "f3d:test:edge-operand#201",
        "scope_record_index": 12,
        "scope_reference_ordinal": 1,
        "record_index": 201,
        "byte_offset": 0,
        "class_tag": "376",
        "paired_byte_offset": 16,
        "paired_class_tag": "260",
        "recipe_record_index": 204,
        "recipe_record_byte_offset": 32,
        "recipe_id": "f3d:test:recipe#204",
        "recipe_prefix_offset": 43,
        "recipe_prefix_bytes": "",
        "recipe_references": [],
        "recipe_program_offset": 0,
        "recipe_program": [-1, -1, 2],
        "resolved_axis_origin": [0.0, 0.0, 0.0],
        "resolved_axis_direction": [0.0, 0.0, 1.0],
        "next_record_index": 205,
        "next_byte_offset": 160
    }))
    .unwrap();
    let Some(cadmpeg_ir::features::FeatureDefinition::Operation(
        cadmpeg_ir::features::FeatureOperation::Revolve { construction, .. },
    )) = super::super::project_fixed_revolve_with_entities(
        &scope,
        &groups,
        std::slice::from_ref(&axis),
        &[],
        &[],
        (&[], &[]),
    )
    else {
        panic!("expected a revolve");
    };
    assert_eq!(
        construction.profile(),
        Some(&cadmpeg_ir::features::PlanarProfileRef::Native(
            groups[0].id.clone()
        ))
    );
}
