// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn design_intent_losses_distinguish_native_and_sketch_gaps() {
    use cadmpeg_ir::document::CadIr;
    use cadmpeg_ir::features::{
        BooleanOp, ConfigurationId, DesignConfiguration, Feature, FeatureDefinition, FeatureId,
        FeatureOperation, UnresolvedFamily,
    };

    let mut ir = CadIr::empty();
    for (ordinal, kind) in ["DELETE", "DELETE"].into_iter().enumerate() {
        ir.model.features.push(Feature {
            product_definition: None,
            needs_recompute: false,
            id: FeatureId::mint(format!("test:test:feature#{ordinal}")).expect("identity grammar"),
            ordinal: cadmpeg_core::decode::u64_from_index(ordinal),
            name: None,
            suppressed: None,
            dependencies: Default::default(),
            source_properties: Default::default(),
            source_tag: None,
            source_text: None,
            source_content: Default::default(),

            evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
                FeatureDefinition::Operation(FeatureOperation::Native {
                    kind: kind.into(),
                    parameters: Default::default(),
                }),
            ),
            native_ref: None,
        });
    }
    ir.model.features.push(Feature {
        product_definition: None,
        needs_recompute: false,
        id: FeatureId::mint("test:test:feature#sketch").expect("identity grammar"),
        ordinal: 3,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Default::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Sketch {
                sketch: cadmpeg_ir::features::SketchFeatureBinding::Unresolved,
            }),
        ),
        native_ref: None,
    });
    ir.model.features.push(Feature {
        product_definition: None,
        needs_recompute: false,
        id: FeatureId::mint("test:test:feature#incomplete-delete").expect("identity grammar"),
        ordinal: 10,
        name: None,
        suppressed: Some(false),
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Default::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::DeleteBody {
                bodies: cadmpeg_ir::features::BodySelection::Unresolved,
                mode: cadmpeg_ir::features::BodyRetentionMode::DeleteSelected,
            }),
        ),
        native_ref: None,
    });
    for (ordinal, definition) in [
        FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::DatumPlane,
        }),
        FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::DatumCoordinateSystem,
        }),
        FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::Loft,
        }),
        FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::FreeformSurface,
        }),
        FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::Loft,
        }),
    ]
    .into_iter()
    .enumerate()
    {
        ir.model.features.push(Feature {
            product_definition: None,
            needs_recompute: false,
            id: FeatureId::mint(format!("test:test:feature#unresolved-{ordinal}"))
                .expect("identity grammar"),
            ordinal: cadmpeg_core::decode::u64_from_index(ordinal) + 4,
            name: None,
            suppressed: None,
            dependencies: Default::default(),
            source_properties: Default::default(),
            source_tag: None,
            source_text: None,
            source_content: Default::default(),

            evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(definition),
            native_ref: None,
        });
    }
    ir.model.features.push(Feature {
        product_definition: None,
        needs_recompute: false,
        id: FeatureId::mint("test:test:feature#incomplete-block").expect("identity grammar"),
        ordinal: 9,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Default::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Block {
                dimensions: None,
                placement: None,
                op: BooleanOp::Unresolved,
            }),
        ),
        native_ref: None,
    });
    ir.model.features.push(Feature {
        product_definition: None,
        needs_recompute: false,
        id: FeatureId::mint("test:test:feature#incomplete-sweep").expect("identity grammar"),
        ordinal: 11,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Default::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Sweep {
                shape: cadmpeg_ir::features::SweepShape::sheet_sections(
                    cadmpeg_ir::features::SweepMode::Unresolved {},
                    cadmpeg_ir::features::SweepSection::Unresolved(None),
                    Vec::new(),
                ),

                path: None,
                path_extent: None,
                guide_rail: None,
                taper: None,

                orientation: None,
                transition: None,
                transformation: None,
                path_tangent: false,
                linearize: false,
                twist: None,
                scale: None,
                allow_multi_profile_faces: None,
            }),
        ),
        native_ref: None,
    });
    ir.model.configurations.extend([
        DesignConfiguration {
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
        },
        DesignConfiguration {
            id: ConfigurationId::mint("test:test:configuration#1").expect("identity grammar"),
            ordinal: 1,
            active: false,
            source_index: Some(1),
            name: Some("Arrangement".to_string()),
            material: None,
            properties: Default::default(),
            parameter_overrides: Default::default(),
            bodies: None,
            parameter_values: Default::default(),
            feature_states: Default::default(),
            native_ref: None,
        },
    ]);

    let mut losses = Vec::new();
    append_design_intent_losses(&ir, &mut losses);

    assert_eq!(losses.len(), 7);
    assert_eq!(losses[0].code.category(), LossCategory::DesignIntent);
    assert!(losses[0]
        .message
        .contains("10 NX feature history operation"));
    assert_eq!(losses[1].code.category(), LossCategory::DesignIntent);
    assert!(losses[1].message.contains("2 NX arrangement(s)"));
    assert_eq!(losses[2].code.category(), LossCategory::DesignIntent);
    assert!(losses[2].message.contains("DELETE (2)"));
    assert_eq!(losses[3].code.category(), LossCategory::DesignIntent);
    assert!(losses[3].message.contains("datum coordinate system (1)"));
    assert!(losses[3].message.contains("datum plane (1)"));
    assert!(losses[3].message.contains("freeform surface (1)"));
    assert!(losses[3].message.contains("loft (2)"));
    assert_eq!(losses[4].code.category(), LossCategory::DesignIntent);
    assert!(losses[4].message.contains("block (1)"));
    assert!(losses[4].message.contains("sweep (1)"));
    assert_eq!(losses[5].code.category(), LossCategory::DesignIntent);
    assert!(losses[5].message.contains("delete body (1)"));
    assert!(losses[5].message.contains("sketch (1)"));
    assert_eq!(losses[6].code.category(), LossCategory::DesignIntent);
    assert!(losses[6].message.contains("1 NX sketch history feature"));
    assert!(losses[6].message.contains("1 have no neutral sketch graph"));

    let sketch_id = cadmpeg_ir::sketches::SketchId::mint("test:test:sketch#0").unwrap();
    ir.model.sketches.push(cadmpeg_ir::sketches::Sketch {
        id: sketch_id.clone(),
        name: None,
        configuration: None,
        visible: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::try_resolved(
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
            cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
        profiles: Default::default(),
        native_ref: None,
    });
    ir.model.features[2]
        .evaluation
        .set_definition(FeatureDefinition::Operation(FeatureOperation::Sketch {
            sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(sketch_id)),
        }));
    losses.clear();
    append_design_intent_losses(&ir, &mut losses);

    assert_eq!(losses.len(), 6);
    assert!(losses[4].message.contains("block (1)"));
    assert!(!losses[5].message.contains("sketch"));
}

