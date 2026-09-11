use super::*;
use cadmpeg_ir::features::FeatureOperation;

#[test]
fn combine_consumes_tools_and_preserves_the_target_identity() {
    let mut ir = complete_block_ir();
    let target = ir.model.bodies[0].id.clone();
    let tool = BodyId::mint("test:model:entity#tool".to_string()).expect("identity grammar");
    ir.model.features[0].evaluation.edit(|_, outputs| {
        outputs.push(tool.clone());
    });
    ir.model.features[0]
        .evaluation
        .set_definition(FeatureDefinition::Operation(
            FeatureOperation::BaseFeature {
                bodies: BodySelection::Bodies(vec![target.clone(), tool.clone()]),
            },
        ));
    ir.model.features.push(body_preserving_feature(
        "combine",
        1,
        target.clone(),
        FeatureDefinition::Operation(FeatureOperation::Combine {
            operands: cadmpeg_ir::features::CombineOperands::new(
                BodySelection::Bodies(vec![target.clone()]),
                BodySelection::Bodies(vec![tool]),
            )
            .unwrap(),

            op: cadmpeg_ir::features::BooleanKind::Join,
            keep_tools: false,
        }),
    ));

    assert_eq!(
        evaluate_saved_body_census(&ir),
        BodyCensusEvaluation::Verified {
            bodies: vec![target]
        }
    );
}

#[test]
fn combine_preserves_tools_when_requested() {
    let mut ir = complete_block_ir();
    let target = ir.model.bodies[0].id.clone();
    let tool = BodyId::mint("test:model:entity#tool".to_string()).expect("identity grammar");
    ir.model.bodies.push(model_body(tool.as_str()));
    ir.model.features[0].evaluation.edit(|_, outputs| {
        outputs.push(tool.clone());
    });
    ir.model.features[0]
        .evaluation
        .set_definition(FeatureDefinition::Operation(
            FeatureOperation::BaseFeature {
                bodies: BodySelection::Bodies(vec![target.clone(), tool.clone()]),
            },
        ));
    ir.model.features.push(body_preserving_feature(
        "combine",
        1,
        target.clone(),
        FeatureDefinition::Operation(FeatureOperation::Combine {
            operands: cadmpeg_ir::features::CombineOperands::new(
                BodySelection::Bodies(vec![target.clone()]),
                BodySelection::Bodies(vec![tool.clone()]),
            )
            .unwrap(),

            op: cadmpeg_ir::features::BooleanKind::Join,
            keep_tools: true,
        }),
    ));

    assert_eq!(
        evaluate_saved_body_census(&ir),
        BodyCensusEvaluation::Verified {
            bodies: vec![target, tool]
        }
    );
}

#[test]
fn combine_with_exact_local_tools_preserves_its_retained_target() {
    let mut ir = complete_block_ir();
    let target = ir.model.bodies[0].id.clone();
    let mut combine = body_preserving_feature(
        "combine",
        1,
        target.clone(),
        FeatureDefinition::Operation(FeatureOperation::Combine {
            operands: cadmpeg_ir::features::CombineOperands::new(
                BodySelection::Bodies(vec![target.clone()]),
                BodySelection::local(vec!["local-tool".to_string()], "native-tools".to_string())
                    .unwrap(),
            )
            .unwrap(),

            op: cadmpeg_ir::features::BooleanKind::Cut,
            keep_tools: false,
        }),
    );
    combine.suppressed = None;
    ir.model.features.push(combine);

    assert_eq!(
        evaluate_saved_body_census(&ir),
        BodyCensusEvaluation::Verified {
            bodies: vec![target]
        }
    );
}

