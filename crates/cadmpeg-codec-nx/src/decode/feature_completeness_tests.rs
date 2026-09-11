// SPDX-License-Identifier: Apache-2.0
//! Feature-completeness predicates owned by `decode`.

#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]

use crate::decode::feature_completeness::operands::{
    body_selection_is_incomplete, edge_selection_is_incomplete, extrude_extent_is_incomplete,
    extrude_start_is_incomplete, face_selection_is_incomplete, hole_feature_is_incomplete,
    hole_specification_is_incomplete, loft_section_is_incomplete, path_ref_is_incomplete,
    pattern_feature_is_incomplete, pattern_is_incomplete, pattern_occurrence_count,
    planar_profile_dependency_is_incomplete, planar_profile_ref_is_incomplete,
    profile_dependency_is_incomplete, profile_ref_is_incomplete, revolve_feature_is_incomplete,
    rib_feature_is_incomplete, sweep_mode_is_incomplete, sweep_orientation_is_incomplete,
    termination_dependency_is_incomplete, termination_is_incomplete,
};
use crate::decode::feature_completeness::{
    datum_coordinate_system_is_incomplete, projected_curve_direction_is_incomplete,
    shell_definition_is_incomplete,
};
use crate::decode::report::append_design_intent_losses;

#[test]
fn nx_hole_completeness_accepts_independent_placement_and_rejects_opaque_operands() {
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::{
        features::{FaceSelection, HoleKind, HolePlacement, LinearTermination, PlanarProfileRef},
        scalar::Length,
    };

    let directed = HolePlacement::Directed {
        position: cadmpeg_ir::features::FinitePoint3::new(Point3::new(1.0, 2.0, 3.0)).unwrap(),
        direction: cadmpeg_ir::features::FeatureDirection3::new(Vector3::new(0.0, 0.0, 1.0))
            .unwrap(),
    };
    assert!(!hole_feature_is_incomplete(
        None,
        None,
        Some(std::slice::from_ref(&directed)),
        (&HoleKind::Simple, None),
        Some(Length::new(5.0).unwrap()),
        Some(&LinearTermination::ThroughAll {}),
    ));
    assert!(!hole_feature_is_incomplete(
        None,
        None,
        Some(std::slice::from_ref(&directed)),
        (&HoleKind::Simple, None),
        Some(Length::new(5.0).unwrap()),
        Some(&LinearTermination::ThroughAll {}),
    ));
    let axis = HolePlacement::Axis {
        origin: cadmpeg_ir::features::FinitePoint3::new(Point3::new(1.0, 2.0, 3.0)).unwrap(),
        axis: cadmpeg_ir::features::FeatureDirection3::new(Vector3::new(0.0, 0.0, 1.0)).unwrap(),
    };
    assert!(!hole_feature_is_incomplete(
        None,
        None,
        Some(std::slice::from_ref(&axis)),
        (&HoleKind::Simple, None),
        Some(Length::new(5.0).unwrap()),
        Some(&LinearTermination::ThroughAll {}),
    ));
    for (placements, exit, extent) in [
        (
            vec![axis.clone()],
            None,
            LinearTermination::Blind {
                length: cadmpeg_ir::scalar::NonZeroLength::new(10.0).unwrap(),
            },
        ),
        (
            vec![axis],
            Some(HoleKind::Chamfer {
                diameter: cadmpeg_ir::scalar::PositiveLength::new(7.0).unwrap(),
                angle: cadmpeg_ir::scalar::InteriorAngle::new(0.5).unwrap(),
            }),
            LinearTermination::ThroughAll {},
        ),
        (
            vec![directed.clone(), directed.clone()],
            None,
            LinearTermination::ThroughAll {},
        ),
    ] {
        assert!(hole_feature_is_incomplete(
            None,
            None,
            Some(&placements),
            (&HoleKind::Simple, exit.as_ref()),
            Some(Length::new(5.0).unwrap()),
            Some(&extent),
        ));
    }
    assert!(hole_feature_is_incomplete(
        Some(&PlanarProfileRef::Unresolved("hole".into())),
        Some(&FaceSelection::Unresolved),
        None,
        (&HoleKind::Simple, None),
        Some(Length::new(5.0).unwrap()),
        Some(&LinearTermination::ThroughAll {}),
    ));
    assert!(hole_feature_is_incomplete(
        None,
        None,
        Some(std::slice::from_ref(&directed)),
        (&HoleKind::Simple, None),
        Some(Length::new(5.0).unwrap()),
        Some(&LinearTermination::Unresolved {}),
    ));
    assert!(hole_feature_is_incomplete(
        None,
        None,
        Some(std::slice::from_ref(&directed)),
        (
            &HoleKind::Simple,
            Some(&HoleKind::Unresolved(Some(
                cadmpeg_ir::features::HoleForm::Chamfer,
            ))),
        ),
        Some(Length::new(5.0).unwrap()),
        Some(&LinearTermination::ThroughAll {}),
    ));
    assert!(hole_feature_is_incomplete(
        None,
        None,
        Some(std::slice::from_ref(&directed)),
        (&HoleKind::Simple, None),
        Some(Length::ZERO),
        Some(&LinearTermination::ThroughAll {}),
    ));

    for kind in [
        HoleKind::Chamfer {
            diameter: cadmpeg_ir::scalar::PositiveLength::new(5.0).unwrap(),
            angle: cadmpeg_ir::scalar::InteriorAngle::new(0.5).unwrap(),
        },
        HoleKind::Counterbore {
            diameter: cadmpeg_ir::scalar::PositiveLength::new(4.0).unwrap(),
            depth: cadmpeg_ir::scalar::PositiveLength::new(2.0).unwrap(),
        },
        HoleKind::CounterboreDrilled {
            diameter: cadmpeg_ir::scalar::PositiveLength::new(5.0).unwrap(),
            depth: cadmpeg_ir::scalar::PositiveLength::new(2.0).unwrap(),
            drill_point_angle: cadmpeg_ir::scalar::InteriorAngle::new(0.5).unwrap(),
        },
        HoleKind::Countersink {
            diameter: cadmpeg_ir::scalar::PositiveLength::new(4.0).unwrap(),
            angle: cadmpeg_ir::scalar::InteriorAngle::new(0.5).unwrap(),
        },
        HoleKind::Counterdrill {
            diameters: cadmpeg_ir::features::CounterdrillDiameters::new(
                cadmpeg_ir::scalar::PositiveLength::new(5.0).unwrap(),
                None,
            )
            .unwrap(),

            depth: cadmpeg_ir::scalar::PositiveLength::new(2.0).unwrap(),
            angle: cadmpeg_ir::scalar::InteriorAngle::new(0.5).unwrap(),
        },
    ] {
        assert!(hole_feature_is_incomplete(
            None,
            None,
            Some(std::slice::from_ref(&directed)),
            (&kind, None),
            Some(Length::new(5.0).unwrap()),
            Some(&LinearTermination::ThroughAll {}),
        ));
    }
}

