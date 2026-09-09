use super::*;

#[test]
fn complete_hole_preserves_the_existing_body_identity() {
    let mut ir = complete_block_ir();
    let body = ir.model.bodies[0].id.clone();
    ir.model.features.push(complete_hole(body.clone()));

    assert_eq!(
        evaluate_saved_body_census(&ir),
        BodyCensusEvaluation::Verified { bodies: vec![body] }
    );
}

#[test]
fn complete_sphere_rederives_a_new_body() {
    let mut ir = CadIr::empty();
    let body = BodyId::mint("test:model:entity#sphere".to_string()).expect("identity grammar");
    ir.model.bodies.push(model_body(body.as_str()));
    ir.model.features.push(Feature {
        id: FeatureId::mint("synthetic:test:id#sphere-feature".to_string())
            .expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::new(
            FeatureDefinition::Sphere {
                center: cadmpeg_ir::features::FinitePoint3::new(Point3::new(1.0, 2.0, 3.0))
                    .unwrap(),
                radius: cadmpeg_ir::scalar::PositiveLength::new(4.0).unwrap(),
                op: BooleanOp::NewBody,
            },
            vec![body.clone()],
        )
        .unwrap(),
        native_ref: None,
    });

    assert_eq!(
        evaluate_saved_body_census(&ir),
        BodyCensusEvaluation::Verified { bodies: vec![body] }
    );
}

#[test]
fn exact_empty_replay_input_precedes_a_new_body_construction() {
    let mut ir = complete_block_ir();
    ir.model.features[0].ordinal = 1;
    ir.model.features.insert(
        0,
        Feature {
            id: FeatureId::mint("synthetic:test:id#initial-bodies".to_string())
                .expect("identity grammar"),
            ordinal: 0,
            name: Some("Retained history input".to_string()),
            suppressed: Some(false),
            dependencies: cadmpeg_ir::features::DistinctMembers::default(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: cadmpeg_ir::features::FeatureContent::default(),

            evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
                FeatureDefinition::BaseFeature {
                    bodies: BodySelection::Resolved {
                        bodies: Vec::new(),
                        native: "nx:segment-body-bindings".to_string(),
                    },
                },
            ),
            native_ref: None,
        },
    );

    assert_eq!(
        evaluate_saved_body_census(&ir),
        BodyCensusEvaluation::Verified {
            bodies: vec![
                BodyId::mint("test:model:entity#body".to_string()).expect("identity grammar")
            ],
        }
    );
}

#[test]
fn in_place_edit_can_precede_a_saved_body_image_creator() {
    let mut ir = complete_block_ir();
    let body = ir.model.bodies[0].id.clone();
    ir.model.features[0].ordinal = 1;
    ir.model.features.push(complete_hole(body.clone()));
    ir.model.features[1].ordinal = 0;

    assert_eq!(
        evaluate_saved_body_census(&ir),
        BodyCensusEvaluation::Verified { bodies: vec![body] }
    );
}

#[test]
fn curve_construction_families_do_not_change_the_body_census() {
    let mut ir = complete_block_ir();
    let body = ir.model.bodies[0].id.clone();
    ir.model.features.extend([
        body_neutral_feature(
            "projected-curve",
            1,
            FeatureDefinition::ProjectedCurve {
                source: PathRef::Unresolved("source".to_string()),
                target_faces: FaceSelection::Unresolved,
                direction: CurveProjectionDirection::State(
                    CurveProjectionDirectionState::Unresolved,
                ),
                bidirectional: None,
            },
        ),
        body_neutral_feature(
            "section",
            2,
            FeatureDefinition::SectionShape {
                operands: cadmpeg_ir::features::SectionOperands::new(
                    BodySelection::Unresolved,
                    BodySelection::Unresolved,
                )
                .unwrap(),

                approximate: None,
            },
        ),
    ]);

    assert_eq!(
        evaluate_saved_body_census(&ir),
        BodyCensusEvaluation::Verified { bodies: vec![body] }
    );
}

