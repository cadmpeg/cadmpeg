// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::geometry::SurfaceGeometry;
use cadmpeg_ir::math::Vector3;

use super::*;

#[test]
fn nx_boolean_keeps_body_namespace_proofs_atomic() {
    use cadmpeg_ir::features::{BodySelection, BooleanKind, FeatureDefinition};
    use cadmpeg_ir::ids::BodyId;
    use std::collections::BTreeMap;

    let operation = crate::native::features::FeatureBooleanOperation {
        id: "boolean#mixed-namespaces".to_string(),
        operation_label: "operation#mixed-namespaces".to_string(),
        kind: crate::native::features::FeatureBooleanKind::Subtract,
        target: crate::test_support::native_references::boolean_reference(94, 0),
        tools: vec![crate::test_support::native_references::boolean_reference(
            122, 1,
        )],
        source_offset: 0,
    };
    let body = BodyId::mint("nx:s18:body#3".to_string()).expect("identity grammar");
    let blocks = BTreeMap::from([(122, "nx:om-data-blocks-3:block#122".to_string())]);

    assert_eq!(
        super::boolean_feature_definition(
            &operation,
            &BTreeMap::from([(94, 94)]),
            &BooleanOffsetStoreResolution::Complete(blocks.clone()),
            &BTreeMap::from([(94, vec![body])]),
        )
        .unwrap(),
        FeatureDefinition::Combine {
            operands: cadmpeg_ir::features::CombineOperands::new(
                BodySelection::Native("nx:om-object-index#94".to_string()),
                BodySelection::Native("nx:om-object-indices#122".to_string())
            )
            .unwrap(),

            op: BooleanKind::Cut,
            keep_tools: false,
        }
    );
    assert_eq!(
        super::boolean_feature_definition(
            &operation,
            &BTreeMap::from([(94, 94), (122, 122)]),
            &BooleanOffsetStoreResolution::Unresolved,
            &BTreeMap::from([(
                94,
                vec![BodyId::mint("nx:s18:body#3".to_string()).expect("identity grammar")]
            )]),
        )
        .unwrap(),
        FeatureDefinition::Combine {
            operands: cadmpeg_ir::features::CombineOperands::new(
                BodySelection::Native("nx:om-object-index#94".to_string()),
                BodySelection::Native("nx:om-object-indices#122".to_string())
            )
            .unwrap(),

            op: BooleanKind::Cut,
            keep_tools: false,
        }
    );

    let colliding_blocks = BTreeMap::from([
        (94, "nx:om-data-blocks-3:block#94".to_string()),
        (122, "nx:om-data-blocks-3:block#122".to_string()),
    ]);
    assert_eq!(
        super::boolean_feature_definition(
            &operation,
            &BTreeMap::from([(94, 94)]),
            &BooleanOffsetStoreResolution::Complete(colliding_blocks.clone()),
            &BTreeMap::from([(
                94,
                vec![BodyId::mint("nx:s18:body#3".to_string()).expect("identity grammar")]
            )]),
        )
        .unwrap(),
        FeatureDefinition::Combine {
            operands: cadmpeg_ir::features::CombineOperands::new(
                BodySelection::local(
                    vec!["nx:om-data-blocks-3:block#94".to_string()],
                    "nx:om-object-index#94".to_string()
                )
                .unwrap(),
                BodySelection::local(
                    vec!["nx:om-data-blocks-3:block#122".to_string()],
                    "nx:om-object-indices#122".to_string()
                )
                .unwrap()
            )
            .unwrap(),

            op: BooleanKind::Cut,
            keep_tools: false,
        }
    );

    let mixed_store_blocks = BTreeMap::from([
        (401, "nx:om-data-blocks-3:block#401".to_string()),
        (402, "nx:om-data-blocks-4:block#402".to_string()),
        (403, "nx:om-data-blocks-4:block#403".to_string()),
    ]);
    let mixed_store_operation = crate::native::features::FeatureBooleanOperation {
        id: "boolean#mixed-stores".to_string(),
        operation_label: "operation#mixed-stores".to_string(),
        kind: crate::native::features::FeatureBooleanKind::Unite,
        target: crate::test_support::native_references::boolean_reference(401, 0),
        tools: vec![
            crate::test_support::native_references::boolean_reference(402, 1),
            crate::test_support::native_references::boolean_reference(403, 2),
        ],
        source_offset: 0,
    };
    assert_eq!(
        super::boolean_feature_definition(
            &mixed_store_operation,
            &BTreeMap::new(),
            &BooleanOffsetStoreResolution::Complete(mixed_store_blocks.clone()),
            &BTreeMap::new(),
        )
        .unwrap(),
        FeatureDefinition::Combine {
            operands: cadmpeg_ir::features::CombineOperands::new(
                BodySelection::Native("nx:om-object-index#401".to_string()),
                BodySelection::Native("nx:om-object-indices#402,403".to_string())
            )
            .unwrap(),

            op: BooleanKind::Join,
            keep_tools: false,
        }
    );
}