#[test]
fn nx_datum_completeness_requires_coherent_finite_frames() {
    use cadmpeg_ir::math::{Point3, Vector3};

    let origin = Point3::new(1.0, 2.0, 3.0);
    let x_axis = Vector3::new(1.0, 0.0, 0.0);
    let y_axis = Vector3::new(0.0, 1.0, 0.0);
    let z_axis = Vector3::new(0.0, 0.0, 1.0);

    assert!(!datum_coordinate_system_is_incomplete(
        origin, x_axis, y_axis, z_axis,
    ));
    assert!(datum_coordinate_system_is_incomplete(
        origin,
        x_axis,
        y_axis,
        Vector3::new(0.0, 0.0, -1.0),
    ));
    assert!(datum_coordinate_system_is_incomplete(
        origin,
        Vector3::new(2.0, 0.0, 0.0),
        y_axis,
        z_axis,
    ));
    assert!(datum_coordinate_system_is_incomplete(
        origin,
        x_axis,
        Vector3::new(1.0e-6, 1.0, 0.0),
        z_axis,
    ));
}

#[test]
fn nx_hole_completeness_checks_nested_auxiliary_semantics() {
    use cadmpeg_ir::features::{HoleSpecification, HoleThreadDepth, ThreadHand};

    let invalid_specification = HoleSpecification::Threaded {
        standard: cadmpeg_ir::NonEmptyString::new(" ").unwrap(),
        designation: None,
        class: None,
        modeled: false,
        cosmetic: true,
        pitch: Some(cadmpeg_ir::scalar::PositiveLength::new(1.0).unwrap()),
        major_diameter: Some(cadmpeg_ir::scalar::PositiveLength::new(5.0).unwrap()),
        hand: ThreadHand::Right,
        depth: HoleThreadDepth::Blind {
            depth: cadmpeg_ir::scalar::PositiveLength::new(1.0).unwrap(),
        },
        clearance: None,
    };
    assert!(hole_specification_is_incomplete(Some(
        &invalid_specification
    )));
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
fn nx_extent_completeness_checks_nested_and_face_termination() {
    use cadmpeg_ir::features::FeatureId;
    use cadmpeg_ir::features::{
        ExtrudeExtent, ExtrudeSide, ExtrudeStart, FaceSelection, GeneratedVertexRef,
        LinearTermination, VertexSelection,
    };

    let side = |termination: LinearTermination| ExtrudeSide {
        termination,
        draft: None,
    };

    assert!(!extrude_extent_is_incomplete(
        &ExtrudeExtent::TwoSided {
            first: side(LinearTermination::Blind {
                length: cadmpeg_ir::scalar::NonZeroLength::new(5.0).unwrap(),
            }),
            second: side(LinearTermination::ThroughAll {}),
        },
        &[],
    ));
    assert!(extrude_extent_is_incomplete(
        &ExtrudeExtent::Symmetric {
            side: side(LinearTermination::Unresolved {}),
        },
        &[],
    ));

    assert!(termination_is_incomplete(&LinearTermination::ToFace {
        face: FaceSelection::Native("nx:face-selection#0".to_string()),
        offset: None,
    }));
    assert!(termination_is_incomplete(&LinearTermination::ToShape {
        target: FaceSelection::Resolved {
            faces: Vec::new(),
            native: "nx:face-selection#1".to_string(),
        },
    }));
    assert!(termination_is_incomplete(
        &LinearTermination::OffsetFromFace {
            face: FaceSelection::Native("nx:face-selection#2".to_string()),
            offset: cadmpeg_ir::scalar::PositiveLength::new(1.0).unwrap(),
        }
    ));
    assert!(termination_is_incomplete(&LinearTermination::ToVertex {
        vertex: VertexSelection::native("nx:vertex-selection#0".to_string()).unwrap(),
    }));
    let vertex_feature = FeatureId::mint("test:test:feature#0").expect("identity grammar");
    let generated_vertex = LinearTermination::ToVertex {
        vertex: VertexSelection::generated(
            GeneratedVertexRef::new(vertex_feature.clone(), "vertex-0".into()).unwrap(),
            "nx:vertex-selection#1".into(),
        )
        .unwrap(),
    };
    assert!(!termination_is_incomplete(&generated_vertex));
    assert!(termination_dependency_is_incomplete(&generated_vertex, &[],));
    assert!(!termination_dependency_is_incomplete(
        &generated_vertex,
        &[vertex_feature],
    ));
    assert!(extrude_start_is_incomplete(&ExtrudeStart::FromFace {
        face: FaceSelection::Native("nx:face-selection#3".to_string()),
        offset: None,
    }));
    assert!(!extrude_start_is_incomplete(&ExtrudeStart::FromFace {
        face: FaceSelection::Faces(vec![
            cadmpeg_ir::ids::FaceId::mint("test:model:face#start").expect("identity grammar")
        ]),
        offset: None,
    }));
}

#[test]
fn nx_rib_completeness_requires_a_resolved_profile() {
    use cadmpeg_ir::features::{BooleanOp, PlanarProfileRef, RibConstruction, RibDraft, RibSide};
    use cadmpeg_ir::math::Vector3;

    let mut construction = RibConstruction {
        profile: Some(cadmpeg_ir::features::PlanarProfileRef::Native(
            "nx:profile#0".to_string(),
        )),
        direction: Some(
            cadmpeg_ir::features::FeatureDirection3::new(Vector3::new(0.0, 0.0, 1.0)).unwrap(),
        ),
        thickness: Some(cadmpeg_ir::scalar::PositiveLength::new(2.0).unwrap()),
        side: Some(RibSide::Centered),
        draft: RibDraft::None,
    };
    assert!(rib_feature_is_incomplete(&construction, BooleanOp::Join,));
    construction.profile = Some(PlanarProfileRef::Faces(vec![
        cadmpeg_ir::ids::FaceId::mint("test:model:entity#face%230".to_string())
            .expect("identity grammar"),
    ]));
    assert!(!rib_feature_is_incomplete(&construction, BooleanOp::Join,));
    construction.profile = Some(cadmpeg_ir::features::PlanarProfileRef::Faces(Vec::new()));
    assert!(rib_feature_is_incomplete(&construction, BooleanOp::Join,));
}

#[test]
fn nx_loft_completeness_validates_point_sections() {
    use cadmpeg_ir::features::{LoftPointSection, LoftSection};
    use cadmpeg_ir::ids::VertexId;
    use cadmpeg_ir::math::Point3;

    assert!(!loft_section_is_incomplete(&LoftSection::Point(
        LoftPointSection::Point(
            cadmpeg_ir::features::FinitePoint3::new(Point3::new(1.0, 2.0, 3.0)).unwrap()
        )
    ),));

    assert!(VertexId::mint(" ").is_err());
}

#[test]
fn nx_sweep_completeness_checks_nested_mode_and_orientation_operands() {
    use cadmpeg_ir::features::{PathRef, SweepMode, SweepOrientation};

    assert!(sweep_mode_is_incomplete(SweepMode::Unresolved {}));
    assert!(!sweep_mode_is_incomplete(SweepMode::Solid {
        op: cadmpeg_ir::features::SolidSweepOperation::NewBody
    }));
    assert!(sweep_orientation_is_incomplete(
        &SweepOrientation::Auxiliary {
            path: PathRef::Native("nx:auxiliary-path#0".into()),
            tangent: false,
            curvilinear: false,
        }
    ));
    assert!(!sweep_orientation_is_incomplete(
        &SweepOrientation::Auxiliary {
            path: PathRef::Curves(vec![cadmpeg_ir::ids::CurveId::mint(
                "test:model:curve#auxiliary"
            )
            .expect("identity grammar")]),
            tangent: false,
            curvilinear: false,
        }
    ));
}

#[test]
fn nx_pattern_completeness_requires_every_regeneration_operand() {
    use cadmpeg_ir::math::Vector3;
    use cadmpeg_ir::{
        features::{PathRef, PatternKind, PatternStage, PatternStageCombination, PatternTransform},
        scalar::Length,
    };

    let linear = PatternKind::new(PatternTransform::Linear {
        direction: Some(Vector3::new(1.0, 0.0, 0.0)),
        spacing: Length::new(10.0).unwrap(),
        count: 3,
        second: None,
    })
    .unwrap();
    assert!(!pattern_is_incomplete(&linear));
    assert!(pattern_is_incomplete(
        &PatternKind::new(PatternTransform::Linear {
            direction: None,
            spacing: Length::new(10.0).unwrap(),
            count: 3,
            second: None,
        })
        .unwrap()
    ));
    assert!(pattern_is_incomplete(
        &PatternKind::new(PatternTransform::Linear {
            direction: Some(Vector3::new(1.0, 0.0, 0.0)),
            spacing: Length::new(10.0).unwrap(),
            count: 1,
            second: None,
        })
        .unwrap()
    ));
    assert!(pattern_is_incomplete(
        &PatternKind::new(PatternTransform::CurveDriven {
            path: Some(PathRef::Native("nx:path".into())),
            spacing: Length::new(10.0).unwrap(),
            count: 3,
        })
        .unwrap()
    ));
    assert!(pattern_is_incomplete(
        &PatternKind::new(PatternTransform::Composite {
            stages: cadmpeg_ir::features::CompositePattern::new(vec![PatternStage {
                pattern: Box::new(
                    PatternKind::new(PatternTransform::Linear {
                        direction: None,
                        spacing: Length::new(10.0).unwrap(),
                        count: 3,
                        second: None,
                    })
                    .unwrap()
                ),
                combination: PatternStageCombination::Initialize,
            }])
            .unwrap(),
        })
        .unwrap()
    ));
    let composite = PatternKind::new(PatternTransform::Composite {
        stages: cadmpeg_ir::features::CompositePattern::new(vec![
            PatternStage {
                pattern: Box::new(linear),
                combination: PatternStageCombination::Initialize,
            },
            PatternStage {
                pattern: Box::new(
                    PatternKind::new(PatternTransform::Mirror {
                        plane_origin: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                        plane_normal: Vector3::new(1.0, 0.0, 0.0),
                    })
                    .unwrap(),
                ),
                combination: PatternStageCombination::CartesianProduct,
            },
        ])
        .unwrap(),
    })
    .unwrap();
    assert!(!pattern_is_incomplete(&composite));
    assert_eq!(pattern_occurrence_count(&composite), Some(6));
}

#[test]
fn nx_selection_completeness_requires_nonempty_unique_identities() {
    use cadmpeg_ir::features::{
        BodySelection, EdgeSelection, FaceSelection, LoftPointSection, LoftSection, PathRef,
        PlanarProfileRef,
    };

    assert!(body_selection_is_incomplete(&BodySelection::Bodies(
        Vec::new()
    )));
    assert!(!body_selection_is_incomplete(
        &BodySelection::local(
            vec!["nx:om-body-object#12".into()],
            "nx:om-object-index#12".into()
        )
        .unwrap()
    ));
    assert!(BodySelection::local(
        vec!["nx:om-body-object#12".into(), "nx:om-body-object#12".into()],
        "nx:om-object-indices#12,13".into()
    )
    .is_err());
    assert!(face_selection_is_incomplete(&FaceSelection::Resolved {
        faces: Vec::new(),
        native: "nx:faces".into(),
    }));
    assert!(edge_selection_is_incomplete(&EdgeSelection::Edges(
        Vec::new()
    )));
    assert!(!edge_selection_is_incomplete(&EdgeSelection::All));
    assert!(planar_profile_ref_is_incomplete(&PlanarProfileRef::Faces(
        Vec::new()
    )));
    assert!(planar_profile_ref_is_incomplete(
        &PlanarProfileRef::sketch_selection(
            cadmpeg_ir::sketches::SketchId::mint("test:test:sketch#0").unwrap(),
            vec!["nx:sketch-selection#0".into()]
        )
        .unwrap()
    ));
    assert!(PlanarProfileRef::sketch_profiles(
        cadmpeg_ir::sketches::SketchId::mint("test:test:sketch#0").unwrap(),
        Vec::new()
    )
    .is_err());
    assert!(path_ref_is_incomplete(&PathRef::Curves(Vec::new())));
    assert!(path_ref_is_incomplete(
        &PathRef::spatial_sketch_selection(
            cadmpeg_ir::sketches::SpatialSketchId::mint("test:test:spatial-sketch#0").unwrap(),
            vec!["nx:path-selection#0".into()]
        )
        .unwrap()
    ));
    let edge =
        cadmpeg_ir::ids::EdgeId::mint("test:model:entity#edge%230").expect("identity grammar");
    assert!(path_ref_is_incomplete(&PathRef::Edges(vec![
        edge.clone(),
        edge
    ])));
    let curve =
        cadmpeg_ir::ids::CurveId::mint("test:model:entity#curve%230").expect("identity grammar");
    assert!(path_ref_is_incomplete(&PathRef::Curves(vec![
        curve.clone(),
        curve
    ])));
    assert!(loft_section_is_incomplete(&LoftSection::Point(
        LoftPointSection::Native(cadmpeg_ir::NonEmptyString::new("nx:point-selection#0").unwrap(),)
    )));
    assert!(!loft_section_is_incomplete(&LoftSection::Point(
        LoftPointSection::Point(
            cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0))
                .unwrap(),
        )
    )));
}

