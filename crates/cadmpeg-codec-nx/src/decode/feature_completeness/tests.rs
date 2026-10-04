// SPDX-License-Identifier: Apache-2.0
//! Feature-completeness predicates owned by `decode`.

#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]

use crate::decode::feature_completeness::operands::revolve_feature_is_incomplete;
use crate::decode::feature_completeness::{
    datum_coordinate_system_is_incomplete, projected_curve_direction_is_incomplete,
};

fn append_design_intent_losses(
    ir: &cadmpeg_ir::document::CadIr,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) {
    crate::test_support::with_decode_context(|ctx| {
        crate::decode::report::append_design_intent_losses(ctx, ir, losses)
    })
    .unwrap();
}

fn one_incomplete_expression_parameter() -> cadmpeg_ir::CadIr {
    use cadmpeg_ir::features::{DesignParameter, ParameterId};

    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.model.parameters.push(DesignParameter {
        id: ParameterId::mint("test:model:parameter#incomplete")
            .expect("parameter identity grammar"),
        owner: None,
        ordinal: 0,
        name: "p1".into(),
        expression: "p2".into(),
        display: None,
        value: None,
        dependencies: Default::default(),
        properties: Default::default(),
        pmi: None,
        native_ref: None,
    });
    ir
}

#[test]
fn expression_completeness_refuses_owner_collection_limit() {
    use cadmpeg_core::decode::ResourceDimension;

    let ir = one_incomplete_expression_parameter();

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            policy.limits.max_collection_items = 0;
        },
        |ctx| {
            let result = super::incomplete_expression_parameters(ctx, &ir);
            let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = result else {
                panic!("expected expression collection refusal");
            };
            assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
            assert_eq!(limit.operation, "nx expression parameter owners");
        },
    );
}

#[test]
fn expression_completeness_refuses_incomplete_identity_retention() {
    use cadmpeg_core::decode::ResourceDimension;

    let ir = one_incomplete_expression_parameter();

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            policy.limits.max_retained_bytes = 0;
        },
        |ctx| {
            let result = super::incomplete_expression_parameters(ctx, &ir);
            let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = result else {
                panic!("expected expression identity refusal");
            };
            assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
            assert_eq!(limit.operation, "nx incomplete expression identity");
        },
    );
}

#[test]
fn nx_datum_completeness_requires_coherent_finite_frames() {
    use cadmpeg_ir::math::Vector3;
    use cadmpeg_ir::units::UnitVector3;

    let unit = |x, y, z| UnitVector3::new(Vector3::new(x, y, z));
    let x_axis = UnitVector3::X_AXIS;
    let y_axis = UnitVector3::Y_AXIS;
    let z_axis = UnitVector3::Z_AXIS;

    assert!(!datum_coordinate_system_is_incomplete(
        x_axis, y_axis, z_axis,
    ));
    assert!(datum_coordinate_system_is_incomplete(
        x_axis,
        y_axis,
        unit(0.0, 0.0, -1.0).unwrap(),
    ));
    assert!(unit(2.0, 0.0, 0.0).is_none());
    assert!(datum_coordinate_system_is_incomplete(
        x_axis,
        unit(1.0e-6, 1.0, 0.0).unwrap(),
        z_axis,
    ));
}

#[test]
fn nx_projected_curve_completeness_requires_a_valid_direction_law() {
    use cadmpeg_ir::features::{CurveProjectionDirection, CurveProjectionDirectionState};
    use cadmpeg_ir::math::Vector3;

    assert!(!projected_curve_direction_is_incomplete(
        CurveProjectionDirection::State(CurveProjectionDirectionState::TargetNormal),
    ));
    assert!(!projected_curve_direction_is_incomplete(
        CurveProjectionDirection::Vector(
            cadmpeg_ir::features::FeatureDirection3::new(Vector3::new(0.0, 0.0, 1.0)).unwrap()
        ),
    ));
    assert!(projected_curve_direction_is_incomplete(
        CurveProjectionDirection::State(CurveProjectionDirectionState::Unresolved),
    ));
}