#[test]
fn nx_sew_projects_ordered_body_operands_without_inventing_tolerance() {
    use cadmpeg_ir::features::{BodySelection, FeatureDefinition};
    use cadmpeg_ir::ids::BodyId;
    use std::collections::BTreeMap;

    let operand = |ordinal, object_index| crate::native::features::FeatureOperationBodyOperand {
        id: format!("operand#{ordinal}"),
        operation_label: "operation#0".to_string(),
        body_object_index: 10,
        body_reference_ordinal: 0,
        ordinal,
        operand: crate::om::compact::LocatedCompactIndex {
            atom: crate::om::compact::CompactIndexAtom::from_wire(
                object_index,
                &[object_index as u8],
            )
            .unwrap(),
            offset: u64::from(ordinal),
        },
        operand_data_block: None,
        segment_body_bindings: vec![format!("binding#{ordinal}")],
    };
    let operands = [operand(0, 20), operand(1, 30)];
    let references = operands.iter().collect::<Vec<_>>();
    let roots = BTreeMap::from([(10, 10), (20, 20), (30, 30)]);

    assert_eq!(
        super::sew_body_feature_definition(Some(10), &[], &references, &roots, &BTreeMap::new(),),
        Some(FeatureDefinition::SewBodies {
            bodies: (BodySelection::local(
                vec![
                    "nx:om-body-object#10".to_string(),
                    "nx:om-body-object#20".to_string(),
                    "nx:om-body-object#30".to_string(),
                ],
                "nx:om-object-indices#10,20,30".to_string()
            )
            .unwrap())
            .try_into()
            .unwrap(),
            gap_tolerance: None,
        })
    );
    assert!(matches!(
    super::sew_body_feature_definition(
        Some(736),
        &[],
        &references,
        &roots,
        &BTreeMap::new(),
    ), Some(FeatureDefinition::SewBodies {
        bodies,
        ..
    }) if matches!((bodies.as_ref(),), (BodySelection::Local { bodies, .. },) if bodies.as_slice() == [
        "nx:om-body-object#736",
        "nx:om-body-object#20",
        "nx:om-body-object#30",
    ])));
    let resolved = BTreeMap::from([
        (
            10,
            vec![BodyId::mint("test:model:entity#target".to_string()).expect("identity grammar")],
        ),
        (
            20,
            vec![
                BodyId::mint("test:model:entity#first-tool".to_string()).expect("identity grammar")
            ],
        ),
        (
            30,
            vec![BodyId::mint("test:model:entity#second-tool".to_string())
                .expect("identity grammar")],
        ),
    ]);
    assert_eq!(
        super::sew_body_feature_definition(Some(10), &[], &references, &roots, &resolved,),
        Some(FeatureDefinition::SewBodies {
            bodies: (BodySelection::Resolved {
                bodies: vec![
                    BodyId::mint("test:model:entity#target".to_string()).expect("identity grammar"),
                    BodyId::mint("test:model:entity#first-tool".to_string())
                        .expect("identity grammar"),
                    BodyId::mint("test:model:entity#second-tool".to_string())
                        .expect("identity grammar"),
                ],
                native: "nx:om-object-indices#10,20,30".to_string(),
            })
            .try_into()
            .unwrap(),
            gap_tolerance: None,
        })
    );
    assert_eq!(
        super::sew_body_feature_definition(Some(10), &[], &[], &roots, &BTreeMap::new(),),
        None
    );

    let alias_roots = BTreeMap::from([(10, 10), (20, 20), (30, 20)]);
    assert_eq!(
        super::sew_body_feature_definition(
            Some(10),
            &[],
            &references,
            &alias_roots,
            &BTreeMap::new(),
        ),
        Some(FeatureDefinition::SewBodies {
            bodies: (BodySelection::local(
                vec![
                    "nx:om-body-object#10".to_string(),
                    "nx:om-body-object#20".to_string(),
                ],
                "nx:om-object-indices#10,20,30".to_string()
            )
            .unwrap())
            .try_into()
            .unwrap(),
            gap_tolerance: None,
        })
    );

    let offset_operand = |ordinal: u32, object_index: u32, data_block: &str| {
        crate::native::features::FeatureOperationBodyOperand {
            id: format!("offset-operand#{ordinal}"),
            operation_label: "operation#0".to_string(),
            body_object_index: 72,
            body_reference_ordinal: 0,
            ordinal,
            operand: crate::om::compact::LocatedCompactIndex {
                atom: crate::om::compact::CompactIndexAtom::from_wire(
                    object_index,
                    &[object_index as u8],
                )
                .unwrap(),
                offset: u64::from(ordinal),
            },
            operand_data_block: Some(data_block.to_string()),
            segment_body_bindings: Vec::new(),
        }
    };
    let offset_operands = [
        offset_operand(0, 71, "nx:om-data-blocks-4:block#71"),
        offset_operand(1, 70, "nx:om-data-blocks-4:block#70"),
    ];
    let offset_references = offset_operands.iter().collect::<Vec<_>>();
    assert_eq!(
        super::sew_body_feature_definition(
            None,
            &[(72, "nx:om-data-blocks-4:block#72".to_string())],
            &offset_references,
            &BTreeMap::new(),
            &BTreeMap::new(),
        ),
        Some(FeatureDefinition::SewBodies {
            bodies: (BodySelection::local(
                vec![
                    "nx:om-data-blocks-4:block#72".to_string(),
                    "nx:om-data-blocks-4:block#71".to_string(),
                    "nx:om-data-blocks-4:block#70".to_string(),
                ],
                "nx:om-object-indices#72,71,70".to_string()
            )
            .unwrap())
            .try_into()
            .unwrap(),
            gap_tolerance: None,
        })
    );
    let mut mixed_operands = offset_operands.clone();
    mixed_operands[1].operand_data_block = None;
    mixed_operands[1].segment_body_bindings = vec!["segment-binding".to_string()];
    let mixed_references = mixed_operands.iter().collect::<Vec<_>>();
    assert!(matches!(
        super::sew_body_feature_definition(
            None,
            &[(72, "nx:om-data-blocks-4:block#72".to_string())],
            &mixed_references,
            &BTreeMap::new(),
            &BTreeMap::new(),
        ), Some(FeatureDefinition::SewBodies {
            bodies,
            ..
        }) if matches!((bodies.as_ref(),), (BodySelection::Native(native),) if native == "nx:om-object-indices#72,71,70")));
}

#[test]
fn nx_delete_body_requires_a_primary_body_field() {
    use cadmpeg_ir::features::{BodyRetentionMode, BodySelection, FeatureDefinition};
    use std::collections::BTreeMap;

    let roots = BTreeMap::from([(20, 20)]);
    assert_eq!(
        super::delete_body_feature_definition(
            super::DeleteBodyField::Native(20),
            &roots,
            &BTreeMap::new()
        ),
        FeatureDefinition::DeleteBody {
            bodies: BodySelection::local(
                vec!["nx:om-body-object#20".to_string()],
                "nx:om-object-index#20".to_string()
            )
            .unwrap(),
            mode: BodyRetentionMode::DeleteSelected,
        }
    );
    assert_eq!(
        super::delete_body_feature_definition(
            super::DeleteBodyField::Native(72),
            &roots,
            &BTreeMap::new()
        ),
        FeatureDefinition::DeleteBody {
            bodies: BodySelection::local(
                vec!["nx:om-body-object#72".to_string()],
                "nx:om-object-index#72".to_string()
            )
            .unwrap(),
            mode: BodyRetentionMode::DeleteSelected,
        }
    );
    assert_eq!(
        super::delete_body_feature_definition(
            super::DeleteBodyField::OffsetStore {
                object_index: 72,
                data_block: "nx:om-data-blocks-2:block#72",
            },
            &roots,
            &BTreeMap::new(),
        ),
        FeatureDefinition::DeleteBody {
            bodies: BodySelection::local(
                vec!["nx:om-data-blocks-2:block#72".to_string()],
                "nx:om-object-index#72".to_string()
            )
            .unwrap(),
            mode: BodyRetentionMode::DeleteSelected,
        }
    );
}