#[test]
fn curve_construction_family_cannot_claim_a_body_output() {
    let mut ir = complete_block_ir();
    let body = ir.model.bodies[0].id.clone();
    let mut section = body_neutral_feature(
        "section",
        1,
        FeatureDefinition::SectionShape {
            operands: cadmpeg_ir::features::SectionOperands::new(
                BodySelection::Unresolved,
                BodySelection::Unresolved,
            )
            .unwrap(),

            approximate: None,
        },
    );
    section
        .evaluation
        .try_edit(|_, outputs| {
            outputs.push(body);
        })
        .unwrap();
    ir.model.features.push(section);

    assert_eq!(
        evaluate_saved_body_census(&ir),
        BodyCensusEvaluation::Unsupported {
            feature: FeatureBoundary {
                id: FeatureId::mint("synthetic:test:id#section".to_string())
                    .expect("identity grammar"),
                name: None,
                family: Some("section_shape".to_string()),
                ordinal: 1
            },
            reason: UnsupportedBodyCensusReason::InvalidOutputLineage,
        }
    );
}

#[test]
fn unresolved_block_result_mode_stops_before_body_effect_evaluation() {
    let mut ir = complete_block_ir();
    ir.model.features[0]
        .evaluation
        .try_edit(|definition, _| {
            let FeatureDefinition::Block { op, .. } = definition else {
                unreachable!("block fixture")
            };
            *op = BooleanOp::Unresolved;
        })
        .unwrap();

    assert_eq!(
        evaluate_saved_body_census(&ir),
        BodyCensusEvaluation::Unsupported {
            feature: FeatureBoundary {
                id: FeatureId::mint("synthetic:test:id#block".to_string())
                    .expect("identity grammar"),
                name: None,
                family: Some("block".to_string()),
                ordinal: 0
            },
            reason: UnsupportedBodyCensusReason::IncompleteFeatureDefinition,
        }
    );
}

#[test]
fn boolean_block_preserves_its_existing_output_body() {
    let mut ir = complete_block_ir();
    let body = ir.model.bodies[0].id.clone();
    ir.model.features.push(body_preserving_feature(
        "joined-block",
        1,
        body.clone(),
        FeatureDefinition::Block {
            dimensions: Some([
                cadmpeg_ir::scalar::PositiveLength::new(0.5).unwrap(),
                cadmpeg_ir::scalar::PositiveLength::new(0.5).unwrap(),
                cadmpeg_ir::scalar::PositiveLength::new(0.5).unwrap(),
            ]),
            placement: Some(cadmpeg_ir::features::FeatureRigidPlacement::identity()),
            op: BooleanOp::Join,
        },
    ));

    assert_eq!(
        evaluate_saved_body_census(&ir),
        BodyCensusEvaluation::Verified { bodies: vec![body] }
    );
}

#[test]
fn unresolved_block_mode_preserves_a_proven_existing_output() {
    let mut ir = complete_block_ir();
    let body = ir.model.bodies[0].id.clone();
    ir.model.features.push(body_preserving_feature(
        "existing-block",
        1,
        body.clone(),
        FeatureDefinition::Block {
            dimensions: Some([
                cadmpeg_ir::scalar::PositiveLength::new(1.0).unwrap(),
                cadmpeg_ir::scalar::PositiveLength::new(2.0).unwrap(),
                cadmpeg_ir::scalar::PositiveLength::new(3.0).unwrap(),
            ]),
            placement: Some(cadmpeg_ir::features::FeatureRigidPlacement::identity()),
            op: BooleanOp::Unresolved,
        },
    ));

    assert_eq!(
        evaluate_saved_body_census(&ir),
        BodyCensusEvaluation::Verified { bodies: vec![body] }
    );
}