#[test]
fn nx_loft_completeness_checks_native_point_sections_and_centerlines() {
    use cadmpeg_ir::features::{
        BooleanOp, Feature, FeatureDefinition, FeatureId, FeatureOperation, LoftPointSection,
        LoftSection, PathRef,
    };
    use cadmpeg_ir::math::Point3;

    let mut ir = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    let output = ir.model.bodies[0].id.clone();
    let definition = |sections, centerline: Option<PathRef>| {
        FeatureDefinition::Operation(FeatureOperation::Loft {
            sections,
            guidance: centerline.map_or_else(
                || cadmpeg_ir::features::LoftGuidance::Guides(Vec::new()),
                cadmpeg_ir::features::LoftGuidance::Centerline,
            ),
            op: BooleanOp::NewBody,
            closed: false,
            solid: true,
            ruled: false,
            linearize: false,
            max_degree: None,
            allow_multi_profile_faces: None,
        })
    };
    ir.model.features.push(Feature {
        id: FeatureId::mint("test:test:feature#loft").expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Default::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::new(
            definition(
                vec![
                    LoftSection::Point(LoftPointSection::Point(
                        cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0))
                            .unwrap(),
                    )),
                    LoftSection::Point(LoftPointSection::Point(
                        cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 1.0))
                            .unwrap(),
                    )),
                ],
                Some(PathRef::Native("nx:centerline#0".into())),
            ),
            cadmpeg_ir::features::DistinctMembers::try_from(
                vec![output],
                &cadmpeg_test_support::service_decode_context(),
            )
            .unwrap(),
        ),
        native_ref: None,
    });

    let mut losses = Vec::new();
    append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("loft (1)"));

    ir.model.features[0].evaluation.set_definition(definition(
        vec![
            LoftSection::Point(LoftPointSection::Native(
                cadmpeg_core::text::NonBlankString::try_from("nx:point#0").unwrap(),
            )),
            LoftSection::Point(LoftPointSection::Point(
                cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 1.0)).unwrap(),
            )),
        ],
        None,
    ));
    losses.clear();
    append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("loft (1)"));

    ir.model.features[0].evaluation.set_definition(definition(
        vec![
            LoftSection::Point(LoftPointSection::Point(
                cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).unwrap(),
            )),
            LoftSection::Point(LoftPointSection::Point(
                cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 1.0)).unwrap(),
            )),
        ],
        None,
    ));
    losses.clear();
    append_design_intent_losses(&ir, &mut losses);
    assert!(losses.is_empty());
}

#[test]
fn nx_extrude_completeness_requires_direction_start_and_solid_state() {
    use cadmpeg_ir::features::{
        BooleanOp, ExtrudeDirection, ExtrudeExtent, ExtrudeSide, ExtrudeStart, Feature,
        FeatureDefinition, FeatureId, FeatureOperation, FeatureResultTopology, LinearTermination,
        PlanarProfileRef, ProfileRef,
    };
    use cadmpeg_ir::ids::FeatureResultTopologyId;

    let mut ir = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    let output = ir.model.bodies[0].id.clone();
    let definition = |direction, start, solid| {
        FeatureDefinition::Operation(FeatureOperation::Extrude {
            profile: ProfileRef::Planar(PlanarProfileRef::Sketch(
                cadmpeg_ir::sketches::SketchId::mint("test:test:sketch#0").unwrap(),
            )),
            direction,
            start,
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: LinearTermination::Blind {
                        length: cadmpeg_ir::scalar::NonZeroLength::new(5.0).unwrap(),
                    },
                    draft: None,
                },
            },
            op: BooleanOp::NewBody,
            solid,
            face_maker: None,
            inner_wire_taper: None,
            length_along_profile_normal: None,
            allow_multi_profile_faces: None,
        })
    };
    let complete = definition(
        ExtrudeDirection::ProfileNormal {},
        ExtrudeStart::ProfilePlane {},
        Some(true),
    );
    ir.model.features.push(Feature {
        id: FeatureId::mint("test:test:feature#extrude").expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Default::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::new(
            complete.clone(),
            cadmpeg_ir::features::DistinctMembers::try_from(
                vec![output],
                &cadmpeg_test_support::service_decode_context(),
            )
            .unwrap(),
        ),
        native_ref: None,
    });

    let mut losses = Vec::new();
    append_design_intent_losses(&ir, &mut losses);
    assert!(losses.is_empty());

    for incomplete in [
        definition(
            ExtrudeDirection::Unresolved {},
            ExtrudeStart::ProfilePlane {},
            Some(true),
        ),
        definition(
            ExtrudeDirection::ProfileNormal {},
            ExtrudeStart::Unresolved {},
            Some(true),
        ),
        definition(
            ExtrudeDirection::ProfileNormal {},
            ExtrudeStart::ProfilePlane {},
            None,
        ),
    ] {
        ir.model.features[0].evaluation.set_definition(incomplete);
        losses.clear();
        append_design_intent_losses(&ir, &mut losses);
        assert_eq!(losses.len(), 1);
        assert!(losses[0].message.contains("extrude (1)"));
    }

    ir.model.features[0].evaluation.set_definition(complete);
    ir.model.features[0].evaluation.edit(|_, outputs| {
        outputs.clear();
    });
    losses.clear();
    append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("extrude (1)"));

    ir.model.feature_result_topologies.push(
        cadmpeg_ir::features::FeatureResultMembers::new(
            vec![cadmpeg_core::nonblank_literal!("test:feature-local-body#0")],
            Vec::new(),
            Vec::new(),
            Vec::new(),
            &cadmpeg_test_support::service_decode_context(),
            "validate feature result members",
        )
        .expect("result membership admission")
        .map(|members| {
            FeatureResultTopology::new(
                FeatureResultTopologyId::mint("test:model:feature-result#extrude")
                    .expect("identity grammar"),
                ir.model.features[0].id.clone(),
                members,
                Some("test:native-body-writer#0".into()),
            )
        })
        .unwrap(),
    );
    losses.clear();
    append_design_intent_losses(&ir, &mut losses);
    assert!(losses.is_empty());
}