#[test]
fn nx_trim_body_retains_exact_input_store_target_and_tools() {
    use cadmpeg_ir::features::{BodySelection, BodyTrimSide, FeatureDefinition};

    let body = (114, "nx:om-data-blocks-2:block#114".to_string());
    let operand = crate::native::features::FeatureOperationBodyOperand {
        id: "operand#0".to_string(),
        operation_label: "operation#0".to_string(),
        body_object_index: 114,
        body_reference_ordinal: 0,
        ordinal: 0,
        operand: crate::om::compact::LocatedCompactIndex {
            atom: crate::om::compact::CompactIndexAtom::from_wire(113, &[113]).unwrap(),
            offset: 0,
        },
        operand_data_block: Some("nx:om-data-blocks-2:block#113".to_string()),
        segment_body_bindings: Vec::new(),
    };
    assert_eq!(
        super::offset_store_trim_body_feature_definition(std::slice::from_ref(&body), &[&operand],),
        Some(FeatureDefinition::TrimBodies {
            operands: cadmpeg_ir::features::TrimBodyOperands::new(
                BodySelection::local(
                    vec!["nx:om-data-blocks-2:block#114".to_string()],
                    "nx:om-object-index#114".to_string()
                )
                .unwrap(),
                BodySelection::local(
                    vec!["nx:om-data-blocks-2:block#113".to_string()],
                    "nx:om-object-indices#113".to_string()
                )
                .unwrap()
            )
            .unwrap(),

            keep: BodyTrimSide::Unresolved,
        })
    );
    assert!(super::offset_store_trim_body_feature_definition(&[], &[&operand]).is_none());
    assert!(
        super::offset_store_trim_body_feature_definition(&[body.clone(), body], &[&operand],)
            .is_none()
    );
    assert_eq!(
        super::offset_store_trim_body_feature_definition(
            std::slice::from_ref(&(114, "nx:om-data-blocks-2:block#114".to_string())),
            &[],
        ),
        Some(FeatureDefinition::TrimBodies {
            operands: cadmpeg_ir::features::TrimBodyOperands::new(
                BodySelection::local(
                    vec!["nx:om-data-blocks-2:block#114".to_string()],
                    "nx:om-object-index#114".to_string()
                )
                .unwrap(),
                BodySelection::Unresolved
            )
            .unwrap(),

            keep: BodyTrimSide::Unresolved,
        })
    );
}

#[test]
fn nx_trim_body_projects_distinct_target_and_ordered_tools() {
    use cadmpeg_ir::features::{BodySelection, BodyTrimSide, FeatureDefinition};
    use cadmpeg_ir::ids::BodyId;
    use std::collections::BTreeMap;

    let operands = [crate::native::features::FeatureOperationBodyOperand {
        id: "operand#0".to_string(),
        operation_label: "operation#0".to_string(),
        body_object_index: 10,
        body_reference_ordinal: 0,
        ordinal: 0,
        operand: crate::om::compact::LocatedCompactIndex {
            atom: crate::om::compact::CompactIndexAtom::from_wire(20, &[20]).unwrap(),
            offset: 0,
        },
        operand_data_block: None,
        segment_body_bindings: vec!["binding#0".to_string()],
    }];
    let references = operands.iter().collect::<Vec<_>>();
    let roots = BTreeMap::from([(10, 10), (20, 20)]);

    assert_eq!(
        super::trim_body_feature_definition(10, &references, &roots, &BTreeMap::new()).unwrap(),
        FeatureDefinition::TrimBodies {
            operands: cadmpeg_ir::features::TrimBodyOperands::new(
                BodySelection::local(
                    vec!["nx:om-body-object#10".to_string()],
                    "nx:om-object-index#10".to_string()
                )
                .unwrap(),
                BodySelection::local(
                    vec!["nx:om-body-object#20".to_string()],
                    "nx:om-object-indices#20".to_string()
                )
                .unwrap()
            )
            .unwrap(),

            keep: BodyTrimSide::Unresolved,
        }
    );
    let resolved = BTreeMap::from([
        (
            10,
            vec![BodyId::mint("test:model:entity#target".to_string()).expect("identity grammar")],
        ),
        (
            20,
            vec![BodyId::mint("test:model:entity#tool".to_string()).expect("identity grammar")],
        ),
    ]);
    assert_eq!(
        super::trim_body_feature_definition(10, &references, &roots, &resolved).unwrap(),
        FeatureDefinition::TrimBodies {
            operands: cadmpeg_ir::features::TrimBodyOperands::new(
                BodySelection::Resolved {
                    bodies: vec![BodyId::mint("test:model:entity#target".to_string())
                        .expect("identity grammar")],
                    native: "nx:om-object-index#10".to_string(),
                },
                BodySelection::Resolved {
                    bodies: vec![BodyId::mint("test:model:entity#tool".to_string())
                        .expect("identity grammar")],
                    native: "nx:om-object-indices#20".to_string(),
                }
            )
            .unwrap(),

            keep: BodyTrimSide::Unresolved,
        }
    );
    assert_eq!(
        super::trim_body_feature_definition(10, &[], &roots, &BTreeMap::new()).unwrap(),
        FeatureDefinition::TrimBodies {
            operands: cadmpeg_ir::features::TrimBodyOperands::new(
                BodySelection::local(
                    vec!["nx:om-body-object#10".to_string()],
                    "nx:om-object-index#10".to_string()
                )
                .unwrap(),
                BodySelection::Unresolved
            )
            .unwrap(),

            keep: BodyTrimSide::Unresolved,
        }
    );

    let same_body = BTreeMap::from([(10, 10), (20, 10)]);
    assert!(matches!(
        super::trim_body_feature_definition(
            10,
            &references,
            &same_body,
            &BTreeMap::new(),
        ).unwrap(), FeatureDefinition::TrimBodies {
            operands,

            ..
        } if matches!((operands.targets(), operands.tools(),), (BodySelection::Native(target), BodySelection::Native(tools),) if target == "nx:om-object-index#10" && tools == "nx:om-object-indices#20")));

    let mut mixed_operand = operands[0].clone();
    mixed_operand.operand_data_block = Some("nx:om-data-blocks-2:block#20".to_string());
    mixed_operand.segment_body_bindings.clear();
    let mixed_references = vec![&mixed_operand];
    assert!(matches!(
        super::trim_body_feature_definition(
            10,
            &mixed_references,
            &roots,
            &BTreeMap::new(),
        ).unwrap(), FeatureDefinition::TrimBodies {
            operands,

            ..
        } if matches!((operands.targets(), operands.tools(),), (BodySelection::Native(target), BodySelection::Native(tools),) if target == "nx:om-object-index#10" && tools == "nx:om-object-indices#20")));
}