#[test]
fn nx_loft_completeness_checks_native_point_sections_and_centerlines() {
    use cadmpeg_ir::features::{
        BooleanOp, Feature, FeatureDefinition, FeatureId, LoftPointSection, LoftSection, PathRef,
    };
    use cadmpeg_ir::math::Point3;

    let mut ir = cadmpeg_ir::examples::unit_cube();
    let output = ir.model.bodies[0].id.clone();
    let definition = |sections, centerline: Option<PathRef>| FeatureDefinition::Loft {
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
            vec![output],
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
                cadmpeg_ir::NonEmptyString::new("nx:point#0").unwrap(),
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
fn nx_pattern_completeness_requires_distinct_seeds() {
    use cadmpeg_ir::features::{BodySelection, FaceSelection, PatternSeed};

    let seed_id =
        cadmpeg_ir::features::FeatureId::mint("test:test:feature#seed").expect("identity grammar");
    let seed = cadmpeg_ir::features::PatternSeed::Feature(seed_id.clone());
    let pattern =
        cadmpeg_ir::features::PatternKind::new(cadmpeg_ir::features::PatternTransform::Mirror {
            plane_origin: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            plane_normal: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
        })
        .unwrap();

    assert!(!pattern_feature_is_incomplete(
        std::slice::from_ref(&seed),
        &pattern,
        std::slice::from_ref(&seed_id),
    ));
    assert!(pattern_feature_is_incomplete(
        std::slice::from_ref(&seed),
        &pattern,
        &[],
    ));
    assert!(pattern_feature_is_incomplete(
        &[seed.clone(), seed],
        &pattern,
        std::slice::from_ref(&seed_id),
    ));
    assert!(pattern_feature_is_incomplete(
        &[PatternSeed::Faces(FaceSelection::Native(
            "nx:pattern-face-selection#0".into(),
        ))],
        &pattern,
        &[],
    ));
    assert!(pattern_feature_is_incomplete(
        &[PatternSeed::Bodies(BodySelection::Unresolved)],
        &pattern,
        &[],
    ));
    assert!(!pattern_feature_is_incomplete(
        &[PatternSeed::Bodies(BodySelection::Bodies(vec![
            cadmpeg_ir::ids::BodyId::mint("test:model:body#seed").expect("identity grammar"),
        ]))],
        &pattern,
        &[],
    ));
}

#[test]
fn nx_replace_face_completeness_requires_resolved_disjoint_operands() {
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition};
    use cadmpeg_ir::ids::FaceId;

    let target = FaceId::mint("test:model:face#target").expect("identity grammar");
    let complete_targets = FaceSelection::Faces(vec![target.clone()]);
    let complete_replacements =
        FaceSelection::Faces(vec![
            FaceId::mint("test:model:face#replacement").expect("identity grammar")
        ]);

    assert_eq!(
        FeatureDefinition::ReplaceFace {
            operands: cadmpeg_ir::features::ReplaceFaceOperands::new(
                complete_targets.clone(),
                complete_replacements.clone()
            )
            .unwrap(),
        }
        .body_output_family(),
        Some("replace face")
    );
    assert!(!face_selection_is_incomplete(&complete_targets));
    assert!(!face_selection_is_incomplete(&complete_replacements));
}

#[test]
fn nx_extrude_completeness_requires_direction_start_and_solid_state() {
    use cadmpeg_ir::features::{
        BooleanOp, ExtrudeDirection, ExtrudeExtent, ExtrudeSide, ExtrudeStart, Feature,
        FeatureDefinition, FeatureId, FeatureResultTopology, LinearTermination, PlanarProfileRef,
        ProfileRef,
    };
    use cadmpeg_ir::ids::FeatureResultTopologyId;

    let mut ir = cadmpeg_ir::examples::unit_cube();
    let output = ir.model.bodies[0].id.clone();
    let definition = |direction, start, solid| FeatureDefinition::Extrude {
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

        evaluation: cadmpeg_ir::features::FeatureEvaluation::new(complete.clone(), vec![output]),
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
        FeatureResultTopology::new(
            FeatureResultTopologyId::mint("test:model:feature-result#extrude")
                .expect("identity grammar"),
            ir.model.features[0].id.clone(),
            vec!["test:feature-local-body#0".into()],
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Some("test:native-body-writer#0".into()),
        )
        .unwrap(),
    );
    losses.clear();
    append_design_intent_losses(&ir, &mut losses);
    assert!(losses.is_empty());
}

#[test]
fn nx_revolve_completeness_checks_construction_and_output_lineage() {
    use cadmpeg_ir::features::{
        AngularTermination, BooleanOp, Feature, FeatureDefinition, FeatureId, GeneratedVertexRef,
        PathRef, RevolutionAxis, RevolveConstruction, RevolveExtent, VertexSelection,
    };
    use cadmpeg_ir::math::{Point3, Vector3};

    let mut ir = cadmpeg_ir::examples::unit_cube();
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
        FeatureDefinition::Revolve {
            construction: complete.clone(),
            op: BooleanOp::NewBody,
        }
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
    incomplete = complete.clone();
    incomplete.axis_mut().unwrap().direction =
        cadmpeg_ir::features::FeatureDirection3::new(Vector3::new(0.0, 0.0, 2.0)).unwrap();
    assert!(revolve_feature_is_incomplete(
        &incomplete,
        BooleanOp::NewBody,
        &[],
    ));
    incomplete = complete.clone();
    incomplete.set_extent(None);
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
    incomplete.set_solid(None);
    assert!(revolve_feature_is_incomplete(
        &incomplete,
        BooleanOp::NewBody,
        &[],
    ));
    let source = FeatureId::mint("test:test:feature#vertex-source").expect("identity grammar");
    incomplete = complete.clone();
    incomplete.set_extent(Some(RevolveExtent::OneSided {
        termination: AngularTermination::ToVertex {
            vertex: VertexSelection::generated(
                GeneratedVertexRef::new(source.clone(), "vertex-0".into()).unwrap(),
                "test:vertex-selection".into(),
            )
            .unwrap(),
        },
    }));
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
            FeatureDefinition::Revolve {
                construction: complete,
                op: BooleanOp::NewBody,
            },
            vec![output],
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
fn nx_selection_completeness_rejects_repeated_faces_and_edges() {
    use cadmpeg_ir::features::{
        EdgeSelection, FaceSelection, FeatureId, GeneratedCurveRef, PlanarProfileRef, ProfileRef,
    };
    use cadmpeg_ir::ids::{EdgeId, FaceId};

    let face = FaceId::mint("test:model:face#repeated").expect("identity grammar");
    assert!(face_selection_is_incomplete(&FaceSelection::Faces(vec![
        face.clone(),
        face
    ]),));

    let face = FaceId::mint("test:model:profile-face#repeated").expect("identity grammar");
    assert!(profile_ref_is_incomplete(&ProfileRef::Planar(
        PlanarProfileRef::Faces(vec![face.clone(), face])
    ),));
    let producer = FeatureId::mint("test:test:feature#profile-producer").expect("identity grammar");
    let generated = PlanarProfileRef::generated(
        vec![GeneratedCurveRef::new(producer.clone(), "curve-0".into()).unwrap()],
        "test:profile-selection".into(),
    )
    .unwrap();
    assert!(!planar_profile_ref_is_incomplete(&generated));
    assert!(planar_profile_dependency_is_incomplete(&generated, &[],));
    assert!(!planar_profile_dependency_is_incomplete(
        &generated,
        std::slice::from_ref(&producer),
    ));
    let direct = ProfileRef::Planar(PlanarProfileRef::Feature(producer.clone()));
    assert!(profile_dependency_is_incomplete(&direct, &[],));
    assert!(!profile_dependency_is_incomplete(&direct, &[producer],));

    let edge = EdgeId::mint("test:model:edge#repeated").expect("identity grammar");
    assert!(edge_selection_is_incomplete(&EdgeSelection::Edges(vec![
        edge.clone(),
        edge
    ]),));
}

#[test]
fn nx_hole_completeness_rejects_opaque_supplied_operands() {
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::{
        features::{FaceSelection, HoleKind, HolePlacement, LinearTermination},
        scalar::Length,
    };

    let placement = HolePlacement::Directed {
        position: cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).unwrap(),
        direction: cadmpeg_ir::features::FeatureDirection3::new(Vector3::new(0.0, 0.0, 1.0))
            .unwrap(),
    };
    let incomplete = |profile, face| {
        hole_feature_is_incomplete(
            profile,
            face,
            Some(std::slice::from_ref(&placement)),
            (&HoleKind::Simple, None),
            Some(Length::new(1.0).unwrap()),
            Some(&LinearTermination::ThroughAll {}),
        )
    };

    assert!(!incomplete(None, None));
    let unresolved_profile = cadmpeg_ir::features::PlanarProfileRef::Unresolved("hole".into());
    assert!(incomplete(Some(&unresolved_profile), None));
    assert!(incomplete(None, Some(&FaceSelection::Unresolved)));
}

#[test]
fn nx_sketch_completeness_reports_native_geometry_and_constraints() {
    use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId};
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::sketches::{
        Sketch, SketchConstraint, SketchConstraintDefinitionInput, SketchConstraintId,
        SketchEntity, SketchEntityId, SketchGeometry, SketchId,
    };

    let mut ir = cadmpeg_ir::examples::unit_cube();
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
            FeatureDefinition::Sketch {
                sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(sketch_id.clone())),
            },
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
            cadmpeg_ir::products::NonEmptyString::new("test").expect("nonempty source identity"),
        ),
    ));
    ir.model.sketch_constraints.push(SketchConstraint {
        id: SketchConstraintId::mint("test:test:sketch-constraint#0").unwrap(),
        sketch: sketch_id,
        definition: cadmpeg_ir::sketches::SketchConstraintDefinition::try_from(
            SketchConstraintDefinitionInput::Native {
                native_kind: cadmpeg_ir::products::NonEmptyString::new("test").unwrap(),
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
fn nx_body_operation_completeness_requires_distinct_members() {
    use cadmpeg_ir::features::BodySelection;
    use cadmpeg_ir::ids::BodyId;

    let shared = BodyId::mint("test:model:body#shared").expect("identity grammar");
    let target = BodySelection::Bodies(vec![shared.clone()]);

    assert!(body_selection_is_incomplete(&BodySelection::Bodies(vec![
        shared.clone(),
        shared.clone()
    ]),));
    assert!(!body_selection_is_incomplete(&target));
}

#[test]
fn nx_configuration_completeness_requires_one_active_full_body_set() {
    use cadmpeg_ir::features::{
        BodySelection, ConfigurationBodies, ConfigurationFeatureState, ConfigurationId,
        DesignConfiguration, DesignParameter, Feature, FeatureDefinition, FeatureId, ParameterId,
        ParameterValue,
    };

    let mut ir = cadmpeg_ir::examples::unit_cube();
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
        name: "Model".into(),
        material: None,
        properties: Default::default(),
        parameter_overrides: Default::default(),
        bodies: ConfigurationBodies::Resolved(Default::default()),
        parameter_values: Default::default(),
        feature_states: Default::default(),
        native_ref: None,
    });

    let mut losses = Vec::new();
    append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("1 NX design configuration"));

    ir.model.configurations[0].bodies = ConfigurationBodies::Resolved((bodies).try_into().unwrap());
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
            FeatureDefinition::BaseFeature {
                bodies: BodySelection::Bodies(vec![output.clone()]),
            },
            vec![output.clone()],
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
                outputs: (feature.evaluation.outputs().clone()).try_into().unwrap(),
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
            FeatureDefinition::DatumPoint {
                position: cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(
                    0.0, 0.0, 0.0,
                ))
                .unwrap(),
                construction: None,
            },
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
            evaluation: cadmpeg_ir::features::ConfigurationEvaluation::Suppressed,
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
        features::{BooleanOp, Feature, FeatureDefinition, FeatureId, UnresolvedFamily},
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
            FeatureDefinition::Block {
                dimensions: Some([
                    cadmpeg_ir::scalar::PositiveLength::new(1.0).unwrap(),
                    cadmpeg_ir::scalar::PositiveLength::new(2.0).unwrap(),
                    cadmpeg_ir::scalar::PositiveLength::new(3.0).unwrap(),
                ]),
                placement: Some(cadmpeg_ir::features::FeatureRigidPlacement::identity()),
                op: BooleanOp::NewBody,
            },
        ),
        native_ref: None,
    });

    let mut losses = Vec::new();
    append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("block (1)"));

    let output = cadmpeg_ir::ids::BodyId::mint("test:model:body#output").expect("identity grammar");
    ir.model.features[0]
        .evaluation
        .set_outputs(vec![output.clone()]);
    losses.clear();
    append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("block (1)"));

    ir.model.features[0]
        .evaluation
        .set_outputs(vec![output.clone(), output.clone()]);
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
        .set_definition(FeatureDefinition::Loft {
            sections: Vec::new(),
            guidance: cadmpeg_ir::features::LoftGuidance::Guides(Vec::new()),
            op: cadmpeg_ir::features::BooleanOp::Unresolved,
            closed: false,
            solid: false,
            ruled: false,
            linearize: false,
            max_degree: None,
            allow_multi_profile_faces: None,
        });
    append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("loft (1)"));

    ir.model.features[0]
        .evaluation
        .set_definition(FeatureDefinition::Draft {
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
        });
    losses.clear();
    append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("draft (1)"));

    let draft = |pull_direction: Option<cadmpeg_ir::math::Vector3>, angle, outward| {
        FeatureDefinition::Draft {
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
        }
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
        .set_definition(FeatureDefinition::DatumOffsetPlane {
            reference: None,
            distance: Length::new(5.0).unwrap(),
        });
    losses.clear();
    append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("datum plane (1)"));

    let datum = FeatureId::mint("test:test:feature#datum-source").expect("identity grammar");
    ir.model.features[0]
        .evaluation
        .set_definition(FeatureDefinition::DatumOffsetPlane {
            reference: Some(cadmpeg_ir::features::DatumPlaneReference::Feature {
                feature: datum.clone(),
            }),
            distance: Length::new(5.0).unwrap(),
        });
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
            FeatureDefinition::DatumPrincipalPlane {
                plane: cadmpeg_ir::features::PrincipalPlane::Top,
            },
        ),
        native_ref: None,
    });
    losses.clear();
    append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("datum plane (1)"));

    ir.model.features[0].dependencies.insert(datum);
    losses.clear();
    append_design_intent_losses(&ir, &mut losses);
    assert!(losses.is_empty());

    ir.model.features[0].suppressed = Some(false);
    ir.model.features[0]
        .evaluation
        .set_definition(FeatureDefinition::SewBodies {
            bodies: (cadmpeg_ir::features::BodySelection::Bodies(vec![
                output.clone(),
                cadmpeg_ir::ids::BodyId::mint("test:model:body#second").expect("identity grammar"),
            ]))
            .try_into()
            .unwrap(),
            gap_tolerance: Some(cadmpeg_ir::scalar::PositiveLength::new(0.01).unwrap()),
        });
    losses.clear();
    append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("sew bodies (1)"));

    ir.model.features[0]
        .evaluation
        .set_definition(FeatureDefinition::SewBodies {
            bodies: (cadmpeg_ir::features::BodySelection::local(
                vec![output.as_str().to_owned(), "second-sheet".into()],
                "nx:body-selection#sew".into(),
            )
            .unwrap())
            .try_into()
            .unwrap(),
            gap_tolerance: Some(cadmpeg_ir::scalar::PositiveLength::new(0.01).unwrap()),
        });
    losses.clear();
    append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("sew bodies (1)"));

    ir.model.features[0]
        .evaluation
        .set_definition(FeatureDefinition::Combine {
            operands: cadmpeg_ir::features::CombineOperands::new(
                cadmpeg_ir::features::BodySelection::local(
                    vec!["target-a".into()],
                    "nx:body-selection#targets".into(),
                )
                .unwrap(),
                cadmpeg_ir::features::BodySelection::local(
                    vec!["tool".into()],
                    "nx:body-selection#tools".into(),
                )
                .unwrap(),
            )
            .unwrap(),

            op: cadmpeg_ir::features::BooleanKind::Join,
            keep_tools: false,
        });
    losses.clear();
    append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("body combine (1)"));

    ir.model.features[0]
        .evaluation
        .set_definition(FeatureDefinition::BaseFeature {
            bodies: cadmpeg_ir::features::BodySelection::Unresolved,
        });
    losses.clear();
    append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("base feature (1)"));

    assert_eq!(
        FeatureDefinition::Unresolved {
            family: UnresolvedFamily::DatumPoint
        }
        .body_output_family(),
        None
    );
    assert_eq!(
        FeatureDefinition::BaseFeature {
            bodies: cadmpeg_ir::features::BodySelection::Unresolved,
        }
        .body_output_family(),
        Some("base feature")
    );
    assert_eq!(
        FeatureDefinition::Loft {
            sections: Vec::new(),
            guidance: cadmpeg_ir::features::LoftGuidance::Guides(Vec::new()),
            op: cadmpeg_ir::features::BooleanOp::NewBody,
            closed: false,
            solid: false,
            ruled: false,
            linearize: false,
            max_degree: None,
            allow_multi_profile_faces: None,
        }
        .body_output_family(),
        Some("loft")
    );
    assert_eq!(
        FeatureDefinition::Draft {
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
        }
        .body_output_family(),
        Some("draft")
    );
    assert_eq!(
        FeatureDefinition::DeleteBody {
            bodies: cadmpeg_ir::features::BodySelection::Unresolved,
            mode: cadmpeg_ir::features::BodyRetentionMode::DeleteSelected,
        }
        .body_output_family(),
        None
    );
}