#[test]
fn nx_revolve_completeness_checks_construction_and_output_lineage() {
    use cadmpeg_ir::features::{
        AngularTermination, BooleanOp, Feature, FeatureDefinition, FeatureId, FeatureOperation,
        GeneratedVertexRef, PathRef, RevolutionAxis, RevolveConstruction, RevolveExtent,
        VertexSelection,
    };
    use cadmpeg_ir::math::{Point3, Vector3};

    let mut ir = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    let output = ir.model.bodies[0].id.clone();
    let face = ir.model.faces[0].id.clone();
    let complete = RevolveConstruction::Resolved {
        profile: cadmpeg_ir::features::PlanarProfileRef::Faces(vec![face]),
        axis: RevolutionAxis {
            origin: cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).unwrap(),
            direction: cadmpeg_ir::features::FeatureDirection3::new(Vector3::new(0.0, 0.0, 1.0))
                .unwrap(),
            reference: None,
        },
        extent: RevolveExtent::OneSided {
            termination: AngularTermination::Angle {
                angle: cadmpeg_ir::scalar::PositiveAngle::new(1.0).unwrap(),
            },
        },
        solid: Some(true),
        face_maker: None,
        fuse_order: None,
        allow_multi_profile_faces: None,
    };
    assert!(!revolve_feature_is_incomplete(
        &complete,
        BooleanOp::NewBody,
        &[],
    ));
    assert_eq!(
        FeatureDefinition::Operation(FeatureOperation::Revolve {
            construction: complete.clone(),
            op: BooleanOp::NewBody,
        })
        .body_output_family(),
        Some("revolve"),
    );

    let mut incomplete = complete.clone();
    incomplete.set_profile(None);
    assert!(revolve_feature_is_incomplete(
        &incomplete,
        BooleanOp::NewBody,
        &[],
    ));
    incomplete = complete.clone();
    incomplete.set_axis(None);
    assert!(revolve_feature_is_incomplete(
        &incomplete,
        BooleanOp::NewBody,
        &[],
    ));
    let mut non_unit_axis = complete.clone();
    non_unit_axis.axis_mut().unwrap().direction =
        cadmpeg_ir::features::FeatureDirection3::new(Vector3::new(0.0, 0.0, 2.0)).unwrap();
    assert!(!revolve_feature_is_incomplete(
        &non_unit_axis,
        BooleanOp::NewBody,
        &[],
    ));
    incomplete = complete.clone();
    let RevolveConstruction::Resolved {
        profile,
        axis,
        solid,
        face_maker,
        fuse_order,
        allow_multi_profile_faces,
        ..
    } = incomplete
    else {
        panic!("resolved fixture")
    };
    incomplete =
        RevolveConstruction::Unresolved(cadmpeg_ir::features::PartialRevolveConstruction::Extent {
            profile,
            axis,
            solid,
            face_maker,
            fuse_order,
            allow_multi_profile_faces,
        });
    assert!(revolve_feature_is_incomplete(
        &incomplete,
        BooleanOp::NewBody,
        &[],
    ));
    incomplete = complete.clone();
    incomplete.axis_mut().unwrap().reference = Some(PathRef::Native("test:axis".into()));
    assert!(revolve_feature_is_incomplete(
        &incomplete,
        BooleanOp::NewBody,
        &[],
    ));
    incomplete = complete.clone();
    let RevolveConstruction::Resolved { solid, .. } = &mut incomplete else {
        panic!("resolved fixture")
    };
    *solid = None;
    assert!(revolve_feature_is_incomplete(
        &incomplete,
        BooleanOp::NewBody,
        &[],
    ));
    let source = FeatureId::mint("test:test:feature#vertex-source").expect("identity grammar");
    incomplete = complete.clone();
    *incomplete.extent_mut().expect("fixture extent") = RevolveExtent::OneSided {
        termination: AngularTermination::ToVertex {
            vertex: VertexSelection::generated(
                GeneratedVertexRef::new(
                    source.clone(),
                    "vertex-0".into(),
                    &cadmpeg_test_support::service_decode_context(),
                )
                .expect("selection reference admission")
                .unwrap(),
                "test:vertex-selection".into(),
                &cadmpeg_test_support::service_decode_context(),
            )
            .expect("selection reference admission")
            .unwrap(),
        },
    };
    assert!(revolve_feature_is_incomplete(
        &incomplete,
        BooleanOp::NewBody,
        &[],
    ));
    assert!(!revolve_feature_is_incomplete(
        &incomplete,
        BooleanOp::NewBody,
        &[source],
    ));
    assert!(revolve_feature_is_incomplete(
        &complete,
        BooleanOp::Unresolved,
        &[],
    ));

    ir.model.features.push(Feature {
        id: FeatureId::mint("test:test:feature#revolve").expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Default::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::new(
            FeatureDefinition::Operation(FeatureOperation::Revolve {
                construction: complete,
                op: BooleanOp::NewBody,
            }),
            cadmpeg_ir::features::DistinctMembers::try_from(
                vec![output],
                &cadmpeg_test_support::service_decode_context(),
            )
            .unwrap(),
        ),
        native_ref: None,
    });
    let mut losses = Vec::new();
    append_design_intent_losses(&ir, &mut losses);
    assert!(losses.is_empty());

    ir.model.features[0].evaluation.edit(|_, outputs| {
        outputs.clear();
    });
    append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("revolve (1)"));
}