#[test]
fn output_free_combine_with_exact_native_operands_is_local_to_history() {
    let mut ir = complete_block_ir();
    let body = ir.model.bodies[0].id.clone();
    let mut combine = body_neutral_feature(
        "local-combine",
        1,
        FeatureDefinition::Operation(FeatureOperation::Combine {
            operands: cadmpeg_ir::features::CombineOperands::new(
                BodySelection::Native("native-target".to_string()),
                BodySelection::NativeSet(vec!["native-tool".to_string()].try_into().unwrap()),
            )
            .unwrap(),

            op: cadmpeg_ir::features::BooleanKind::Intersect,
            keep_tools: false,
        }),
    );
    combine.suppressed = None;
    ir.model.features.push(combine);

    assert_eq!(
        evaluate_saved_body_census(&ir),
        BodyCensusEvaluation::Verified { bodies: vec![body] }
    );
}

#[test]
fn trim_bodies_preserves_all_targets_and_tools() {
    let mut ir = complete_block_ir();
    let first = ir.model.bodies[0].id.clone();
    let second = BodyId::mint("test:model:entity#second".to_string()).expect("identity grammar");
    let tool = BodyId::mint("test:model:entity#tool".to_string()).expect("identity grammar");
    ir.model.bodies.push(model_body(second.as_str()));
    ir.model.bodies.push(model_body(tool.as_str()));
    ir.model.features[0]
        .evaluation
        .set_outputs(vec![first.clone(), second.clone(), tool.clone()]);
    ir.model.features[0]
        .evaluation
        .set_definition(FeatureDefinition::Operation(
            FeatureOperation::BaseFeature {
                bodies: BodySelection::Bodies(vec![first.clone(), second.clone(), tool.clone()]),
            },
        ));
    let mut trim = body_neutral_feature(
        "trim",
        1,
        FeatureDefinition::Operation(FeatureOperation::TrimBodies {
            operands: cadmpeg_ir::features::TrimBodyOperands::new(
                BodySelection::Bodies(vec![first.clone(), second.clone()]),
                BodySelection::Bodies(vec![tool.clone()]),
            )
            .unwrap(),

            keep: BodyTrimSide::Forward,
        }),
    );
    trim.evaluation
        .set_outputs(vec![first.clone(), second.clone()]);
    ir.model.features.push(trim);

    assert_eq!(
        evaluate_saved_body_census(&ir),
        BodyCensusEvaluation::Verified {
            bodies: vec![first, second, tool]
        }
    );
}

#[test]
fn trim_bodies_rejects_outputs_that_do_not_match_its_targets() {
    let mut ir = complete_block_ir();
    let target = ir.model.bodies[0].id.clone();
    let tool = BodyId::mint("test:model:entity#tool".to_string()).expect("identity grammar");
    ir.model.bodies.push(model_body(tool.as_str()));
    ir.model.features[0].evaluation.edit(|_, outputs| {
        outputs.push(tool.clone());
    });
    ir.model.features[0]
        .evaluation
        .set_definition(FeatureDefinition::Operation(
            FeatureOperation::BaseFeature {
                bodies: BodySelection::Bodies(vec![target.clone(), tool.clone()]),
            },
        ));
    ir.model.features.push(body_preserving_feature(
        "trim",
        1,
        tool.clone(),
        FeatureDefinition::Operation(FeatureOperation::TrimBodies {
            operands: cadmpeg_ir::features::TrimBodyOperands::new(
                BodySelection::Bodies(vec![target]),
                BodySelection::Bodies(vec![tool]),
            )
            .unwrap(),

            keep: BodyTrimSide::Reverse,
        }),
    ));

    assert_eq!(
        evaluate_saved_body_census(&ir),
        BodyCensusEvaluation::Unsupported {
            feature: FeatureBoundary {
                id: FeatureId::mint("synthetic:test:id#trim".to_string())
                    .expect("identity grammar"),
                name: None,
                family: Some("trim_bodies".to_string()),
                ordinal: 1
            },
            reason: UnsupportedBodyCensusReason::InvalidOutputLineage,
        }
    );
}