#[test]
fn design_intent_losses_ignore_unresolved_suppression_outside_active_closure() {
    use cadmpeg_ir::features::{
        Feature, FeatureDefinition, FeatureId, FeatureOperation, UnresolvedFamily,
    };

    let mut ir = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    let body = ir.model.bodies[0].id.clone();
    ir.model.features.extend([
        Feature {
            product_definition: None,
            needs_recompute: false,
            id: FeatureId::mint("test:test:feature#active").expect("identity grammar"),
            ordinal: 0,
            name: Some("active".into()),
            suppressed: Some(false),
            dependencies: Default::default(),
            source_properties: Default::default(),
            source_tag: None,
            source_text: None,
            source_content: Default::default(),

            evaluation: cadmpeg_ir::features::FeatureEvaluation::new(
                FeatureDefinition::Operation(FeatureOperation::DatumPoint {
                    position: cadmpeg_ir::features::FinitePoint3::new(
                        cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                    )
                    .unwrap(),
                    construction: None,
                }),
                cadmpeg_ir::features::DistinctMembers::try_from(vec![body]).unwrap(),
            ),
            native_ref: None,
        },
        Feature {
            product_definition: None,
            needs_recompute: false,
            id: FeatureId::mint("test:test:feature#inactive").expect("identity grammar"),
            ordinal: 1,
            name: Some("inactive".into()),
            suppressed: None,
            dependencies: Default::default(),
            source_properties: Default::default(),
            source_tag: None,
            source_text: None,
            source_content: Default::default(),

            evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
                FeatureDefinition::Operation(FeatureOperation::DatumPoint {
                    position: cadmpeg_ir::features::FinitePoint3::new(
                        cadmpeg_ir::math::Point3::new(1.0, 0.0, 0.0),
                    )
                    .unwrap(),
                    construction: None,
                }),
            ),
            native_ref: None,
        },
        Feature {
            product_definition: None,
            needs_recompute: false,
            id: FeatureId::mint("test:test:feature#inactive-native").expect("identity grammar"),
            ordinal: 2,
            name: Some("inactive-native".into()),
            suppressed: None,
            dependencies: Default::default(),
            source_properties: Default::default(),
            source_tag: None,
            source_text: None,
            source_content: Default::default(),

            evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
                FeatureDefinition::Operation(FeatureOperation::Native {
                    kind: "DELETE".into(),
                    parameters: Default::default(),
                }),
            ),
            native_ref: None,
        },
        Feature {
            product_definition: None,
            needs_recompute: false,
            id: FeatureId::mint("test:test:feature#inactive-datum-csys").expect("identity grammar"),
            ordinal: 3,
            name: Some("inactive-datum-csys".into()),
            suppressed: None,
            dependencies: Default::default(),
            source_properties: Default::default(),
            source_tag: None,
            source_text: None,
            source_content: Default::default(),

            evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
                FeatureDefinition::Operation(FeatureOperation::Unresolved {
                    family: UnresolvedFamily::DatumCoordinateSystem,
                }),
            ),
            native_ref: None,
        },
        Feature {
            product_definition: None,
            needs_recompute: false,
            id: FeatureId::mint("test:test:feature#inactive-sketch").expect("identity grammar"),
            ordinal: 4,
            name: Some("inactive-sketch".into()),
            suppressed: None,
            dependencies: Default::default(),
            source_properties: Default::default(),
            source_tag: None,
            source_text: None,
            source_content: Default::default(),

            evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
                FeatureDefinition::Operation(FeatureOperation::Sketch {
                    sketch: cadmpeg_ir::features::SketchFeatureBinding::Unresolved,
                }),
            ),
            native_ref: None,
        },
    ]);

    let mut losses = Vec::new();
    append_design_intent_losses(&ir, &mut losses);
    assert!(losses.is_empty());
}