#[test]
fn nx_sketch_completeness_reports_native_geometry_and_constraints() {
    use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId, FeatureOperation};
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::sketches::{
        Sketch, SketchConstraint, SketchConstraintDefinitionInput, SketchConstraintId,
        SketchEntity, SketchEntityId, SketchGeometry, SketchId,
    };

    let mut ir = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    let sketch_id = SketchId::mint("test:test:sketch#0").unwrap();
    ir.model.features.push(Feature {
        id: FeatureId::mint("test:test:feature#sketch").expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Default::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Sketch {
                sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(sketch_id.clone())),
            }),
        ),
        native_ref: None,
    });
    ir.model.sketches.push(Sketch {
        id: sketch_id.clone(),
        name: None,
        configuration: None,
        visible: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::try_resolved(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
        profiles: Default::default(),
        native_ref: None,
    });
    let entity_id = SketchEntityId::mint("test:test:sketch-entity#0").unwrap();
    ir.model.sketch_entities.push(SketchEntity::new(
        entity_id.clone(),
        sketch_id.clone(),
        SketchGeometry::native(
            cadmpeg_core::text::NonBlankString::try_from("test").expect("nonempty source identity"),
        ),
    ));
    ir.model.sketch_constraints.push(SketchConstraint {
        id: SketchConstraintId::mint("test:test:sketch-constraint#0").unwrap(),
        sketch: sketch_id,
        definition: cadmpeg_ir::sketches::SketchConstraintDefinition::try_from(
            SketchConstraintDefinitionInput::Native {
                native_kind: cadmpeg_core::text::NonBlankString::try_from("test").unwrap(),
                entities: vec![entity_id],
                parameter: None,
                operands: Vec::new(),
                native_state: None,
                native_flags: None,
                native_properties: std::collections::BTreeMap::new(),
            },
        )
        .unwrap(),
        name: None,
        driving: None,
        active: None,
        virtual_space: None,
        visible: None,
        orientation: None,
        label_distance: None,
        label_position: None,
        metadata: None,
        native_ref: None,
    });

    let mut losses = Vec::new();
    append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0]
        .message
        .contains("1 NX sketch geometry record(s) and 1 sketch constraint"));
}

