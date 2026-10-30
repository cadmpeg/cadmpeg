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
                second_side: None,
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

#[test]
fn revolve_projects_design_geometry_origin_axis_selections() {
    use crate::records::feature::path_features::{
        DesignPathFeatureConstruction, DesignRevolveConstruction,
    };
    use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
    use crate::records::topology::construction::DesignConstructionOperandGroup;
    use crate::records::topology::entity_selection::DesignEntitySelectionOperand;
    use crate::records::topology::extrude_selection::DesignOperandRole;

    let mut scope =
        DesignParameterScope::empty("f3d:test:scope#12", DesignFeatureKind::Revolve, 12);
    scope
        .try_edit(|draft| {
            draft.payload = DesignPathFeatureConstruction::Revolve(DesignRevolveConstruction {
                operation: crate::records::feature::extrude::DesignExtrudeOperation::NewBody,
                operation_offset: 0,
                angle: cadmpeg_ir::scalar::PositiveAngle::new(std::f64::consts::TAU).unwrap(),
                angle_record_index: 300,
                angle_offset: 0,
                second_side: None,
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
        group(101, 2, 201, DesignOperandRole::ROLE_0X21),
    ];
    let selection = |primary_identity: u64| {
        serde_json::from_value::<DesignEntitySelectionOperand>(serde_json::json!({
            "id": "f3d:test:entity-selection#201", "scope_record_index": 12,
            "group_record_index": 101, "group_member_ordinal": 0,
            "record_index": 201, "byte_offset": 100, "class_tag": "307",
            "asset_id": "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d", "asset_id_offset": 130,
            "context_id": "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e", "context_id_offset": 210,
            "identity_record_index": 204, "identity_record_offset": 300,
            "primary_identity": primary_identity, "primary_identity_offset": 321,
            "next_record_index": 205, "next_byte_offset": 329
        }))
        .unwrap()
    };
    let project = |selection: &DesignEntitySelectionOperand| {
        super::super::project_fixed_revolve_with_entities(
            &scope,
            &groups,
            &[],
            std::slice::from_ref(selection),
            &[],
            (&[], &[]),
        )
    };
    for (primary_identity, direction) in [
        (39, cadmpeg_ir::units::UnitVector3::X_AXIS),
        (40, cadmpeg_ir::units::UnitVector3::Y_AXIS),
        (41, cadmpeg_ir::units::UnitVector3::Z_AXIS),
    ] {
        let Some(cadmpeg_ir::features::FeatureDefinition::Operation(
            cadmpeg_ir::features::FeatureOperation::Revolve { construction, .. },
        )) = project(&selection(primary_identity))
        else {
            panic!("expected a revolve about origin axis {primary_identity}");
        };
        let axis = construction.axis().expect("origin axis");
        assert_eq!(axis.origin, cadmpeg_ir::features::FinitePoint3::ZERO);
        assert_eq!(
            axis.direction,
            cadmpeg_ir::features::FeatureDirection3::from(direction)
        );
    }
    assert_eq!(project(&selection(38)), None);
    assert_eq!(project(&selection(45)), None);

    // Fusion's symmetric angle applies to each side; the neutral extent
    // states the total. Two Sides keeps the revolution-direction angle first.
    let angle = |radians| cadmpeg_ir::features::AngularTermination::Angle {
        angle: cadmpeg_ir::scalar::PositiveAngle::new(radians).unwrap(),
    };
    for (symmetric, opposite, extent) in [
        (
            true,
            std::f64::consts::FRAC_PI_2,
            cadmpeg_ir::features::RevolveExtent::Symmetric {
                termination: angle(std::f64::consts::PI),
            },
        ),
        (
            false,
            std::f64::consts::FRAC_PI_6,
            cadmpeg_ir::features::RevolveExtent::TwoSided {
                first: angle(std::f64::consts::FRAC_PI_2),
                second: angle(std::f64::consts::FRAC_PI_6),
            },
        ),
    ] {
        let mut two_sided = scope.clone();
        two_sided
            .try_edit(|draft| {
                draft.payload = DesignPathFeatureConstruction::Revolve(DesignRevolveConstruction {
                    operation: crate::records::feature::extrude::DesignExtrudeOperation::NewBody,
                    operation_offset: 0,
                    angle: cadmpeg_ir::scalar::PositiveAngle::new(std::f64::consts::FRAC_PI_2)
                        .unwrap(),
                    angle_record_index: 300,
                    angle_offset: 0,
                    opposite_angle: None,
                    second_side: Some(
                        crate::records::feature::path_features::DesignRevolveSecondSide {
                            symmetric,
                            angle: cadmpeg_ir::scalar::PositiveAngle::new(opposite).unwrap(),
                            angle_record_index: 301,
                            angle_offset: 0,
                        },
                    ),
                })
                .into();
            })
            .unwrap();
        let Some(cadmpeg_ir::features::FeatureDefinition::Operation(
            cadmpeg_ir::features::FeatureOperation::Revolve {
                construction:
                    cadmpeg_ir::features::RevolveConstruction::Resolved {
                        extent: projected, ..
                    },
                ..
            },
        )) = super::super::project_fixed_revolve_with_entities(
            &two_sided,
            &groups,
            &[],
            std::slice::from_ref(&selection(41)),
            &[],
            (&[], &[]),
        )
        else {
            panic!("expected a resolved revolve");
        };
        assert_eq!(projected, extent);
    }
}

#[test]
fn revolve_axes_turn_the_profile_toward_the_sketch_normal() {
    use cadmpeg_ir::features::{
        AngularTermination, BooleanOp, Feature, FeatureDefinition, FeatureDirection3, FeatureId,
        FeatureOperation, FinitePoint3, PlanarProfileRef, RevolutionAxis, RevolveConstruction,
        RevolveExtent,
    };
    use cadmpeg_ir::math::{Point2, Point3, Vector3};
    use cadmpeg_ir::sketches::{
        Sketch, SketchEntity, SketchEntityId, SketchEntityUse, SketchGeometry,
        SketchGeometryDefinition, SketchId, SketchPlacement,
    };
    use cadmpeg_ir::units::UnitVector3;

    let sketch_id = SketchId::mint("generated:test:sketch#1").unwrap();
    // A profile edge on the XY plane, with the sketch normal along +Z.
    let oriented = |start: [f64; 2], end: [f64; 2], axis_direction: UnitVector3| {
        let entity = SketchEntity::new(
            SketchEntityId::mint("generated:test:sketch-entity#1").unwrap(),
            sketch_id.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(start[0], start[1]),
                end: Point2::new(end[0], end[1]),
            })
            .unwrap(),
        );
        let sketch = Sketch {
            id: sketch_id.clone(),
            name: None,
            configuration: None,
            visible: None,
            placement: SketchPlacement::try_resolved(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
            profiles: vec![vec![SketchEntityUse {
                entity: entity.id().clone(),
                reversed: false,
            }]]
            .try_into()
            .unwrap(),
            native_ref: None,
        };
        let mut features = [Feature {
            id: FeatureId::mint("synthetic:test:id#f3d:feature:revolve").expect("identity grammar"),
            ordinal: 0,
            name: None,
            suppressed: None,
            dependencies: Default::default(),
            source_properties: Default::default(),
            source_tag: None,
            source_text: None,
            source_content: Default::default(),
            evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
                FeatureDefinition::Operation(FeatureOperation::Revolve {
                    construction: RevolveConstruction::Resolved {
                        profile: PlanarProfileRef::Sketch(sketch_id.clone()),
                        axis: RevolutionAxis {
                            origin: FinitePoint3::ZERO,
                            direction: FeatureDirection3::from(axis_direction),
                            reference: None,
                        },
                        extent: RevolveExtent::OneSided {
                            termination: AngularTermination::Angle {
                                angle: cadmpeg_ir::scalar::PositiveAngle::QUARTER_TURN,
                            },
                        },
                        solid: None,
                        face_maker: None,
                        fuse_order: None,
                        allow_multi_profile_faces: None,
                    },
                    op: BooleanOp::NewBody,
                }),
            ),
            native_ref: None,
        }];
        super::super::orient_revolve_axes(&mut features, &[sketch], &[entity], (1.0e-6, 1.0e-9));
        let FeatureDefinition::Operation(FeatureOperation::Revolve { construction, .. }) =
            features[0].evaluation.definition()
        else {
            panic!("expected a revolve");
        };
        construction.axis().map(|axis| axis.direction)
    };
    let y = FeatureDirection3::from(UnitVector3::Y_AXIS);
    let minus_y = FeatureDirection3::from(UnitVector3::Y_AXIS.reversed());
    // Turning +X toward +Z is right-handed about -Y, whichever way the Y axis
    // or a sketch line is stored; turning -X toward +Z is right-handed about +Y.
    assert_eq!(
        oriented([10.0, 0.0], [30.0, 20.0], UnitVector3::Y_AXIS),
        Some(minus_y)
    );
    assert_eq!(
        oriented([10.0, 0.0], [30.0, 20.0], UnitVector3::Y_AXIS.reversed()),
        Some(minus_y)
    );
    assert_eq!(
        oriented([-30.0, 0.0], [-10.0, 20.0], UnitVector3::Y_AXIS),
        Some(y)
    );
    // A profile edge may touch the axis.
    assert_eq!(
        oriented([0.0, 0.0], [20.0, 20.0], UnitVector3::Y_AXIS),
        Some(minus_y)
    );
    // A profile across the axis, or an axis along the sketch normal, has no side.
    assert_eq!(
        oriented([-10.0, 0.0], [10.0, 20.0], UnitVector3::Y_AXIS),
        None
    );
    assert_eq!(
        oriented([10.0, 0.0], [30.0, 20.0], UnitVector3::Z_AXIS),
        None
    );
}