#[test]
fn complete_extrudes_apply_new_body_and_boolean_output_lineage() {
    let mut ir = complete_block_ir();
    let existing = ir.model.bodies[0].id.clone();
    let created = BodyId::mint("test:model:entity#extruded".to_string()).expect("identity grammar");
    ir.model.bodies.push(model_body(created.as_str()));
    let profile =
        FeatureId::mint("synthetic:test:id#profile".to_string()).expect("identity grammar");
    ir.model.features.push(body_neutral_feature(
        "profile",
        1,
        FeatureDefinition::Sketch {
            sketch: cadmpeg_ir::features::SketchFeatureBinding::Unresolved,
        },
    ));
    ir.model.features.extend([
        complete_extrude_feature(
            "new-extrude",
            2,
            profile.clone(),
            vec![created.clone()],
            BooleanOp::NewBody,
        ),
        complete_extrude_feature(
            "joined-extrude",
            3,
            profile,
            vec![existing.clone()],
            BooleanOp::Join,
        ),
    ]);

    assert_eq!(
        evaluate_saved_body_census(&ir),
        BodyCensusEvaluation::Verified {
            bodies: vec![existing, created],
        }
    );
}

#[test]
fn new_body_operation_cannot_reuse_an_existing_body_identity() {
    let mut ir = complete_block_ir();
    let body = ir.model.bodies[0].id.clone();
    let profile =
        FeatureId::mint("synthetic:test:id#profile".to_string()).expect("identity grammar");
    ir.model.features.push(body_neutral_feature(
        "profile",
        1,
        FeatureDefinition::Sketch {
            sketch: cadmpeg_ir::features::SketchFeatureBinding::Unresolved,
        },
    ));
    ir.model.features.push(complete_extrude_feature(
        "extrude",
        2,
        profile,
        vec![body],
        BooleanOp::NewBody,
    ));

    assert_eq!(
        evaluate_saved_body_census(&ir),
        BodyCensusEvaluation::Unsupported {
            feature: FeatureBoundary {
                id: FeatureId::mint("synthetic:test:id#extrude".to_string())
                    .expect("identity grammar"),
                name: None,
                family: Some("extrude".to_string()),
                ordinal: 2
            },
            reason: UnsupportedBodyCensusReason::InvalidOutputLineage,
        }
    );
}

#[test]
fn unresolved_extrude_preserves_an_existing_output_without_construction_replay() {
    let mut ir = complete_block_ir();
    let body = ir.model.bodies[0].id.clone();
    let mut extrude = complete_extrude_feature(
        "extrude",
        1,
        FeatureId::mint("synthetic:test:id#missing-profile".to_string()).expect("identity grammar"),
        vec![body.clone()],
        BooleanOp::Unresolved,
    );
    extrude.suppressed = None;
    extrude.dependencies.clear();
    ir.model.features.push(extrude);

    assert_eq!(
        evaluate_saved_body_census(&ir),
        BodyCensusEvaluation::Verified { bodies: vec![body] }
    );
}

#[test]
fn profile_driven_families_report_incomplete_construction_before_lineage() {
    let definitions = [
        FeatureDefinition::Loft {
            sections: Vec::new(),
            guidance: cadmpeg_ir::features::LoftGuidance::Guides(Vec::new()),
            op: BooleanOp::NewBody,
            closed: false,
            solid: true,
            ruled: false,
            linearize: false,
            max_degree: None,
            allow_multi_profile_faces: None,
        },
        complete_extrude_feature(
            "fixture",
            1,
            FeatureId::mint("synthetic:test:id#profile".to_string()).expect("identity grammar"),
            Vec::new(),
            BooleanOp::NewBody,
        )
        .evaluation
        .definition()
        .clone(),
        FeatureDefinition::Revolve {
            construction: RevolveConstruction::new(None, None, None, None, None, None, None),
            op: BooleanOp::NewBody,
        },
        FeatureDefinition::Rib {
            construction: RibConstruction {
                profile: None,
                direction: None,
                thickness: None,
                side: None,
                draft: RibDraft::Unresolved,
            },
            op: BooleanOp::Join,
        },
        FeatureDefinition::Sweep {
            shape: cadmpeg_ir::features::SweepShape::new(
                SweepSection::Unresolved(None),
                Vec::new(),
                SweepMode::Unresolved,
            )
            .unwrap(),

            path: None,

            orientation: None,
            transition: None,
            transformation: None,
            path_tangent: false,
            linearize: false,
            twist: None,
            path_extent: None,
            guide_rail: None,
            taper: None,
            scale: None,
            allow_multi_profile_faces: None,
        },
    ];
    for (index, definition) in definitions.into_iter().enumerate() {
        let mut ir = complete_block_ir();
        let id = format!("incomplete-{index}");
        ir.model
            .features
            .push(body_neutral_feature(&id, 1, definition));

        assert_eq!(
            evaluate_saved_body_census(&ir),
            BodyCensusEvaluation::Unsupported {
                feature: FeatureBoundary {
                    id: FeatureId::mint(format!("synthetic:test:id#{id}"))
                        .expect("identity grammar"),
                    name: None,
                    family: Some(["loft", "extrude", "revolve", "rib", "sweep"][index].to_string()),
                    ordinal: 1
                },
                reason: UnsupportedBodyCensusReason::IncompleteFeatureDefinition,
            }
        );
    }
}