#[test]
fn nx_named_operation_families_preserve_unresolved_semantics() {
    assert!(matches!(
        super::non_boolean_feature_definition("SKETCH", &[], None, None, None),
        cadmpeg_ir::features::FeatureDefinition::Sketch {
            sketch: cadmpeg_ir::features::SketchFeatureBinding::Unresolved
                | cadmpeg_ir::features::SketchFeatureBinding::Planar(None)
        }
    ));
    assert!(matches!(
        super::non_boolean_feature_definition(
            "SIMPLE HOLE",
            &["Hole_GeneralHole_Simple_Through_StartChamfer_EndChamfer"],
            None,
            None,
            None,
        ), cadmpeg_ir::features::FeatureDefinition::Hole {
            face: None,
            placements: None,
            shape,
            extent: Some(cadmpeg_ir::features::LinearTermination::ThroughAll {}),
            ..
        } if matches!((shape.construction(), shape.exit_kind(), &shape.diameter(),), (cadmpeg_ir::features::HoleConstruction::Form {
                kind: cadmpeg_ir::features::HoleKind::Unresolved(Some(
                    cadmpeg_ir::features::HoleForm::Chamfer,
                )),
                ..
            }, Some(cadmpeg_ir::features::HoleKind::Unresolved(Some(
                cadmpeg_ir::features::HoleForm::Chamfer,
            ))), None,))));
    assert!(matches!(
        super::non_boolean_feature_definition("SIMPLE HOLE", &["unrelated"], None, None, None,),
        cadmpeg_ir::features::FeatureDefinition::Hole { extent: None, .. }
    ));
    assert!(matches!(
        super::non_boolean_feature_definition(
            "CBORE_HOLE",
            &["Hole_GeneralHole_Counterbored_Through"],
            None,
            None,
            None,
        ), cadmpeg_ir::features::FeatureDefinition::Hole {
            shape,

            extent: Some(cadmpeg_ir::features::LinearTermination::ThroughAll {}),
            ..
        } if matches!((shape.construction(), shape.exit_kind(),), (cadmpeg_ir::features::HoleConstruction::Form {
                kind: cadmpeg_ir::features::HoleKind::Unresolved(Some(
                    cadmpeg_ir::features::HoleForm::Counterbore,
                )),
                ..
            }, None,))));
    assert!(matches!(
        super::non_boolean_feature_definition(
            "SIMPLE HOLE",
            &["Hole_GeneralHole_Simple_Blind"],
            None,
            None,
            None,
        ), cadmpeg_ir::features::FeatureDefinition::Hole {
            shape,

            extent: None,
            ..
        } if matches!((shape.construction(), shape.exit_kind(),), (cadmpeg_ir::features::HoleConstruction::Form {
                kind: cadmpeg_ir::features::HoleKind::Simple,
                ..
            }, None,))));
    assert!(matches!(
        super::non_boolean_feature_definition(
            "CSUNK_HOLE",
            &["Hole_GeneralHole_Countersunk_Through"],
            None,
            None,
            None,
        ), cadmpeg_ir::features::FeatureDefinition::Hole {
            shape,

            extent: Some(cadmpeg_ir::features::LinearTermination::ThroughAll {}),
            ..
        } if matches!((shape.construction(), shape.exit_kind(),), (cadmpeg_ir::features::HoleConstruction::Form {
                kind: cadmpeg_ir::features::HoleKind::Unresolved(Some(
                    cadmpeg_ir::features::HoleForm::Countersink,
                )),
                ..
            }, None,))));
    for competing in [
        "Hole_GeneralHole_Simple_Through_StartChamfer_EndChamfer",
        "Hole_Unknown",
    ] {
        assert!(matches!(
            super::non_boolean_feature_definition(
                "SIMPLE HOLE",
                &[
                    "Hole_GeneralHole_Simple_Through_StartChamfer_EndChamfer",
                    competing,
                ],
                None,
                None,
                None,
            ), cadmpeg_ir::features::FeatureDefinition::Hole {
                shape,

                extent: None,
                ..
            } if matches!((shape.construction(), shape.exit_kind(),), (cadmpeg_ir::features::HoleConstruction::Form {
                    kind: cadmpeg_ir::features::HoleKind::Simple,
                    ..
                }, None,))));
    }
    assert!(matches!(
        super::non_boolean_feature_definition("DATUM_PLANE", &[], None, None, None),
        cadmpeg_ir::features::FeatureDefinition::Unresolved {
            family: cadmpeg_ir::features::UnresolvedFamily::DatumPlane
        }
    ));
    assert!(matches!(
        super::non_boolean_feature_definition("EXTRACT_DATUM_PLANE", &[], None, None, None,),
        cadmpeg_ir::features::FeatureDefinition::Unresolved {
            family: cadmpeg_ir::features::UnresolvedFamily::DatumPlane
        }
    ));
    assert!(matches!(
        super::non_boolean_feature_definition("DATUM_CSYS", &[], None, None, None),
        cadmpeg_ir::features::FeatureDefinition::Unresolved {
            family: cadmpeg_ir::features::UnresolvedFamily::DatumCoordinateSystem
        }
    ));
    assert!(matches!(
        super::non_boolean_feature_definition("MASTER SNAPSHOT BODY", &[], None, None, None,),
        cadmpeg_ir::features::FeatureDefinition::BaseFeature {
            bodies: cadmpeg_ir::features::BodySelection::Unresolved,
        }
    ));
    assert!(matches!(
        super::non_boolean_feature_definition("TEXT", &["annotation", "Arial"], None, None, None,),
        cadmpeg_ir::features::FeatureDefinition::Native { .. }
    ));
    assert!(!super::projects_neutral_feature("TEXT"));
    assert!(matches!(
        super::non_boolean_feature_definition("BLOCK", &[], Some([10.0, 20.0, 30.0]), None, None,),
        cadmpeg_ir::features::FeatureDefinition::Block {
            dimensions: Some([
                actual_dimensions,
                actual_dimensions_2,
                actual_dimensions_3,
            ]),
            placement: None,
            op: BooleanOp::Unresolved,
        } if actual_dimensions.get() == 10.0 && actual_dimensions_2.get() == 20.0 && actual_dimensions_3.get() == 30.0
    ));
    assert_eq!(
        super::non_boolean_feature_definition("BLOCK", &[], None, None, None),
        cadmpeg_ir::features::FeatureDefinition::Block {
            dimensions: None,
            placement: None,
            op: BooleanOp::Unresolved,
        }
    );
}

#[test]
fn nx_extract_string_projects_as_history_only_without_semantic_lanes() {
    let object_indices = [None; 4];
    let source_properties = BTreeMap::from([
        ("object_index.0".to_string(), "null".to_string()),
        ("object_index.1".to_string(), "null".to_string()),
        ("object_index.2".to_string(), "null".to_string()),
        ("object_index.3".to_string(), "null".to_string()),
        ("operation_record".to_string(), "record".to_string()),
        (
            "operation_terminal_frame".to_string(),
            "terminal".to_string(),
        ),
    ]);
    assert!(matches!(
        super::non_modeling_history_definition(
            "EXTRACT_STRING",
            &object_indices,
            &[],
            0,
            0,
            0,
            &source_properties,
        ),
        Some(FeatureDefinition::TreeNode {
            role: FeatureTreeNodeRole::History,
            children,
        }) if children.is_empty() && children.active_child().is_none()
    ));

    let rejected = [
        (
            [Some(7), None, None, None],
            Vec::new(),
            0,
            0,
            0,
            source_properties.clone(),
        ),
        (
            object_indices,
            vec![BodyId::mint("test:model:entity#body").expect("identity grammar")],
            0,
            0,
            0,
            source_properties.clone(),
        ),
        (
            object_indices,
            Vec::new(),
            1,
            0,
            0,
            source_properties.clone(),
        ),
        (
            object_indices,
            Vec::new(),
            0,
            1,
            0,
            source_properties.clone(),
        ),
        (
            object_indices,
            Vec::new(),
            0,
            0,
            1,
            source_properties.clone(),
        ),
    ];
    for (object_indices, outputs, body_references, body_operands, strings, properties) in rejected {
        assert!(super::non_modeling_history_definition(
            "EXTRACT_STRING",
            &object_indices,
            &outputs,
            body_references,
            body_operands,
            strings,
            &properties,
        )
        .is_none());
    }

    let mut extra_property = source_properties.clone();
    extra_property.insert("input_block.0".into(), "block".into());
    assert!(super::non_modeling_history_definition(
        "EXTRACT_STRING",
        &object_indices,
        &[],
        0,
        0,
        0,
        &extra_property,
    )
    .is_none());
}