#[test]
fn nx_exact_empty_base_feature_is_a_complete_replay_boundary() {
    use cadmpeg_ir::features::{BodySelection, Feature, FeatureDefinition, FeatureId};

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
            FeatureDefinition::BaseFeature {
                bodies: BodySelection::Resolved {
                    bodies: Vec::new(),
                    native: "nx:segment-body-bindings".into(),
                },
            },
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

    use cadmpeg_ir::features::{BodySelection, Feature, FeatureDefinition, FeatureId};

    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.model.features.push(Feature {
        id: FeatureId::mint("test:test:feature#snapshot").expect("identity grammar"),
        ordinal: 0,
        name: Some("MASTER SNAPSHOT BODY".into()),
        suppressed: Some(false),
        dependencies: Default::default(),
        source_properties: BTreeMap::from([(
            String::from("operation_record"),
            String::from("record"),
        )]),
        source_tag: None,
        source_text: None,
        source_content: Default::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::BaseFeature {
                bodies: BodySelection::Unresolved,
            },
        ),
        native_ref: None,
    });

    let mut losses = Vec::new();
    append_design_intent_losses(&ir, &mut losses);

    assert!(losses.is_empty());
}

#[test]
fn nx_sew_completeness_does_not_invent_a_gap_tolerance() {
    use cadmpeg_ir::features::{BodySelection, Feature, FeatureDefinition, FeatureId};

    let mut ir = cadmpeg_ir::examples::unit_cube();
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
            FeatureDefinition::SewBodies {
                bodies: (BodySelection::Bodies(vec![first.clone(), second]))
                    .try_into()
                    .unwrap(),
                gap_tolerance: None,
            },
            vec![first.clone()],
        ),
        native_ref: None,
    });

    let mut losses = Vec::new();
    append_design_intent_losses(&ir, &mut losses);
    assert!(losses.is_empty());
}