#[test]
fn complete_active_configuration_admits_the_rederived_model() {
    let mut ir = complete_block_ir();
    let body = ir.model.bodies[0].id.clone();
    attach_complete_active_configuration(&mut ir);

    assert_eq!(
        evaluate_saved_body_census(&ir),
        BodyCensusEvaluation::Verified { bodies: vec![body] }
    );
}

#[test]
fn incomplete_active_configuration_remains_an_evaluation_boundary() {
    let mut ir = complete_block_ir();
    attach_complete_active_configuration(&mut ir);
    ir.model.configurations[0].feature_states.clear();

    assert_eq!(
        evaluate_saved_body_census(&ir),
        BodyCensusEvaluation::ConfigurationEvaluation
    );
}

#[test]
fn body_neutral_history_needs_only_exact_configuration_body_membership() {
    let mut ir = CadIr::empty();
    let mut datum = body_neutral_feature(
        "datum",
        0,
        FeatureDefinition::Unresolved {
            family: UnresolvedFamily::DatumCoordinateSystem,
        },
    );
    datum.suppressed = None;
    ir.model.features.push(datum);
    attach_complete_active_configuration(&mut ir);
    ir.model.configurations[0].feature_states.clear();

    assert_eq!(
        evaluate_saved_body_census(&ir),
        BodyCensusEvaluation::Verified { bodies: Vec::new() }
    );
}

#[test]
fn empty_body_neutral_model_does_not_need_an_active_configuration_identity() {
    let mut ir = CadIr::empty();
    ir.model.features.push(body_neutral_feature(
        "datum",
        0,
        FeatureDefinition::Unresolved {
            family: UnresolvedFamily::DatumCoordinateSystem,
        },
    ));
    attach_complete_active_configuration(&mut ir);
    ir.model.configurations[0].active = false;
    ir.model.configurations[0].bodies = ConfigurationBodies::Unresolved;

    assert_eq!(
        evaluate_saved_body_census(&ir),
        BodyCensusEvaluation::Verified { bodies: Vec::new() }
    );
}

#[test]
fn incomplete_hole_construction_does_not_change_its_body_identity_effect() {
    let mut ir = complete_block_ir();
    let body = ir.model.bodies[0].id.clone();
    let mut hole = complete_hole(body.clone());
    hole.evaluation
        .try_edit(|definition, _| {
            let FeatureDefinition::Hole { placements, .. } = definition else {
                unreachable!("hole fixture")
            };
            *placements = None;
        })
        .unwrap();
    ir.model.features.push(hole);

    assert!(feature_completeness::hole_definition_is_incomplete(
        &ir.model.features[1]
    ));
    assert_eq!(
        evaluate_saved_body_census(&ir),
        BodyCensusEvaluation::Verified { bodies: vec![body] }
    );
}

#[test]
fn hole_cannot_modify_a_body_absent_from_prior_history() {
    let mut ir = complete_block_ir();
    ir.model.features.push(complete_hole(
        BodyId::mint("test:model:entity#other".to_string()).expect("identity grammar"),
    ));

    assert_eq!(
        evaluate_saved_body_census(&ir),
        BodyCensusEvaluation::Unsupported {
            feature: FeatureBoundary {
                id: FeatureId::mint("synthetic:test:id#hole".to_string())
                    .expect("identity grammar"),
                name: None,
                family: Some("hole".to_string()),
                ordinal: 1
            },
            reason: UnsupportedBodyCensusReason::InvalidOutputLineage,
        }
    );
}