#[test]
fn nx_text_payload_projects_semantic_text_and_font_family() {
    let annotation =
        super::text_semantic_annotation("nx:test:text#1", 7, &["plate label", "Arial"])
            .expect("valid text annotation");
    assert_eq!(annotation.object, "nx:test:text#1");
    assert_eq!(
        annotation.kind,
        cadmpeg_ir::semantic_annotations::SemanticAnnotationKind::Text
    );
    assert_eq!(annotation.text, ["plate label"]);
    assert_eq!(annotation.parameters["font_family"], "Arial");
    assert_eq!(annotation.native_ref, "nx:test:text#1");
    assert_eq!(annotation.order, 7);

    let empty = super::text_semantic_annotation("nx:test:text#empty", 8, &["", ""])
        .expect("empty text fields remain a valid annotation");
    assert_eq!(empty.text, [""]);
    assert_eq!(empty.parameters["font_family"], "");

    assert!(
        super::text_semantic_annotation("nx:test:text#2", 0, &["ambiguous", "Arial", "extra"],)
            .is_none()
    );
}

#[test]
fn nx_extract_body_projects_its_primary_source_namespace() {
    use cadmpeg_ir::features::{BodySelection, FeatureDefinition};
    use cadmpeg_ir::ids::BodyId;
    use std::collections::BTreeMap;

    let roots = BTreeMap::from([(20, 20)]);
    let bodies = BTreeMap::from([(
        20,
        vec![BodyId::mint("test:model:entity#body".to_string()).expect("identity grammar")],
    )]);
    assert_eq!(
        super::extract_body_feature_definition(Some(20), &[], &roots, &bodies),
        FeatureDefinition::ExtractBody {
            source: BodySelection::Resolved {
                bodies: vec![
                    BodyId::mint("test:model:entity#body".to_string()).expect("identity grammar")
                ],
                native: "nx:om-object-index#20".to_string(),
            },
        }
    );
    assert_eq!(
        super::extract_body_feature_definition(
            None,
            &[(72, "nx:om-data-blocks-2:block#72".to_string())],
            &roots,
            &BTreeMap::new(),
        ),
        FeatureDefinition::ExtractBody {
            source: BodySelection::local(
                vec!["nx:om-data-blocks-2:block#72".to_string()],
                "nx:om-object-index#72".to_string()
            )
            .unwrap(),
        }
    );
    assert_eq!(
        super::extract_body_feature_definition(
            None,
            &[
                (72, "nx:om-data-blocks-2:block#72".to_string()),
                (73, "nx:om-data-blocks-2:block#73".to_string()),
            ],
            &roots,
            &BTreeMap::new(),
        ),
        FeatureDefinition::ExtractBody {
            source: BodySelection::Unresolved,
        }
    );
}