#[test]
fn design_intent_losses_do_not_scope_to_retained_base_feature_alone() {
    use cadmpeg_ir::features::{
        BodySelection, Feature, FeatureDefinition, FeatureId, FeatureOperation,
    };

    let mut ir = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    let body = ir.model.bodies[0].id.clone();
    ir.model.features.extend([
        Feature {
            product_definition: None,
            needs_recompute: false,
            id: FeatureId::mint("test:test:feature#retained-input").expect("identity grammar"),
            ordinal: 0,
            name: Some("Retained history input".into()),
            suppressed: Some(false),
            dependencies: Default::default(),
            source_properties: Default::default(),
            source_tag: None,
            source_text: None,
            source_content: Default::default(),

            evaluation: cadmpeg_ir::features::FeatureEvaluation::new(
                FeatureDefinition::Operation(FeatureOperation::BaseFeature {
                    bodies: BodySelection::Resolved {
                        bodies: cadmpeg_ir::features::DistinctMembers::try_from(vec![body.clone()])
                            .expect("distinct bodies"),
                        native: "nx:segment-body-bindings".into(),
                    },
                }),
                cadmpeg_ir::features::DistinctMembers::try_from(vec![body.clone()]).unwrap(),
            ),
            native_ref: None,
        },
        Feature {
            product_definition: None,
            needs_recompute: false,
            id: FeatureId::mint("test:test:feature#unresolved").expect("identity grammar"),
            ordinal: 1,
            name: Some("unresolved".into()),
            suppressed: None,
            dependencies: Default::default(),
            source_properties: Default::default(),
            source_tag: None,
            source_text: None,
            source_content: Default::default(),

            evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
                FeatureDefinition::Operation(FeatureOperation::Native {
                    kind: "DELETE".into(),
                    parameters: Default::default(),
                }),
            ),
            native_ref: None,
        },
    ]);

    let mut losses = Vec::new();
    append_design_intent_losses(&ir, &mut losses);

    assert_eq!(losses.len(), 2);
    assert!(losses[0]
        .message
        .contains("Suppression state remains unresolved for 1 NX feature history operation"));
    assert!(losses[0]
        .message
        .contains("no admitted operation-to-state-object-to-typed-value relation is present"));
    assert!(losses[1].message.contains("DELETE (1)"));
}

#[test]
fn design_intent_losses_accept_output_free_local_body_operations() {
    use cadmpeg_ir::document::CadIr;
    use cadmpeg_ir::features::{
        patterns::PatternKind, Feature, FeatureDefinition, FeatureId, FeatureOperation,
    };

    let mut ir = CadIr::empty();
    let mut source_properties = std::collections::BTreeMap::new();
    source_properties.insert(
        "primary_body_reference".to_string(),
        "reference".to_string(),
    );
    ir.model.features.push(Feature {
        product_definition: None,
        needs_recompute: false,
        id: FeatureId::mint("test:test:feature#local-pattern").expect("identity grammar"),
        ordinal: 0,
        name: Some("Pattern Geometry".into()),
        suppressed: Some(false),
        dependencies: Default::default(),
        source_properties: cadmpeg_core::text::named_entries(
            "test:test:feature#local-pattern",
            source_properties,
        )
        .expect("the fixture states named properties"),
        source_tag: Some("Pattern Geometry".into()),
        source_text: None,
        source_content: Default::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Pattern {
                seeds: Vec::new(),
                pattern: PatternKind::UNRESOLVED,
            }),
        ),
        native_ref: None,
    });

    let mut losses = Vec::new();
    append_design_intent_losses(&ir, &mut losses);

    assert_eq!(losses.len(), 1);
    assert!(losses[0]
        .message
        .contains("incomplete neutral construction fields"));
    assert!(losses[0].message.contains("pattern (1)"));
}