#[test]
fn replay_requires_dependencies_to_precede_their_consumers() {
    let mut ir = complete_block_ir();
    ir.model.features[0]
        .dependencies
        .insert(FeatureId::mint("synthetic:test:id#later".to_string()).expect("identity grammar"));

    assert_eq!(
        evaluate_saved_body_census(&ir),
        BodyCensusEvaluation::Unsupported {
            feature: FeatureBoundary {
                id: FeatureId::mint("synthetic:test:id#block".to_string())
                    .expect("identity grammar"),
                name: None,
                family: Some("block".to_string()),
                ordinal: 0
            },
            reason: UnsupportedBodyCensusReason::InvalidHistoryOrder,
        }
    );
}

#[test]
fn replay_uses_stable_feature_ordinals_independently_of_storage_order() {
    let mut ir = complete_block_ir();
    let body = ir.model.bodies[0].id.clone();
    ir.model.features.push(complete_hole(body.clone()));
    ir.model.features.reverse();

    assert_eq!(
        evaluate_saved_body_census(&ir),
        BodyCensusEvaluation::Verified { bodies: vec![body] }
    );
}

#[test]
fn replay_rejects_duplicate_feature_ordinals() {
    let mut ir = complete_block_ir();
    let body = ir.model.bodies[0].id.clone();
    let mut hole = complete_hole(body.clone());
    hole.ordinal = 0;
    ir.model.features.push(hole);

    assert_eq!(
        evaluate_saved_body_census(&ir),
        BodyCensusEvaluation::Unsupported {
            feature: FeatureBoundary {
                id: FeatureId::mint("synthetic:test:id#hole".to_string())
                    .expect("identity grammar"),
                name: None,
                family: Some("hole".to_string()),
                ordinal: 0
            },
            reason: UnsupportedBodyCensusReason::InvalidHistoryOrder,
        }
    );
}

#[test]
fn base_feature_introduces_its_complete_selected_outputs() {
    let mut ir = complete_block_ir();
    let body = ir.model.bodies[0].id.clone();
    ir.model.features[0]
        .evaluation
        .set_definition(FeatureDefinition::BaseFeature {
            bodies: BodySelection::Bodies(vec![body.clone()]),
        })
        .unwrap();

    assert_eq!(
        evaluate_saved_body_census(&ir),
        BodyCensusEvaluation::Verified { bodies: vec![body] }
    );
}

#[test]
fn extract_body_copies_each_existing_source_to_one_new_output() {
    let mut ir = complete_block_ir();
    let source = ir.model.bodies[0].id.clone();
    let extracted =
        BodyId::mint("test:model:entity#extracted".to_string()).expect("identity grammar");
    ir.model.bodies.push(model_body(extracted.as_str()));
    ir.model.features.push(Feature {
        id: FeatureId::mint("synthetic:test:id#extract".to_string()).expect("identity grammar"),
        ordinal: 1,
        name: None,
        suppressed: Some(false),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::new(
            FeatureDefinition::ExtractBody {
                source: BodySelection::Bodies(vec![source]),
            },
            vec![extracted.clone()],
        )
        .unwrap(),
        native_ref: None,
    });

    assert_eq!(
        evaluate_saved_body_census(&ir),
        BodyCensusEvaluation::Verified {
            bodies: vec![
                BodyId::mint("test:model:entity#body".to_string()).expect("identity grammar"),
                extracted
            ],
        }
    );
}

#[test]
fn output_free_local_extract_does_not_change_the_saved_body_census() {
    let mut ir = complete_block_ir();
    ir.model.features.push(Feature {
        id: FeatureId::mint("synthetic:test:id#extract-local".to_string())
            .expect("identity grammar"),
        ordinal: 1,
        name: None,
        suppressed: None,
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::ExtractBody {
                source: BodySelection::local(
                    vec!["nx:om-data-blocks-2:block#736".to_string()],
                    "nx:om-object-index#736".to_string(),
                )
                .unwrap(),
            },
        ),
        native_ref: None,
    });

    assert!(matches!(
        evaluate_saved_body_census(&ir),
        BodyCensusEvaluation::Verified { bodies }
            if bodies == [BodyId::mint("test:model:entity#body".to_string()).expect("identity grammar")]
    ));
}