#[test]
fn nx_configuration_completeness_requires_one_active_full_body_set() {
    use cadmpeg_ir::features::{
        BodySelection, ConfigurationFeatureState, ConfigurationId, DesignConfiguration,
        DesignParameter, Feature, FeatureDefinition, FeatureId, FeatureOperation, ParameterId,
        ParameterValue,
    };

    let mut ir = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    let bodies = ir
        .model
        .bodies
        .iter()
        .map(|body| body.id.clone())
        .collect::<Vec<_>>();
    ir.model.configurations.push(DesignConfiguration {
        id: ConfigurationId::mint("test:test:configuration#0").expect("identity grammar"),
        ordinal: 0,
        active: true,
        source_index: Some(0),
        name: Some("Model".to_string()),
        material: None,
        properties: Default::default(),
        parameter_overrides: Default::default(),
        bodies: Some(Default::default()),
        parameter_values: Default::default(),
        feature_states: Default::default(),
        native_ref: None,
    });

    let mut losses = Vec::new();
    append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("1 NX design configuration"));

    ir.model.configurations[0].bodies = Some(
        cadmpeg_ir::features::DistinctMembers::try_from(
            bodies,
            &cadmpeg_test_support::service_decode_context(),
        )
        .unwrap(),
    );
    losses.clear();
    append_design_intent_losses(&ir, &mut losses);
    assert!(losses.is_empty());

    ir.model.configurations[0].active = false;
    losses.clear();
    append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("1 NX design configuration"));

    ir.model.configurations[0].active = true;
    let output = ir.model.bodies[0].id.clone();
    let feature = Feature {
        id: FeatureId::mint("test:test:feature#base").expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Default::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::new(
            FeatureDefinition::Operation(FeatureOperation::BaseFeature {
                bodies: BodySelection::Bodies(
                    cadmpeg_ir::features::DistinctMembers::try_from(
                        vec![output.clone()],
                        &cadmpeg_test_support::service_decode_context(),
                    )
                    .expect("distinct bodies"),
                ),
            }),
            cadmpeg_ir::features::DistinctMembers::try_from(
                vec![output.clone()],
                &cadmpeg_test_support::service_decode_context(),
            )
            .unwrap(),
        ),
        native_ref: None,
    };
    ir.model.features.push(feature.clone());
    losses.clear();
    append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("1 NX design configuration"));

    ir.model.configurations[0].feature_states.insert(
        feature.id.clone(),
        ConfigurationFeatureState {
            evaluation: cadmpeg_ir::features::ConfigurationEvaluation::Active {
                outputs: cadmpeg_ir::features::DistinctMembers::try_from(
                    feature.evaluation.outputs().clone(),
                    &cadmpeg_test_support::service_decode_context(),
                )
                .unwrap(),
            },
            dependencies: feature.dependencies.clone(),
            definition: feature.evaluation.definition().clone(),
        },
    );
    losses.clear();
    append_design_intent_losses(&ir, &mut losses);
    assert!(losses.is_empty());

    let parameter = DesignParameter {
        id: ParameterId::mint("test:test:parameter#length").expect("identity grammar"),
        owner: Some(feature.id),
        ordinal: 0,
        name: "length".into(),
        expression: "2".into(),
        display: None,
        value: Some(ParameterValue::Real(
            cadmpeg_ir::scalar::FiniteReal::new(2.0).unwrap(),
        )),
        dependencies: Default::default(),
        properties: Default::default(),
        pmi: None,
        native_ref: None,
    };
    ir.model.parameters.push(parameter.clone());
    losses.clear();
    append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("1 NX design configuration"));

    ir.model.configurations[0]
        .parameter_values
        .insert(parameter.id, parameter.value.expect("evaluated parameter"));
    losses.clear();
    append_design_intent_losses(&ir, &mut losses);
    assert!(losses.is_empty());

    let suppressed = Feature {
        id: FeatureId::mint("test:test:feature#suppressed").expect("identity grammar"),
        ordinal: 1,
        name: None,
        suppressed: Some(true),
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Default::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::DatumPoint {
                position: cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(
                    0.0, 0.0, 0.0,
                ))
                .unwrap(),
                construction: None,
            }),
        ),
        native_ref: None,
    };
    ir.model.features.push(suppressed.clone());
    losses.clear();
    append_design_intent_losses(&ir, &mut losses);
    assert!(losses
        .iter()
        .any(|loss| loss.message.contains("1 NX design configuration")));

    ir.model.configurations[0].feature_states.insert(
        suppressed.id.clone(),
        ConfigurationFeatureState {
            evaluation: cadmpeg_ir::features::ConfigurationEvaluation::Suppressed {},
            dependencies: suppressed.dependencies,
            definition: suppressed.evaluation.definition().clone(),
        },
    );
    losses.clear();
    append_design_intent_losses(&ir, &mut losses);
    assert!(losses.is_empty());
}