#[test]
fn design_intent_losses_accept_pattern_construction_without_body_reference() {
    use cadmpeg_ir::features::{
        patterns::PatternKind, Feature, FeatureDefinition, FeatureId, FeatureOperation,
    };

    let feature = Feature {
        product_definition: None,
        needs_recompute: false,
        id: FeatureId::mint("test:test:feature#pattern-construction").expect("identity grammar"),
        ordinal: 0,
        name: Some("Pattern Geometry".into()),
        suppressed: Some(false),
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: Some("Pattern Geometry".into()),
        source_text: None,
        source_content: Default::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Pattern {
                seeds: Vec::new(),
                pattern: PatternKind::UNRESOLVED,
            }),
        ),
        native_ref: None,
    };
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    ir.model.features.push(feature);

    let mut losses = Vec::new();
    append_design_intent_losses(&ir, &mut losses);

    assert_eq!(losses.len(), 1);
    assert!(losses[0]
        .message
        .contains("incomplete neutral construction fields"));
    assert!(losses[0].message.contains("pattern (1)"));

    ir.model.features[0].source_properties.insert(
        cadmpeg_core::nonblank_literal!("body_reference.0"),
        "42".into(),
    );
    losses.clear();
    append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0]
        .message
        .contains("output lineage is missing, duplicated"));
}

#[test]
fn design_intent_losses_accept_unbound_trim_surface_construction() {
    use cadmpeg_ir::features::{
        FaceSelection, Feature, FeatureDefinition, FeatureId, FeatureOperation, PathRef, TrimRegion,
    };

    let mut ir = cadmpeg_ir::document::CadIr::empty();
    ir.model.features.push(Feature {
        product_definition: None,
        needs_recompute: false,
        id: FeatureId::mint("test:test:feature#construction-trim").expect("identity grammar"),
        ordinal: 0,
        name: Some("TRIMMED_SH".into()),
        suppressed: Some(false),
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: Some("TRIMMED_SH".into()),
        source_text: None,
        source_content: Default::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::TrimSurface {
                faces: FaceSelection::Faces(vec![cadmpeg_ir::ids::FaceId::mint(
                    "test:model:entity#face",
                )
                .expect("identity grammar")]),
                tool: PathRef::Edges(vec![cadmpeg_ir::ids::EdgeId::mint(
                    "test:model:entity#edge",
                )
                .expect("identity grammar")]),
                keep: TrimRegion::Inside,
            }),
        ),
        native_ref: None,
    });

    let mut losses = Vec::new();
    append_design_intent_losses(&ir, &mut losses);
    assert!(losses.is_empty());

    ir.model.features[0].source_properties.insert(
        cadmpeg_core::nonblank_literal!("body_reference.0"),
        "42".into(),
    );
    append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0]
        .message
        .contains("output lineage is missing, duplicated"));
}

#[test]
fn output_free_local_body_construction_requires_unbound_primary_body() {
    use cadmpeg_ir::features::{
        patterns::PatternKind, Feature, FeatureDefinition, FeatureId, FeatureOperation,
    };

    let mut source_properties = std::collections::BTreeMap::new();
    source_properties.insert(
        "primary_body_reference".to_string(),
        "reference".to_string(),
    );
    let mut feature = Feature {
        product_definition: None,
        needs_recompute: false,
        id: FeatureId::mint("test:test:feature#local-pattern").expect("identity grammar"),
        ordinal: 0,
        name: Some("Pattern Geometry".into()),
        suppressed: Some(false),
        dependencies: Default::default(),
        source_properties: cadmpeg_core::text::named_entries(
            "test:test:feature#local-pattern",
            source_properties,
        )
        .expect("the fixture states named properties"),
        source_tag: Some("Pattern Geometry".into()),
        source_text: None,
        source_content: Default::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Pattern {
                seeds: Vec::new(),
                pattern: PatternKind::UNRESOLVED,
            }),
        ),
        native_ref: None,
    };

    assert!(output_free_local_body_construction(&feature));

    feature.source_properties.remove("primary_body_reference");
    feature.source_properties.insert(
        cadmpeg_core::nonblank_literal!("body_reference.0"),
        "42".to_string(),
    );
    assert!(!output_free_local_body_construction(&feature));

    feature.source_properties.insert(
        cadmpeg_core::nonblank_literal!("primary_body_reference"),
        "reference".to_string(),
    );
    feature.source_properties.insert(
        cadmpeg_core::nonblank_literal!("primary_body_segment_use"),
        "segment-use".to_string(),
    );
    assert!(!output_free_local_body_construction(&feature));
}