#[test]
fn delete_body_removes_an_existing_selected_body() {
    let mut ir = complete_block_ir();
    let body = ir.model.bodies[0].id.clone();
    ir.model.features.push(Feature {
        id: FeatureId::mint("synthetic:test:id#delete".to_string()).expect("identity grammar"),
        ordinal: 1,
        name: None,
        suppressed: Some(false),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::DeleteBody {
                bodies: BodySelection::Bodies(vec![body]),
                mode: BodyRetentionMode::DeleteSelected,
            },
        ),
        native_ref: None,
    });
    ir.model.bodies.clear();

    assert_eq!(
        evaluate_saved_body_census(&ir),
        BodyCensusEvaluation::Verified { bodies: Vec::new() }
    );
}

#[test]
fn delete_body_ignores_a_complete_feature_local_body() {
    let mut ir = complete_block_ir();
    ir.model.features.push(Feature {
        id: FeatureId::mint("synthetic:test:id#delete-local".to_string())
            .expect("identity grammar"),
        ordinal: 1,
        name: None,
        suppressed: None,
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::DeleteBody {
                bodies: BodySelection::local(
                    vec!["input-body".to_string()],
                    "native-selection".to_string(),
                )
                .unwrap(),
                mode: BodyRetentionMode::DeleteSelected,
            },
        ),
        native_ref: None,
    });

    assert!(matches!(
        evaluate_saved_body_census(&ir),
        BodyCensusEvaluation::Verified { bodies }
            if bodies == [BodyId::mint("test:model:entity#body".to_string()).expect("identity grammar")]
    ));
}

#[test]
fn unresolved_suppression_of_a_resolved_delete_remains_a_boundary() {
    let mut ir = complete_block_ir();
    let body = ir.model.bodies[0].id.clone();
    ir.model.features.push(Feature {
        id: FeatureId::mint("synthetic:test:id#delete".to_string()).expect("identity grammar"),
        ordinal: 1,
        name: None,
        suppressed: None,
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::DeleteBody {
                bodies: BodySelection::Bodies(vec![body]),
                mode: BodyRetentionMode::DeleteSelected,
            },
        ),
        native_ref: None,
    });

    assert_eq!(
        evaluate_saved_body_census(&ir),
        BodyCensusEvaluation::Unsupported {
            feature: FeatureBoundary {
                id: FeatureId::mint("synthetic:test:id#delete".to_string())
                    .expect("identity grammar"),
                name: None,
                family: Some("delete_body".to_string()),
                ordinal: 1
            },
            reason: UnsupportedBodyCensusReason::UnresolvedSuppression,
        }
    );
}

#[test]
fn keep_selected_removes_every_unselected_body() {
    let mut ir = complete_block_ir();
    let retained = ir.model.bodies[0].id.clone();
    let removed = BodyId::mint("test:model:entity#removed".to_string()).expect("identity grammar");
    ir.model.features[0]
        .evaluation
        .try_edit(|_, outputs| {
            outputs.push(removed.clone());
        })
        .unwrap();
    ir.model.features[0]
        .evaluation
        .set_definition(FeatureDefinition::BaseFeature {
            bodies: BodySelection::Bodies(vec![retained.clone(), removed.clone()]),
        })
        .unwrap();
    ir.model.features.push(body_neutral_feature(
        "retain",
        1,
        FeatureDefinition::DeleteBody {
            bodies: BodySelection::Bodies(vec![retained.clone()]),
            mode: BodyRetentionMode::KeepSelected,
        },
    ));

    assert_eq!(
        evaluate_saved_body_census(&ir),
        BodyCensusEvaluation::Verified {
            bodies: vec![retained]
        }
    );
}