#[test]
fn nx_body_producing_feature_families_require_history_outputs() {
    use cadmpeg_ir::{
        features::{
            BooleanOp, Feature, FeatureDefinition, FeatureId, FeatureOperation, UnresolvedFamily,
        },
        scalar::Length,
    };
    use std::collections::BTreeMap;

    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.model.features.push(Feature {
        id: FeatureId::mint("test:test:feature#block").expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: Default::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Default::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Block {
                dimensions: Some([
                    cadmpeg_ir::scalar::PositiveLength::new(1.0).unwrap(),
                    cadmpeg_ir::scalar::PositiveLength::new(2.0).unwrap(),
                    cadmpeg_ir::scalar::PositiveLength::new(3.0).unwrap(),
                ]),
                placement: Some(cadmpeg_ir::features::FeatureRigidPlacement::identity()),
                op: BooleanOp::NewBody,
            }),
        ),
        native_ref: None,
    });

    let mut losses = Vec::new();
    append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("block (1)"));

    let output = cadmpeg_ir::ids::BodyId::mint("test:model:body#output").expect("identity grammar");
    ir.model.features[0].evaluation.set_outputs(
        cadmpeg_ir::features::DistinctMembers::try_from(
            vec![output.clone()],
            &cadmpeg_test_support::service_decode_context(),
        )
        .unwrap(),
    );
    losses.clear();
    append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("block (1)"));

    assert!(cadmpeg_ir::features::DistinctMembers::try_from(
        vec![output.clone(), output.clone()],
        &cadmpeg_test_support::service_decode_context()
    )
    .is_err());
    losses.clear();
    append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("block (1)"));

    ir.model.features[0].suppressed = Some(true);
    losses.clear();
    append_design_intent_losses(&ir, &mut losses);
    assert!(losses.is_empty());

    ir.model.features[0]
        .evaluation
        .set_definition(FeatureDefinition::Operation(FeatureOperation::Loft {
            sections: Vec::new(),
            guidance: cadmpeg_ir::features::LoftGuidance::Guides(Vec::new()),
            op: cadmpeg_ir::features::BooleanOp::Unresolved,
            closed: false,
            solid: false,
            ruled: false,
            linearize: false,
            max_degree: None,
            allow_multi_profile_faces: None,
        }));
    append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("loft (1)"));

    ir.model.features[0]
        .evaluation
        .set_definition(FeatureDefinition::Operation(FeatureOperation::Draft {
            faces: cadmpeg_ir::features::FaceSelection::Unresolved,
            anchor: cadmpeg_ir::features::DraftAnchor::NeutralPlane {
                plane: cadmpeg_ir::features::FaceSelection::Unresolved,
                pull: Some(cadmpeg_ir::features::DraftPull {
                    direction: cadmpeg_ir::features::FeatureDirection3::new(
                        cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
                    )
                    .unwrap(),
                    plane: None,
                }),
            },
            angle: Some(cadmpeg_ir::scalar::SlopeAngle::new(0.1).unwrap()),
            outward: Some(false),
        }));
    losses.clear();
    append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("draft (1)"));

    let draft = |pull_direction: Option<cadmpeg_ir::math::Vector3>, angle, outward| {
        FeatureDefinition::Operation(FeatureOperation::Draft {
            faces: cadmpeg_ir::features::FaceSelection::Faces(vec![cadmpeg_ir::ids::FaceId::mint(
                "test:model:face#draft",
            )
            .expect("identity grammar")]),
            anchor: cadmpeg_ir::features::DraftAnchor::NeutralPlane {
                plane: cadmpeg_ir::features::FaceSelection::Faces(vec![
                    cadmpeg_ir::ids::FaceId::mint("test:model:face#neutral")
                        .expect("identity grammar"),
                ]),
                pull: pull_direction.map(|direction| cadmpeg_ir::features::DraftPull {
                    direction: cadmpeg_ir::features::FeatureDirection3::new(direction).unwrap(),
                    plane: None,
                }),
            },
            angle,
            outward,
        })
    };
    for incomplete in [
        draft(
            None,
            Some(cadmpeg_ir::scalar::SlopeAngle::new(0.1).unwrap()),
            Some(false),
        ),
        draft(
            Some(cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0)),
            None,
            Some(false),
        ),
        draft(
            Some(cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0)),
            Some(cadmpeg_ir::scalar::SlopeAngle::new(0.1).unwrap()),
            None,
        ),
    ] {
        ir.model.features[0].evaluation.set_definition(incomplete);
        losses.clear();
        append_design_intent_losses(&ir, &mut losses);
        assert_eq!(losses.len(), 1);
        assert!(losses[0].message.contains("draft (1)"));
    }

    ir.model.features[0]
        .evaluation
        .set_definition(FeatureDefinition::Operation(
            FeatureOperation::DatumOffsetPlane {
                reference: None,
                distance: Length::new(5.0).unwrap(),
            },
        ));
    losses.clear();
    append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("datum plane (1)"));

    let datum = FeatureId::mint("test:test:feature#datum-source").expect("identity grammar");
    ir.model.features[0]
        .evaluation
        .set_definition(FeatureDefinition::Operation(
            FeatureOperation::DatumOffsetPlane {
                reference: Some(cadmpeg_ir::features::DatumPlaneReference::Feature {
                    feature: datum.clone(),
                }),
                distance: Length::new(5.0).unwrap(),
            },
        ));
    losses.clear();
    append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("datum plane (1)"));

    ir.model.features[0].ordinal = 1;
    ir.model.features.push(Feature {
        id: datum.clone(),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: Default::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Default::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::DatumPrincipalPlane {
                plane: cadmpeg_ir::features::PrincipalPlane::Top,
            }),
        ),
        native_ref: None,
    });
    losses.clear();
    append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("datum plane (1)"));

    ir.model.features[0]
        .dependencies
        .insert(
            &cadmpeg_test_support::service_decode_context(),
            datum,
            "insert fixture member",
        )
        .expect("member insertion admission");
    losses.clear();
    append_design_intent_losses(&ir, &mut losses);
    assert!(losses.is_empty());

    ir.model.features[0].suppressed = Some(false);
    ir.model.features[0]
        .evaluation
        .set_definition(FeatureDefinition::Operation(FeatureOperation::SewBodies {
            bodies: (cadmpeg_ir::features::BodySelection::Bodies(
                cadmpeg_ir::features::DistinctMembers::try_from(
                    vec![
                        output.clone(),
                        cadmpeg_ir::ids::BodyId::mint("test:model:body#second")
                            .expect("identity grammar"),
                    ],
                    &cadmpeg_test_support::service_decode_context(),
                )
                .expect("distinct bodies"),
            ))
            .try_into()
            .unwrap(),
            gap_tolerance: Some(cadmpeg_ir::scalar::PositiveLength::new(0.01).unwrap()),
        }));
    losses.clear();
    append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("sew bodies (1)"));

    ir.model.features[0]
        .evaluation
        .set_definition(FeatureDefinition::Operation(FeatureOperation::SewBodies {
            bodies: (cadmpeg_ir::features::BodySelection::local(
                vec![output.as_str().to_owned(), "second-sheet".into()],
                "nx:body-selection#sew".into(),
                &cadmpeg_test_support::service_decode_context(),
            )
            .expect("body selection admission")
            .unwrap())
            .try_into()
            .unwrap(),
            gap_tolerance: Some(cadmpeg_ir::scalar::PositiveLength::new(0.01).unwrap()),
        }));
    losses.clear();
    append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("sew bodies (1)"));

    ir.model.features[0]
        .evaluation
        .set_definition(FeatureDefinition::Operation(FeatureOperation::Combine {
            operands: cadmpeg_ir::features::CombineOperands::new(
                cadmpeg_ir::features::BodySelection::local(
                    vec!["target-a".into()],
                    "nx:body-selection#targets".into(),
                    &cadmpeg_test_support::service_decode_context(),
                )
                .expect("body selection admission")
                .unwrap(),
                cadmpeg_ir::features::BodySelection::local(
                    vec!["tool".into()],
                    "nx:body-selection#tools".into(),
                    &cadmpeg_test_support::service_decode_context(),
                )
                .expect("body selection admission")
                .unwrap(),
                &cadmpeg_test_support::service_decode_context(),
            )
            .expect("operand admission")
            .unwrap(),

            op: cadmpeg_ir::features::BooleanKind::Join,
            keep_tools: false,
        }));
    losses.clear();
    append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("body combine (1)"));

    ir.model.features[0]
        .evaluation
        .set_definition(FeatureDefinition::Operation(
            FeatureOperation::BaseFeature {
                bodies: cadmpeg_ir::features::BodySelection::Unresolved,
            },
        ));
    losses.clear();
    append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("base feature (1)"));

    assert_eq!(
        FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::DatumPoint
        })
        .body_output_family(),
        None
    );
    assert_eq!(
        FeatureDefinition::Operation(FeatureOperation::BaseFeature {
            bodies: cadmpeg_ir::features::BodySelection::Unresolved,
        })
        .body_output_family(),
        Some("base feature")
    );
    assert_eq!(
        FeatureDefinition::Operation(FeatureOperation::Loft {
            sections: Vec::new(),
            guidance: cadmpeg_ir::features::LoftGuidance::Guides(Vec::new()),
            op: cadmpeg_ir::features::BooleanOp::NewBody,
            closed: false,
            solid: false,
            ruled: false,
            linearize: false,
            max_degree: None,
            allow_multi_profile_faces: None,
        })
        .body_output_family(),
        Some("loft")
    );
    assert_eq!(
        FeatureDefinition::Operation(FeatureOperation::Draft {
            faces: cadmpeg_ir::features::FaceSelection::Unresolved,
            anchor: cadmpeg_ir::features::DraftAnchor::NeutralPlane {
                plane: cadmpeg_ir::features::FaceSelection::Unresolved,
                pull: Some(cadmpeg_ir::features::DraftPull {
                    direction: cadmpeg_ir::features::FeatureDirection3::new(
                        cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0)
                    )
                    .unwrap(),
                    plane: None,
                }),
            },
            angle: Some(cadmpeg_ir::scalar::SlopeAngle::new(0.1).unwrap()),
            outward: Some(false),
        })
        .body_output_family(),
        Some("draft")
    );
    assert_eq!(
        FeatureDefinition::Operation(FeatureOperation::DeleteBody {
            bodies: cadmpeg_ir::features::BodySelection::Unresolved,
            mode: cadmpeg_ir::features::BodyRetentionMode::DeleteSelected,
        })
        .body_output_family(),
        None
    );
}