#[test]
fn nx_mainstream_operation_labels_project_typed_unresolved_definitions() {
    use cadmpeg_ir::features::{
        BodySelection, BodyTrimSide, BooleanKind, BooleanOp, ChamferSpec, EdgeSelection,
        FaceSelection, FeatureDefinition, HoleKind, PatternTransform, RibDraft,
    };

    for (kind, op) in [
        ("UNITE", BooleanKind::Join),
        ("SUBTRACT", BooleanKind::Cut),
        ("INTERSECT", BooleanKind::Intersect),
    ] {
        assert_eq!(
            super::non_boolean_feature_definition(kind, &[], None, None, None),
            FeatureDefinition::Combine {
                operands: cadmpeg_ir::features::CombineOperands::new(
                    BodySelection::Unresolved,
                    BodySelection::Unresolved
                )
                .unwrap(),

                op,
                keep_tools: false,
            }
        );
    }

    assert_eq!(
        super::non_boolean_feature_definition("EXTRACT_BODY", &[], None, None, None),
        FeatureDefinition::ExtractBody {
            source: BodySelection::Unresolved,
        }
    );
    assert_eq!(
        super::non_boolean_feature_definition("SKIN", &[], None, None, None),
        FeatureDefinition::Unresolved {
            family: UnresolvedFamily::Loft
        }
    );
    assert_eq!(
        super::non_boolean_feature_definition("Studio Surface", &[], None, None, None),
        FeatureDefinition::Unresolved {
            family: UnresolvedFamily::FreeformSurface
        }
    );
    assert_eq!(
        super::non_boolean_feature_definition("POINT", &[], None, None, None),
        FeatureDefinition::Unresolved {
            family: UnresolvedFamily::DatumPoint
        }
    );
    assert_eq!(
        super::non_boolean_feature_definition("DRAFT", &[], None, None, None),
        FeatureDefinition::Unresolved {
            family: UnresolvedFamily::Draft
        }
    );

    assert!(matches!(
        super::non_boolean_feature_definition("HOLE PACKAGE", &[], None, None, None), FeatureDefinition::Hole {
            shape,
            ..
        } if matches!((shape.construction(),), (cadmpeg_ir::features::HoleConstruction::Form {
                kind: HoleKind::Unresolved(None),
                ..
            },))));
    assert!(matches!(
        super::non_boolean_feature_definition(
            "HOLE PACKAGE",
            &[],
            None,
            None,
            Some(cadmpeg_ir::scalar::Length::new(8.0).unwrap()),
        ), FeatureDefinition::Hole {
            shape,

            ..
        } if matches!((&shape.diameter(), shape.construction(),), (Some(actual_diameter), cadmpeg_ir::features::HoleConstruction::Form {
                kind: HoleKind::Unresolved(None),
                ..
            },) if actual_diameter.get() == 8.0)));
    assert!(matches!(
        super::non_boolean_feature_definition("RIB", &[], None, None, None),
        FeatureDefinition::Rib {
            construction: cadmpeg_ir::features::RibConstruction {
                draft: RibDraft::Unresolved,
                ..
            },
            op: BooleanOp::Unresolved,
        }
    ));
    assert_eq!(
        super::non_boolean_feature_definition("BLEND", &[], None, None, None),
        FeatureDefinition::Native {
            kind: "BLEND".into(),
            parameters: BTreeMap::new(),
        }
    );
    assert_eq!(
        super::non_boolean_feature_definition("FACE_BLEND", &[], None, None, None),
        FeatureDefinition::Native {
            kind: "FACE_BLEND".into(),
            parameters: BTreeMap::new(),
        }
    );
    for kind in ["CPROJ", "CPROJ_CMB"] {
        assert_eq!(
            super::non_boolean_feature_definition(kind, &[], None, None, None),
            FeatureDefinition::ProjectedCurve {
                source: cadmpeg_ir::features::PathRef::Unresolved("nx:unresolved".into()),
                target_faces: FaceSelection::Unresolved,
                direction: cadmpeg_ir::features::CurveProjectionDirection::State(
                    cadmpeg_ir::features::CurveProjectionDirectionState::Unresolved,
                ),
                bidirectional: None,
            }
        );
    }
    assert_eq!(
        super::non_boolean_feature_definition("TRIMMED_SH", &[], None, None, None),
        FeatureDefinition::TrimSurface {
            faces: FaceSelection::Unresolved,
            tool: cadmpeg_ir::features::PathRef::Unresolved("nx:unresolved".into()),
            keep: cadmpeg_ir::features::TrimRegion::Unresolved,
        }
    );
    assert_eq!(
        super::non_boolean_feature_definition("EXTEND_SHEET", &[], None, None, None),
        FeatureDefinition::ExtendSurface {
            faces: FaceSelection::Unresolved,
            distance: None,
            method: cadmpeg_ir::features::SurfaceExtension::Unresolved,
        }
    );
    assert!(matches!(
        super::non_boolean_feature_definition("CHAMFER", &[], None, None, None),
        FeatureDefinition::Chamfer {
            groups,
            flip_direction: false,
        } if matches!(groups.as_slice(), [cadmpeg_ir::features::ChamferGroup {
            edges: EdgeSelection::Unresolved,
        spec: ChamferSpec::Unresolved,
        }])
    ));
    assert_eq!(
        super::non_boolean_feature_definition("SEW", &[], None, None, None),
        FeatureDefinition::SewBodies {
            bodies: (BodySelection::Unresolved).try_into().unwrap(),
            gap_tolerance: None,
        }
    );
    assert_eq!(
        super::non_boolean_feature_definition("TRIM BODY", &[], None, None, None),
        FeatureDefinition::TrimBodies {
            operands: cadmpeg_ir::features::TrimBodyOperands::new(
                BodySelection::Unresolved,
                BodySelection::Unresolved
            )
            .unwrap(),

            keep: BodyTrimSide::Unresolved,
        }
    );
    assert_eq!(
        super::non_boolean_feature_definition("EXTRUDE", &[], None, None, None),
        FeatureDefinition::Extrude {
            profile: cadmpeg_ir::features::ProfileRef::Unresolved("EXTRUDE".into()),
            direction: cadmpeg_ir::features::ExtrudeDirection::Unresolved {},
            extent: cadmpeg_ir::features::ExtrudeExtent::OneSided {
                side: cadmpeg_ir::features::ExtrudeSide {
                    termination: cadmpeg_ir::features::LinearTermination::Unresolved {},
                    draft: None,
                },
            },
            op: BooleanOp::Unresolved,
            start: cadmpeg_ir::features::ExtrudeStart::Unresolved {},
            solid: None,
            face_maker: None,
            inner_wire_taper: None,
            length_along_profile_normal: None,
            allow_multi_profile_faces: None,
        }
    );
    assert_eq!(
        super::non_boolean_feature_definition("OFFSET", &[], None, None, None),
        FeatureDefinition::OffsetSurface {
            faces: FaceSelection::Unresolved,
            distance: None,
        }
    );
    assert!(matches!(
        super::non_boolean_feature_definition("THICKEN_SHEET", &[], None, None, None),
        FeatureDefinition::Thicken {
            faces: FaceSelection::Unresolved,
            thickness: None,
            side: None,
        }
    ));
    for kind in [
        "Pattern Feature",
        "Pattern Geometry",
        "Geometry Instance",
        "IDENTICAL INSTANCE OUTPUT",
        "Instance Feature",
    ] {
        assert!(
            matches!(&(super::non_boolean_feature_definition(kind, &[], None, None, None)),
                FeatureDefinition::Pattern {
                    seeds,
            pattern: admitted_pattern,
                } if matches!(admitted_pattern.definition(), PatternTransform::Unresolved if seeds.is_empty())
            )
        );
    }
}

#[test]
fn nx_container_record_is_not_a_modeling_feature() {
    assert!(!super::projects_neutral_feature("Container"));
    assert!(super::projects_neutral_feature("EXTRUDE"));
}