#[test]
fn trim_bodies_requires_a_resolved_retained_side_before_lineage() {
    let mut ir = complete_block_ir();
    let target = ir.model.bodies[0].id.clone();
    ir.model.features.push(body_preserving_feature(
        "trim",
        1,
        target.clone(),
        FeatureDefinition::Operation(FeatureOperation::TrimBodies {
            operands: cadmpeg_ir::features::TrimBodyOperands::new(
                BodySelection::Bodies(vec![target]),
                BodySelection::Bodies(vec![
                    BodyId::mint("test:model:entity#tool".to_string()).expect("identity grammar")
                ]),
            )
            .unwrap(),

            keep: BodyTrimSide::Unresolved,
        }),
    ));

    assert_eq!(
        evaluate_saved_body_census(&ir),
        BodyCensusEvaluation::Unsupported {
            feature: FeatureBoundary {
                id: FeatureId::mint("synthetic:test:id#trim".to_string())
                    .expect("identity grammar"),
                name: None,
                family: Some("trim_bodies".to_string()),
                ordinal: 1
            },
            reason: UnsupportedBodyCensusReason::IncompleteFeatureDefinition,
        }
    );
}

#[test]
fn output_free_trim_is_body_census_neutral_without_resolved_roles() {
    let mut ir = complete_block_ir();
    ir.model.features.push(Feature {
        id: FeatureId::mint("synthetic:test:id#trim-local".to_string()).expect("identity grammar"),
        ordinal: 1,
        name: None,
        suppressed: None,
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::TrimBodies {
                operands: cadmpeg_ir::features::TrimBodyOperands::new(
                    BodySelection::Unresolved,
                    BodySelection::Unresolved,
                )
                .unwrap(),

                keep: BodyTrimSide::Unresolved,
            }),
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
fn sew_replaces_all_inputs_with_its_declared_outputs() {
    let mut ir = complete_block_ir();
    let first = ir.model.bodies[0].id.clone();
    let second = BodyId::mint("test:model:entity#second".to_string()).expect("identity grammar");
    let sewn = BodyId::mint("test:model:entity#sewn".to_string()).expect("identity grammar");
    ir.model.bodies[0] = model_body(sewn.as_str());
    ir.model.features[0]
        .evaluation
        .set_outputs(vec![first.clone(), second.clone()]);
    ir.model.features[0]
        .evaluation
        .set_definition(FeatureDefinition::Operation(
            FeatureOperation::BaseFeature {
                bodies: BodySelection::Bodies(vec![first.clone(), second.clone()]),
            },
        ));
    let mut feature = body_preserving_feature(
        "sew",
        1,
        sewn.clone(),
        FeatureDefinition::Operation(FeatureOperation::SewBodies {
            bodies: (BodySelection::Bodies(vec![first, second]))
                .try_into()
                .unwrap(),
            gap_tolerance: None,
        }),
    );
    feature.evaluation.set_outputs(vec![sewn.clone()]);
    ir.model.features.push(feature);

    assert_eq!(
        evaluate_saved_body_census(&ir),
        BodyCensusEvaluation::Verified { bodies: vec![sewn] }
    );
}

#[test]
fn local_sew_with_an_already_retained_output_is_census_invariant() {
    let mut ir = complete_block_ir();
    let output = ir.model.bodies[0].id.clone();
    ir.model.features.push(Feature {
        id: FeatureId::mint("synthetic:test:id#sew-local".to_string()).expect("identity grammar"),
        ordinal: 1,
        name: None,
        suppressed: None,
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::new(
            FeatureDefinition::Operation(FeatureOperation::SewBodies {
                bodies: (BodySelection::local(
                    vec![
                        "historical-sheet".to_string(),
                        "second-historical-sheet".to_string(),
                    ],
                    "native-selection".to_string(),
                )
                .unwrap())
                .try_into()
                .unwrap(),
                gap_tolerance: None,
            }),
            vec![output.clone()],
        ),
        native_ref: None,
    });

    assert_eq!(
        evaluate_saved_body_census(&ir),
        BodyCensusEvaluation::Verified {
            bodies: vec![output]
        }
    );
}

#[test]
fn combine_rejects_a_tool_absent_from_prior_history() {
    let mut ir = complete_block_ir();
    let body = ir.model.bodies[0].id.clone();
    ir.model.features.push(body_preserving_feature(
        "combine",
        1,
        body.clone(),
        FeatureDefinition::Operation(FeatureOperation::Combine {
            operands: cadmpeg_ir::features::CombineOperands::new(
                BodySelection::Bodies(vec![body]),
                BodySelection::Bodies(vec![BodyId::mint("test:model:entity#missing".to_string())
                    .expect("identity grammar")]),
            )
            .unwrap(),

            op: cadmpeg_ir::features::BooleanKind::Cut,
            keep_tools: false,
        }),
    ));

    assert_eq!(
        evaluate_saved_body_census(&ir),
        BodyCensusEvaluation::Unsupported {
            feature: FeatureBoundary {
                id: FeatureId::mint("synthetic:test:id#combine".to_string())
                    .expect("identity grammar"),
                name: None,
                family: Some("combine".to_string()),
                ordinal: 1
            },
            reason: UnsupportedBodyCensusReason::InvalidOutputLineage,
        }
    );
}

#[test]
fn sew_rejects_an_output_identity_owned_by_an_unconsumed_body() {
    let mut ir = complete_block_ir();
    let first = ir.model.bodies[0].id.clone();
    let second = BodyId::mint("test:model:entity#second".to_string()).expect("identity grammar");
    let unrelated =
        BodyId::mint("test:model:entity#unrelated".to_string()).expect("identity grammar");
    ir.model.bodies = vec![model_body(unrelated.as_str())];
    ir.model.features[0].evaluation.set_outputs(vec![
        first.clone(),
        second.clone(),
        unrelated.clone(),
    ]);
    ir.model.features[0]
        .evaluation
        .set_definition(FeatureDefinition::Operation(
            FeatureOperation::BaseFeature {
                bodies: BodySelection::Bodies(vec![
                    first.clone(),
                    second.clone(),
                    unrelated.clone(),
                ]),
            },
        ));
    ir.model.features.push(body_preserving_feature(
        "sew",
        1,
        unrelated,
        FeatureDefinition::Operation(FeatureOperation::SewBodies {
            bodies: (BodySelection::Bodies(vec![first, second]))
                .try_into()
                .unwrap(),
            gap_tolerance: None,
        }),
    ));

    assert_eq!(
        evaluate_saved_body_census(&ir),
        BodyCensusEvaluation::Unsupported {
            feature: FeatureBoundary {
                id: FeatureId::mint("synthetic:test:id#sew".to_string()).expect("identity grammar"),
                name: None,
                family: Some("sew_bodies".to_string()),
                ordinal: 1
            },
            reason: UnsupportedBodyCensusReason::InvalidOutputLineage,
        }
    );
}

#[test]
fn native_body_selection_is_an_incomplete_semantic_boundary() {
    let mut ir = complete_block_ir();
    ir.model.features[0]
        .evaluation
        .set_definition(FeatureDefinition::Operation(
            FeatureOperation::BaseFeature {
                bodies: BodySelection::Native("selection".to_string()),
            },
        ));

    assert_eq!(
        evaluate_saved_body_census(&ir),
        BodyCensusEvaluation::Unsupported {
            feature: FeatureBoundary {
                id: FeatureId::mint("synthetic:test:id#block".to_string())
                    .expect("identity grammar"),
                name: None,
                family: Some("base_feature".to_string()),
                ordinal: 0
            },
            reason: UnsupportedBodyCensusReason::IncompleteFeatureDefinition,
        }
    );
}

#[test]
fn completed_history_reports_a_saved_body_census_mismatch() {
    let mut ir = complete_block_ir();
    ir.model.bodies.clear();

    assert_eq!(
        evaluate_saved_body_census(&ir),
        BodyCensusEvaluation::Mismatch {
            rederived: vec![
                BodyId::mint("test:model:entity#body".to_string()).expect("identity grammar")
            ],
            saved: Vec::new(),
        }
    );
}

#[test]
fn complete_chamfer_and_fillet_preserve_the_existing_body() {
    let mut ir = complete_block_ir();
    let body = ir.model.bodies[0].id.clone();
    ir.model.features.push(body_preserving_feature(
        "chamfer",
        1,
        body.clone(),
        FeatureDefinition::Operation(FeatureOperation::Chamfer {
            groups: cadmpeg_ir::features::NonEmptyMembers::one(ChamferGroup {
                edges: EdgeSelection::All,
                spec: ChamferSpec::Distance {
                    distance: cadmpeg_ir::scalar::PositiveLength::new(0.25).unwrap(),
                },
            }),
            flip_direction: false,
        }),
    ));
    ir.model.features.push(body_preserving_feature(
        "fillet",
        2,
        body.clone(),
        FeatureDefinition::Operation(FeatureOperation::Fillet {
            groups: cadmpeg_ir::features::NonEmptyMembers::one(FilletGroup {
                edges: EdgeSelection::All,
                radius: RadiusSpec::Constant {
                    radius: cadmpeg_ir::scalar::PositiveLength::new(0.2).unwrap(),
                },
                tangency_weight: Some(cadmpeg_ir::scalar::FiniteReal::new(1.0).unwrap()),
            }),
        }),
    ));

    assert_eq!(
        evaluate_saved_body_census(&ir),
        BodyCensusEvaluation::Verified { bodies: vec![body] }
    );
}

#[test]
fn incomplete_chamfer_construction_does_not_change_its_body_identity_effect() {
    let mut ir = complete_block_ir();
    let body = ir.model.bodies[0].id.clone();
    ir.model.features.push(body_preserving_feature(
        "chamfer",
        1,
        body.clone(),
        FeatureDefinition::Operation(FeatureOperation::Chamfer {
            groups: cadmpeg_ir::features::NonEmptyMembers::one(ChamferGroup {
                edges: EdgeSelection::Unresolved,
                spec: ChamferSpec::Distance {
                    distance: cadmpeg_ir::scalar::PositiveLength::new(0.25).unwrap(),
                },
            }),
            flip_direction: false,
        }),
    ));

    assert!(feature_completeness::chamfer_definition_is_incomplete(
        &ir.model.features[1]
    ));
    assert_eq!(
        evaluate_saved_body_census(&ir),
        BodyCensusEvaluation::Verified { bodies: vec![body] }
    );
}

#[test]
fn complete_single_body_dress_up_families_preserve_identity() {
    let mut ir = complete_block_ir();
    let body = ir.model.bodies[0].id.clone();
    let first = FaceSelection::Faces(vec![
        FaceId::mint("test:model:entity#first".to_string()).expect("identity grammar")
    ]);
    let second = FaceSelection::Faces(vec![
        FaceId::mint("test:model:entity#second".to_string()).expect("identity grammar")
    ]);
    let definitions = [
        FeatureDefinition::Operation(FeatureOperation::FaceBlend {
            operands: cadmpeg_ir::features::FaceBlendOperands::new(first.clone(), second.clone())
                .unwrap(),

            radius: RadiusSpec::Constant {
                radius: cadmpeg_ir::scalar::PositiveLength::new(0.2).unwrap(),
            },
        }),
        FeatureDefinition::Operation(FeatureOperation::OffsetSurface {
            faces: first.clone(),
            distance: Some(Length::new(0.1).unwrap()),
        }),
        FeatureDefinition::Operation(FeatureOperation::Thicken {
            faces: first.clone(),
            thickness: Some(cadmpeg_ir::scalar::PositiveLength::new(0.3).unwrap()),
            side: Some(ThickenSide::Forward),
        }),
        FeatureDefinition::Operation(FeatureOperation::Draft {
            faces: first.clone(),
            anchor: cadmpeg_ir::features::DraftAnchor::NeutralPlane {
                plane: second.clone(),
                pull: Some(cadmpeg_ir::features::DraftPull {
                    direction: cadmpeg_ir::features::FeatureDirection3::new(Vector3::new(
                        0.0, 0.0, 1.0,
                    ))
                    .unwrap(),
                    plane: None,
                }),
            },
            angle: Some(cadmpeg_ir::scalar::SlopeAngle::new(0.1).unwrap()),
            outward: Some(false),
        }),
        FeatureDefinition::Operation(FeatureOperation::ReplaceFace {
            operands: cadmpeg_ir::features::ReplaceFaceOperands::new(first, second).unwrap(),
        }),
    ];
    for (index, definition) in definitions.into_iter().enumerate() {
        ir.model.features.push(body_preserving_feature(
            &format!("dress-up-{index}"),
            u64::try_from(index).expect("five dress-up fixtures") + 1,
            body.clone(),
            definition,
        ));
    }

    assert_eq!(
        evaluate_saved_body_census(&ir),
        BodyCensusEvaluation::Verified { bodies: vec![body] }
    );
}

#[test]
fn complete_surface_edits_preserve_every_declared_body_identity() {
    let mut ir = complete_block_ir();
    let first = ir.model.bodies[0].id.clone();
    let second = BodyId::mint("test:model:entity#second".to_string()).expect("identity grammar");
    ir.model.bodies.push(model_body(second.as_str()));
    ir.model.features[0].evaluation.edit(|_, outputs| {
        outputs.push(second.clone());
    });
    ir.model.features[0]
        .evaluation
        .set_definition(FeatureDefinition::Operation(
            FeatureOperation::BaseFeature {
                bodies: BodySelection::Bodies(vec![first.clone(), second.clone()]),
            },
        ));
    let faces = FaceSelection::Faces(vec![
        FaceId::mint("test:model:entity#face".to_string()).expect("identity grammar")
    ]);
    let mut trim = body_neutral_feature(
        "trim-surface",
        1,
        FeatureDefinition::Operation(FeatureOperation::TrimSurface {
            faces: faces.clone(),
            tool: PathRef::Curves(vec![CurveId::mint(
                "test:model:entity#trim-curve".to_string(),
            )
            .expect("identity grammar")]),
            keep: TrimRegion::Inside,
        }),
    );
    trim.evaluation
        .set_outputs(vec![first.clone(), second.clone()]);
    let mut extend = body_neutral_feature(
        "extend-surface",
        2,
        FeatureDefinition::Operation(FeatureOperation::ExtendSurface {
            faces,
            distance: Some(cadmpeg_ir::scalar::PositiveLength::new(0.5).unwrap()),
            method: SurfaceExtension::Natural,
        }),
    );
    extend
        .evaluation
        .set_outputs(vec![first.clone(), second.clone()]);
    ir.model.features.extend([trim, extend]);

    assert_eq!(
        evaluate_saved_body_census(&ir),
        BodyCensusEvaluation::Verified {
            bodies: vec![first, second]
        }
    );
}

#[test]
fn output_free_surface_edits_are_body_identity_neutral() {
    let definitions = [
        FeatureDefinition::Operation(FeatureOperation::TrimSurface {
            faces: FaceSelection::Unresolved,
            tool: PathRef::Unresolved("trim".to_string()),
            keep: TrimRegion::Unresolved,
        }),
        FeatureDefinition::Operation(FeatureOperation::ExtendSurface {
            faces: FaceSelection::Unresolved,
            distance: None,
            method: SurfaceExtension::Unresolved,
        }),
    ];
    for (ordinal, definition) in definitions.into_iter().enumerate() {
        let mut ir = complete_block_ir();
        ir.model.features.push(body_neutral_feature(
            "surface-edit",
            u64::try_from(ordinal).expect("two surface edit families") + 1,
            definition,
        ));
        assert!(match ir.model.features[1].evaluation.definition() {
            FeatureDefinition::Operation(FeatureOperation::TrimSurface { .. }) => {
                feature_completeness::trim_surface_definition_is_incomplete(&ir.model.features[1])
            }
            FeatureDefinition::Operation(FeatureOperation::ExtendSurface { .. }) => {
                feature_completeness::extend_surface_definition_is_incomplete(&ir.model.features[1])
            }
            _ => unreachable!("surface edit fixture"),
        });
        assert_eq!(
            evaluate_saved_body_census(&ir),
            BodyCensusEvaluation::Verified {
                bodies: vec![ir.model.bodies[0].id.clone()],
            }
        );
    }
}

#[test]
fn output_free_unresolved_loft_is_body_census_neutral() {
    let mut ir = complete_block_ir();
    ir.model.features.push(Feature {
        id: FeatureId::mint("synthetic:test:id#loft".to_string()).expect("identity grammar"),
        ordinal: 1,
        name: None,
        suppressed: None,
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Unresolved {
                family: UnresolvedFamily::Loft,
            }),
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
fn output_free_unresolved_freeform_surface_is_body_census_neutral() {
    let mut ir = complete_block_ir();
    ir.model.features.push(Feature {
        id: FeatureId::mint("synthetic:test:id#freeform".to_string()).expect("identity grammar"),
        ordinal: 1,
        name: None,
        suppressed: None,
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Unresolved {
                family: UnresolvedFamily::FreeformSurface,
            }),
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
fn complete_surface_edit_rejects_an_output_absent_from_prior_history() {
    let mut ir = complete_block_ir();
    let missing = BodyId::mint("test:model:entity#missing".to_string()).expect("identity grammar");
    let mut trim = body_neutral_feature(
        "trim-surface",
        1,
        FeatureDefinition::Operation(FeatureOperation::TrimSurface {
            faces: FaceSelection::Faces(vec![
                FaceId::mint("test:model:entity#face".to_string()).expect("identity grammar")
            ]),
            tool: PathRef::Curves(vec![CurveId::mint(
                "test:model:entity#trim-curve".to_string(),
            )
            .expect("identity grammar")]),
            keep: TrimRegion::Outside,
        }),
    );
    trim.evaluation.set_outputs(vec![missing]);
    ir.model.features.push(trim);

    assert_eq!(
        evaluate_saved_body_census(&ir),
        BodyCensusEvaluation::Unsupported {
            feature: FeatureBoundary {
                id: FeatureId::mint("synthetic:test:id#trim-surface".to_string())
                    .expect("identity grammar"),
                name: None,
                family: Some("trim_surface".to_string()),
                ordinal: 1
            },
            reason: UnsupportedBodyCensusReason::InvalidOutputLineage,
        }
    );
}

#[test]
fn unresolved_suppression_is_irrelevant_to_output_free_construction() {
    let mut ir = CadIr::empty();
    ir.model.features.push(Feature {
        id: FeatureId::mint("synthetic:test:id#datum".to_string()).expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Unresolved {
                family: UnresolvedFamily::DatumCoordinateSystem,
            }),
        ),
        native_ref: None,
    });

    assert_eq!(
        evaluate_saved_body_census(&ir),
        BodyCensusEvaluation::Verified { bodies: Vec::new() }
    );
}

#[test]
fn output_free_boolean_construction_has_no_retained_body_effect() {
    let mut ir = complete_block_ir();
    let body = ir.model.bodies[0].id.clone();
    let mut extrude = complete_extrude_feature(
        "transient-extrude",
        1,
        FeatureId::mint("synthetic:test:id#unresolved-profile".to_string())
            .expect("identity grammar"),
        Vec::new(),
        BooleanOp::NewBody,
    );
    extrude.suppressed = None;
    extrude.dependencies.clear();
    extrude.source_properties.insert(
        "primary_body_reference".to_string(),
        "reference".to_string(),
    );
    ir.model.features.push(extrude);

    assert_eq!(
        evaluate_saved_body_census(&ir),
        BodyCensusEvaluation::Verified { bodies: vec![body] }
    );
}

#[test]
fn output_free_fset_is_body_census_neutral() {
    let mut ir = complete_block_ir();
    let body = ir.model.bodies[0].id.clone();
    let mut fset = body_neutral_feature(
        "fset",
        1,
        FeatureDefinition::Operation(FeatureOperation::Native {
            kind: "FSET".into(),
            parameters: BTreeMap::new(),
        }),
    );
    fset.suppressed = None;
    ir.model.features.push(fset);

    assert_eq!(
        evaluate_saved_body_census(&ir),
        BodyCensusEvaluation::Verified { bodies: vec![body] }
    );
}

#[test]
fn output_free_native_snapshot_is_local_to_history() {
    let mut ir = complete_block_ir();
    let body = ir.model.bodies[0].id.clone();
    let mut snapshot = body_neutral_feature(
        "snapshot",
        1,
        FeatureDefinition::Operation(FeatureOperation::BaseFeature {
            bodies: BodySelection::Unresolved,
        }),
    );
    snapshot.name = Some("MASTER SNAPSHOT BODY".to_string());
    snapshot.suppressed = None;
    snapshot
        .source_properties
        .insert("operation_record".to_string(), "native-record".to_string());
    ir.model.features.push(snapshot);

    assert_eq!(
        evaluate_saved_body_census(&ir),
        BodyCensusEvaluation::Verified { bodies: vec![body] }
    );
}

#[test]
fn unresolved_suppression_still_blocks_a_body_effect() {
    let mut ir = complete_block_ir();
    ir.model.features[0].suppressed = None;

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
            reason: UnsupportedBodyCensusReason::UnresolvedSuppression,
        }
    );
}

#[test]
fn unresolved_suppression_does_not_block_a_complete_in_place_edit() {
    let mut ir = complete_block_ir();
    let body = ir.model.bodies[0].id.clone();
    let mut hole = complete_hole(body.clone());
    hole.suppressed = None;
    ir.model.features.push(hole);

    assert_eq!(
        evaluate_saved_body_census(&ir),
        BodyCensusEvaluation::Verified { bodies: vec![body] }
    );
}

#[test]
fn output_free_hole_is_body_identity_neutral_regardless_of_suppression() {
    let mut ir = complete_block_ir();
    let body = ir.model.bodies[0].id.clone();
    let mut hole = complete_hole(body.clone());
    hole.suppressed = None;
    hole.evaluation.edit(|definition, _| {
        if let FeatureDefinition::Operation(FeatureOperation::Hole { placements, .. }) = definition
        {
            *placements = None;
        }
    });
    hole.evaluation.edit(|_, outputs| {
        outputs.clear();
    });
    ir.model.features.push(hole);

    assert_eq!(
        evaluate_saved_body_census(&ir),
        BodyCensusEvaluation::Verified { bodies: vec![body] }
    );
}

#[test]
fn native_delete_without_a_primary_body_is_body_neutral() {
    let mut ir = CadIr::empty();
    let mut deletion = Feature {
        id: FeatureId::mint("synthetic:test:id#delete".to_string()).expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Native {
                kind: "DELETE".into(),
                parameters: BTreeMap::new(),
            }),
        ),
        native_ref: None,
    };
    ir.model.features.push(deletion.clone());

    assert_eq!(
        evaluate_saved_body_census(&ir),
        BodyCensusEvaluation::Verified { bodies: Vec::new() }
    );

    deletion
        .source_properties
        .insert("primary_body_object_index".to_string(), "7".to_string());
    ir.model.features[0] = deletion;
    assert_eq!(
        evaluate_saved_body_census(&ir),
        BodyCensusEvaluation::Unsupported {
            feature: FeatureBoundary {
                id: FeatureId::mint("synthetic:test:id#delete".to_string())
                    .expect("identity grammar"),
                name: None,
                family: Some("native".to_string()),
                ordinal: 0
            },
            reason: UnsupportedBodyCensusReason::UnresolvedSuppression,
        }
    );
}