#[test]
fn nx_exact_empty_base_feature_is_a_complete_replay_boundary() {
    use cadmpeg_ir::features::{
        BodySelection, Feature, FeatureDefinition, FeatureId, FeatureOperation,
    };

    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.model.features.push(Feature {
        id: FeatureId::mint("test:test:feature#initial-bodies").expect("identity grammar"),
        ordinal: 0,
        name: Some("Retained history input".into()),
        suppressed: Some(false),
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Default::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::BaseFeature {
                bodies: BodySelection::Resolved {
                    bodies: Default::default(),
                    native: "nx:segment-body-bindings".into(),
                },
            }),
        ),
        native_ref: None,
    });

    let mut losses = Vec::new();
    append_design_intent_losses(&ir, &mut losses);

    assert!(losses.is_empty());
}

#[test]
fn nx_master_snapshot_base_feature_is_an_output_free_replay_boundary() {
    use std::collections::BTreeMap;

    use cadmpeg_ir::features::{
        BodySelection, Feature, FeatureDefinition, FeatureId, FeatureOperation,
    };

    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.model.features.push(Feature {
        id: FeatureId::mint("test:test:feature#snapshot").expect("identity grammar"),
        ordinal: 0,
        name: Some("MASTER SNAPSHOT BODY".into()),
        suppressed: Some(false),
        dependencies: Default::default(),
        source_properties: BTreeMap::from([(
            cadmpeg_core::nonblank_literal!("operation_record"),
            String::from("record"),
        )]),
        source_tag: None,
        source_text: None,
        source_content: Default::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::BaseFeature {
                bodies: BodySelection::Unresolved,
            }),
        ),
        native_ref: None,
    });

    let mut losses = Vec::new();
    append_design_intent_losses(&ir, &mut losses);

    assert!(losses.is_empty());
}