#[test]
fn nx_shell_completeness_requires_each_construction_field() {
    use cadmpeg_ir::features::{
        BodySelection, FaceSelection, FeatureDefinition, ShellJoin, ShellMode,
    };
    use cadmpeg_ir::ids::{BodyId, FaceId};

    let incomplete = FeatureDefinition::Shell {
        bodies: None,
        removed_faces: FaceSelection::Unresolved,
        thickness: None,
        outward: None,
        mode: None,
        join: None,
        resolve_intersections: None,
        allow_self_intersections: None,
    };
    assert!(shell_definition_is_incomplete(&incomplete));

    let complete = FeatureDefinition::Shell {
        bodies: Some(BodySelection::Bodies(vec![BodyId::mint(
            "test:model:body#shell",
        )
        .expect("identity grammar")])),
        removed_faces: FaceSelection::Faces(vec![
            FaceId::mint("test:model:face#opening").expect("identity grammar")
        ]),
        thickness: Some(cadmpeg_ir::scalar::PositiveLength::new(2.0).unwrap()),
        outward: Some(false),
        mode: Some(ShellMode::Skin),
        join: Some(ShellJoin::Intersection),
        resolve_intersections: Some(true),
        allow_self_intersections: Some(false),
    };
    assert!(!shell_definition_is_incomplete(&complete));
    assert_eq!(complete.body_output_family(), Some("shell"));
}