#[test]
fn nx_block_placement_requires_native_dimensions_and_unique_axes() {
    let mut ir = cadmpeg_ir::examples::unit_cube();
    let dimensions = [10.0, 20.0, 30.0];
    for axis in 0..3 {
        let mut surfaces = ir
            .model
            .surfaces
            .iter_mut()
            .filter_map(|surface| {
                let SurfaceGeometry::Plane(plane_surface) = &mut surface.geometry else {
                    return None;
                };
                let normal = plane_surface.normal();
                let components = [normal.x.abs(), normal.y.abs(), normal.z.abs()];
                (components[axis] > 0.5).then_some(plane_surface)
            })
            .collect::<Vec<_>>();
        assert_eq!(surfaces.len(), 2);
        surfaces.sort_by(|first, second| {
            let first = first.origin();
            let second = second.origin();
            [first.x, first.y, first.z][axis].total_cmp(&[second.x, second.y, second.z][axis])
        });
        for (index, surface) in surfaces.into_iter().enumerate() {
            let origin = surface.origin();
            let normal = surface.normal();
            let u_axis = surface.u_axis();
            let mut origin = *origin;
            let coordinate = if index == 0 { 0.0 } else { dimensions[axis] };
            match axis {
                0 => origin.x = coordinate,
                1 => origin.y = coordinate,
                2 => origin.z = coordinate,
                _ => unreachable!(),
            }
            *surface =
                cadmpeg_ir::geometry::PlaneSurface::try_new(origin, *normal, *u_axis).unwrap();
        }
    }
    let output = ir.model.bodies[0].id.clone();
    let placement = |ir: &CadIr, dimensions, outputs: &[BodyId]| {
        super::block_placement(ir, dimensions, outputs).map(|(_, transform)| transform)
    };

    assert_eq!(
        placement(&ir, dimensions, std::slice::from_ref(&output)),
        Some(cadmpeg_ir::transform::Transform::identity())
    );
    assert_eq!(
        super::block_placement(&ir, dimensions, &[]),
        Some((output.clone(), cadmpeg_ir::transform::Transform::identity()))
    );
    assert_eq!(
        placement(&ir, dimensions, &[]),
        Some(cadmpeg_ir::transform::Transform::identity())
    );
    assert_eq!(
        placement(&ir, dimensions, &[output.clone(), output.clone()],),
        None
    );
    assert_eq!(
        placement(&ir, [10.0, 10.0, 30.0], std::slice::from_ref(&output),),
        None
    );

    let mut repeated = ir.clone();
    let high_y = repeated
        .model
        .surfaces
        .iter_mut()
        .find_map(|surface| {
            let SurfaceGeometry::Plane(plane_surface) = &mut surface.geometry else {
                return None;
            };
            let origin = plane_surface.origin();
            let normal = plane_surface.normal();
            (normal.y.abs() > 0.5 && origin.y > 0.0).then_some(plane_surface)
        })
        .expect("positive y plane");
    let origin = high_y.origin();
    let normal = high_y.normal();
    let u_axis = high_y.u_axis();
    let mut origin = *origin;
    origin.y = 10.0;
    *high_y = cadmpeg_ir::geometry::PlaneSurface::try_new(origin, *normal, *u_axis).unwrap();
    assert_eq!(
        placement(&repeated, [10.0, 10.0, 30.0], std::slice::from_ref(&output),),
        None
    );

    let mut stepped = ir.clone();
    let mut intermediate_surface = stepped
        .model
        .surfaces
        .iter()
        .find(|surface| {
            matches!(&surface.geometry, SurfaceGeometry::Plane(plane_surface)
            if {
                let normal = plane_surface.normal();
                normal.x.abs() > 0.5
            })
        })
        .expect("x-normal plane")
        .clone();
    intermediate_surface.id =
        cadmpeg_ir::ids::SurfaceId::mint("test:model:entity#intermediate-plane")
            .expect("identity grammar");
    let SurfaceGeometry::Plane(plane_surface) = &mut intermediate_surface.geometry else {
        unreachable!()
    };
    let origin = plane_surface.origin();
    let normal = plane_surface.normal();
    let u_axis = plane_surface.u_axis();
    let mut origin = *origin;
    origin.x = 5.0;
    *plane_surface = cadmpeg_ir::geometry::PlaneSurface::try_new(origin, *normal, *u_axis).unwrap();
    stepped.model.surfaces.push(intermediate_surface);
    let mut intermediate_face = stepped.model.faces.first().expect("cube face").clone();
    intermediate_face.id = cadmpeg_ir::ids::FaceId::mint("test:model:entity#intermediate-face")
        .expect("identity grammar");
    intermediate_face.surface =
        cadmpeg_ir::ids::SurfaceId::mint("test:model:entity#intermediate-plane")
            .expect("identity grammar");
    intermediate_face.loops.clear();
    stepped.model.shells[0].add_face(intermediate_face.id.clone());
    stepped.model.faces.push(intermediate_face);
    assert_eq!(
        placement(&stepped, dimensions, std::slice::from_ref(&output)),
        None
    );

    let mut nonplanar = ir.clone();
    nonplanar.model.surfaces[0].geometry = SurfaceGeometry::Sphere(
        cadmpeg_ir::geometry::SphereSurface::try_new(
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            1.0,
        )
        .unwrap(),
    );
    assert_eq!(
        placement(&nonplanar, dimensions, std::slice::from_ref(&output)),
        None
    );

    let mut missing_surface = ir.clone();
    let removed = missing_surface.model.surfaces.pop().expect("cube surface");
    assert!(missing_surface
        .model
        .faces
        .iter()
        .any(|face| face.surface == removed.id));
    assert_eq!(placement(&missing_surface, dimensions, &[]), None);

    let mut curved_feature = ir.clone();
    let mut curved_surface = curved_feature.model.surfaces[0].clone();
    curved_surface.id = cadmpeg_ir::ids::SurfaceId::mint("test:model:entity#later-curved-surface")
        .expect("identity grammar");
    curved_surface.geometry = SurfaceGeometry::Sphere(
        cadmpeg_ir::geometry::SphereSurface::try_new(
            cadmpeg_ir::math::Point3::new(5.0, 10.0, 15.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            1.0,
        )
        .unwrap(),
    );
    curved_feature.model.surfaces.push(curved_surface);
    let mut curved_face = curved_feature.model.faces[0].clone();
    curved_face.id = cadmpeg_ir::ids::FaceId::mint("test:model:entity#later-curved-face")
        .expect("identity grammar");
    curved_face.surface =
        cadmpeg_ir::ids::SurfaceId::mint("test:model:entity#later-curved-surface")
            .expect("identity grammar");
    curved_face.loops.clear();
    curved_feature.model.shells[0].add_face(curved_face.id.clone());
    curved_feature.model.faces.push(curved_face);
    assert_eq!(
        placement(&curved_feature, dimensions, &[]),
        Some(cadmpeg_ir::transform::Transform::identity())
    );

    let mut sheet = ir.clone();
    sheet.model.bodies[0].kind = cadmpeg_ir::topology::BodyKind::Sheet;
    assert_eq!(
        placement(&sheet, dimensions, std::slice::from_ref(&output)),
        None
    );

    let mut disconnected = ir.clone();
    let mut second_region = disconnected.model.regions[0].clone();
    second_region.id = cadmpeg_ir::ids::RegionId::mint("test:model:entity#second-region")
        .expect("identity grammar");
    second_region.shells.clear();
    disconnected.model.bodies[0]
        .regions
        .push(second_region.id.clone());
    disconnected.model.regions.push(second_region);
    assert_eq!(
        placement(&disconnected, dimensions, std::slice::from_ref(&output),),
        None
    );
}

#[test]
fn nx_sphere_projection_requires_one_complete_spherical_body() {
    let mut ir = cadmpeg_ir::examples::unit_cube();
    let body = ir.model.bodies[0].id.clone();
    let face = ir.model.faces[0].id.clone();
    let surface = ir.model.faces[0].surface.clone();
    {
        let members = vec![face];
        ir.model.shells[0].edit_topology(|faces, _, _| *faces = members)
    }
    .unwrap();
    ir.model
        .faces
        .retain(|candidate| candidate.id == ir.model.shells[0].faces()[0]);
    ir.model
        .surfaces
        .retain(|candidate| candidate.id == surface);
    ir.model.surfaces[0].geometry = SurfaceGeometry::Sphere(
        cadmpeg_ir::geometry::SphereSurface::try_new(
            Point3::new(1.0, 2.0, 3.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            f64::EPSILON,
        )
        .unwrap(),
    );

    assert_eq!(
        super::sphere_body_projection(&ir, &[]),
        Some((
            body.clone(),
            Point3::new(1., 2., 3.),
            Length::new(f64::EPSILON).unwrap()
        ))
    );
    assert_eq!(
        super::sphere_body_projection(&ir, std::slice::from_ref(&body)),
        Some((
            body.clone(),
            Point3::new(1., 2., 3.),
            Length::new(f64::EPSILON).unwrap()
        ))
    );

    let mut second_body = ir.model.bodies[0].clone();
    second_body.id = BodyId::mint("test:model:entity#second-body").expect("identity grammar");
    second_body.regions = vec![
        cadmpeg_ir::ids::RegionId::mint("test:model:entity#second-region")
            .expect("identity grammar"),
    ];
    let mut second_region = ir.model.regions[0].clone();
    second_region.id = cadmpeg_ir::ids::RegionId::mint("test:model:entity#second-region")
        .expect("identity grammar");
    second_region.body = second_body.id.clone();
    second_region.shells = vec![
        cadmpeg_ir::ids::ShellId::mint("test:model:entity#second-shell").expect("identity grammar"),
    ];
    let mut second_shell = ir.model.shells[0].clone();
    second_shell.id =
        cadmpeg_ir::ids::ShellId::mint("test:model:entity#second-shell").expect("identity grammar");
    second_shell.region = second_region.id.clone();
    {
        let members = vec![
            cadmpeg_ir::ids::FaceId::mint("test:model:entity#second-face")
                .expect("identity grammar"),
        ];
        second_shell.edit_topology(|faces, _, _| *faces = members)
    }
    .unwrap();
    let mut second_face = ir.model.faces[0].clone();
    second_face.id =
        cadmpeg_ir::ids::FaceId::mint("test:model:entity#second-face").expect("identity grammar");
    second_face.shell = second_shell.id.clone();
    second_face.surface = cadmpeg_ir::ids::SurfaceId::mint("test:model:entity#second-surface")
        .expect("identity grammar");
    let mut second_surface = ir.model.surfaces[0].clone();
    second_surface.id = second_face.surface.clone();
    ir.model.bodies.push(second_body);
    ir.model.regions.push(second_region);
    ir.model.shells.push(second_shell);
    ir.model.faces.push(second_face);
    ir.model.surfaces.push(second_surface);

    assert!(super::sphere_body_projection(&ir, &[]).is_none());
    assert!(super::sphere_body_projection(
        &ir,
        &[
            body,
            BodyId::mint("test:model:entity#second-body").expect("identity grammar")
        ]
    )
    .is_none());
}

#[test]
fn nx_block_new_body_ignores_only_the_provisional_initial_writer() {
    let body = BodyId::mint("test:model:entity#body").expect("identity grammar");
    let provisional =
        FeatureId::mint("synthetic:test:id#initial-bodies").expect("identity grammar");
    let mut history = BodyWriterHistory::default();
    history.record_writer(None, None, std::slice::from_ref(&body), &provisional);

    assert_eq!(
        super::new_body_boolean_op(&super::NewBodyEvidence {
            has_complete_projection: true,
            has_complete_primitive_construction: false,
            outputs: std::slice::from_ref(&body),
            body_reference_count: 0,
            provisional_feature: Some(&provisional),
            native_primary_body: None,
            offset_store_primary_body: None,
            history: &history,
        }),
        BooleanOp::NewBody
    );

    let fallback_prior =
        FeatureId::mint("synthetic:test:id#fallback-prior-feature").expect("identity grammar");
    let mut fallback_history = BodyWriterHistory::default();
    fallback_history.record_writer(None, None, std::slice::from_ref(&body), &fallback_prior);
    assert_eq!(
        super::new_body_boolean_op(&super::NewBodyEvidence {
            has_complete_projection: true,
            has_complete_primitive_construction: false,
            outputs: std::slice::from_ref(&body),
            body_reference_count: 0,
            provisional_feature: Some(&provisional),
            native_primary_body: None,
            offset_store_primary_body: None,
            history: &fallback_history,
        }),
        BooleanOp::Unresolved
    );

    let prior = FeatureId::mint("synthetic:test:id#prior-feature").expect("identity grammar");
    history.record_writer(Some(7), None, std::slice::from_ref(&body), &prior);
    assert_eq!(
        super::new_body_boolean_op(&super::NewBodyEvidence {
            has_complete_projection: true,
            has_complete_primitive_construction: false,
            outputs: std::slice::from_ref(&body),
            body_reference_count: 1,
            provisional_feature: Some(&provisional),
            native_primary_body: Some(7),
            offset_store_primary_body: None,
            history: &history,
        }),
        BooleanOp::Unresolved
    );
    assert_eq!(
        super::new_body_boolean_op(&super::NewBodyEvidence {
            has_complete_projection: false,
            has_complete_primitive_construction: false,
            outputs: std::slice::from_ref(&body),
            body_reference_count: 0,
            provisional_feature: Some(&provisional),
            native_primary_body: None,
            offset_store_primary_body: None,
            history: &history,
        }),
        BooleanOp::Unresolved
    );

    let offset_prior =
        FeatureId::mint("synthetic:test:id#offset-prior-feature").expect("identity grammar");
    let mut offset_history = BodyWriterHistory::default();
    offset_history.record_writer(None, Some("store:block#7"), &[], &offset_prior);
    assert_eq!(
        super::new_body_boolean_op(&super::NewBodyEvidence {
            has_complete_projection: true,
            has_complete_primitive_construction: false,
            outputs: std::slice::from_ref(&body),
            body_reference_count: 1,
            provisional_feature: Some(&provisional),
            native_primary_body: None,
            offset_store_primary_body: Some("store:block#7"),
            history: &offset_history,
        }),
        BooleanOp::Unresolved
    );

    let offset_without_prior = BodyWriterHistory::default();
    assert_eq!(
        super::new_body_boolean_op(&super::NewBodyEvidence {
            has_complete_projection: true,
            has_complete_primitive_construction: false,
            outputs: std::slice::from_ref(&body),
            body_reference_count: 1,
            provisional_feature: Some(&provisional),
            native_primary_body: None,
            offset_store_primary_body: Some("store:block#8"),
            history: &offset_without_prior,
        }),
        BooleanOp::NewBody
    );

    assert_eq!(
        super::new_body_boolean_op(&super::NewBodyEvidence {
            has_complete_projection: true,
            has_complete_primitive_construction: false,
            outputs: std::slice::from_ref(&body),
            body_reference_count: 2,
            provisional_feature: Some(&provisional),
            native_primary_body: None,
            offset_store_primary_body: None,
            history: &offset_without_prior,
        }),
        BooleanOp::Unresolved
    );

    assert_eq!(
        super::new_body_boolean_op(&super::NewBodyEvidence {
            has_complete_projection: true,
            has_complete_primitive_construction: true,
            outputs: std::slice::from_ref(&body),
            body_reference_count: 2,
            provisional_feature: Some(&provisional),
            native_primary_body: None,
            offset_store_primary_body: None,
            history: &offset_without_prior,
        }),
        BooleanOp::NewBody
    );
}
mod hole_geometry;