#[test]
fn nx_sew_completeness_does_not_invent_a_gap_tolerance() {
    use cadmpeg_ir::features::{
        BodySelection, Feature, FeatureDefinition, FeatureId, FeatureOperation,
    };

    let mut ir = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    let first = ir.model.bodies[0].id.clone();
    let mut second_body = ir.model.bodies[0].clone();
    second_body.id =
        cadmpeg_ir::ids::BodyId::mint("test:model:body#second").expect("identity grammar");
    let second = second_body.id.clone();
    ir.model.bodies.push(second_body);
    ir.model.features.push(Feature {
        id: FeatureId::mint("test:test:feature#sew").expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Default::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::new(
            FeatureDefinition::Operation(FeatureOperation::SewBodies {
                bodies: (BodySelection::Bodies(
                    cadmpeg_ir::features::DistinctMembers::try_from(
                        vec![first.clone(), second],
                        &cadmpeg_test_support::service_decode_context(),
                    )
                    .expect("distinct bodies"),
                ))
                .try_into()
                .unwrap(),
                gap_tolerance: None,
            }),
            cadmpeg_ir::features::DistinctMembers::try_from(
                vec![first.clone()],
                &cadmpeg_test_support::service_decode_context(),
            )
            .unwrap(),
        ),
        native_ref: None,
    });

    let mut losses = Vec::new();
    append_design_intent_losses(&ir, &mut losses);
    assert!(losses.is_empty());
}

mod numeric;

mod resource_limits;

mod shells;
