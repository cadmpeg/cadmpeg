// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]

use super::{
    body_selection_is_incomplete, edge_selection_is_incomplete, extrude_extent_is_incomplete,
    extrude_start_is_incomplete, face_selection_is_incomplete, hole_feature_is_incomplete,
    loft_section_is_incomplete, path_ref_is_incomplete, pattern_feature_is_incomplete,
    pattern_is_incomplete, pattern_occurrence_count, planar_profile_dependency_is_incomplete,
    planar_profile_ref_is_incomplete, profile_dependency_is_incomplete, profile_ref_is_incomplete,
    rib_feature_is_incomplete, selection_ids_are_incomplete, sweep_mode_is_incomplete,
    sweep_orientation_is_incomplete, termination_dependency_is_incomplete,
    termination_is_incomplete,
};

#[test]
    fn selection_completeness_detects_nonadjacent_duplicate_ids() {
        assert!(selection_ids_are_incomplete::<u32>(&[]));
        assert!(!selection_ids_are_incomplete(&[2, 1, 3]));
        assert!(selection_ids_are_incomplete(&[2, 1, 2]));
    }

#[test]
fn nx_hole_completeness_accepts_independent_placement_and_rejects_opaque_operands() {
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::{
        features::{
            holes::{HoleKind, HolePlacement},
            FaceSelection, LinearTermination, PlanarProfileRef,
        },
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
    let short =
        cadmpeg_ir::features::FeatureDirection3::new(Vector3::new(0.0, 0.0, 1.0e-13)).unwrap();
    for placement in [
        HolePlacement::Directed {
            position: cadmpeg_ir::features::FinitePoint3::new(Point3::new(1.0, 2.0, 3.0)).unwrap(),
            direction: short,
        },
        HolePlacement::Axis {
            origin: cadmpeg_ir::features::FinitePoint3::new(Point3::new(1.0, 2.0, 3.0)).unwrap(),
            axis: short,
        },
    ] {
        assert!(!hole_feature_is_incomplete(
            None,
            None,
            Some(std::slice::from_ref(&placement)),
            (&HoleKind::Simple, None),
            Some(Length::new(5.0).unwrap()),
            Some(&LinearTermination::ThroughAll {}),
        ));
    }
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
                cadmpeg_ir::features::holes::HoleForm::Chamfer,
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
            diameters: cadmpeg_ir::features::holes::CounterdrillDiameters::new(
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
        vertex: VertexSelection::native(
            "nx:vertex-selection#0".to_string(),
            &cadmpeg_test_support::service_decode_context(),
        )
        .expect("selection reference admission")
        .unwrap(),
    }));
    let vertex_feature = FeatureId::mint("test:test:feature#0").expect("identity grammar");
    let generated_vertex = LinearTermination::ToVertex {
        vertex: VertexSelection::generated(
            GeneratedVertexRef::new(
                vertex_feature.clone(),
                "vertex-0".into(),
                &cadmpeg_test_support::service_decode_context(),
            )
            .expect("selection reference admission")
            .unwrap(),
            "nx:vertex-selection#1".into(),
            &cadmpeg_test_support::service_decode_context(),
        )
        .expect("selection reference admission")
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
    use cadmpeg_ir::features::{
        patterns::{PatternKind, PatternStage, PatternTransform},
        PathRef,
    };
    use cadmpeg_ir::math::Vector3;

    let linear = PatternKind::new(PatternTransform::Linear {
        direction: Some(
            cadmpeg_ir::features::FeatureDirection3::new(Vector3::new(1.0, 0.0, 0.0)).unwrap(),
        ),
        spacing: cadmpeg_ir::scalar::PositiveLength::new(10.0).unwrap(),
        count: 3,
        second: None,
    })
    .unwrap();
    assert!(!pattern_is_incomplete(&linear));
    assert!(pattern_is_incomplete(
        &PatternKind::<cadmpeg_ir::features::patterns::CompositePattern>::new(
            PatternTransform::Linear {
                direction: None,
                spacing: cadmpeg_ir::scalar::PositiveLength::new(10.0).unwrap(),
                count: 3,
                second: None,
            }
        )
        .unwrap()
    ));
    assert!(pattern_is_incomplete(
        &PatternKind::<cadmpeg_ir::features::patterns::CompositePattern>::new(
            PatternTransform::Linear {
                direction: Some(
                    cadmpeg_ir::features::FeatureDirection3::new(Vector3::new(1.0, 0.0, 0.0))
                        .unwrap()
                ),
                spacing: cadmpeg_ir::scalar::PositiveLength::new(10.0).unwrap(),
                count: 1,
                second: None,
            }
        )
        .unwrap()
    ));
    assert!(pattern_is_incomplete(
        &PatternKind::<cadmpeg_ir::features::patterns::CompositePattern>::new(
            PatternTransform::CurveDriven {
                path: Some(PathRef::Native("nx:path".into())),
                spacing: cadmpeg_ir::scalar::PositiveLength::new(10.0).unwrap(),
                count: 3,
            }
        )
        .unwrap()
    ));
    assert!(pattern_is_incomplete(
        &PatternKind::new(PatternTransform::Composite {
            stages: cadmpeg_ir::features::patterns::CompositePattern::new(vec![PatternStage {
                pattern: Box::new(
                    PatternKind::new(PatternTransform::Linear {
                        direction: None,
                        spacing: cadmpeg_ir::scalar::PositiveLength::new(10.0).unwrap(),
                        count: 3,
                        second: None,
                    })
                    .unwrap()
                ),
            }])
            .unwrap(),
        })
        .unwrap()
    ));
    let composite = PatternKind::new(PatternTransform::Composite {
        stages: cadmpeg_ir::features::patterns::CompositePattern::new(vec![
            PatternStage {
                pattern: Box::new(linear),
            },
            PatternStage {
                pattern: Box::new(
                    PatternKind::new(PatternTransform::Mirror {
                        plane_origin: cadmpeg_ir::features::FinitePoint3::new(
                            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                        )
                        .unwrap(),
                        plane_normal: cadmpeg_ir::features::FeatureDirection3::new(Vector3::new(
                            1.0, 0.0, 0.0,
                        ))
                        .unwrap(),
                    })
                    .unwrap(),
                ),
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
        Default::default()
    )));
    assert!(!body_selection_is_incomplete(
        &BodySelection::local(
            vec!["nx:om-body-object#12".into()],
            "nx:om-object-index#12".into(),
            &cadmpeg_test_support::service_decode_context(),
        )
        .expect("body selection admission")
        .unwrap()
    ));
    assert!(BodySelection::local(
        vec!["nx:om-body-object#12".into(), "nx:om-body-object#12".into()],
        "nx:om-object-indices#12,13".into(),
        &cadmpeg_test_support::service_decode_context(),
    )
    .expect("body selection admission")
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
            vec!["nx:sketch-selection#0".into()],
            &cadmpeg_test_support::service_decode_context(),
        )
        .expect("profile membership admission")
        .unwrap()
    ));
    assert!(PlanarProfileRef::sketch_profiles(
        cadmpeg_ir::sketches::SketchId::mint("test:test:sketch#0").unwrap(),
        Vec::new(),
        &cadmpeg_test_support::service_decode_context(),
    )
    .expect("profile membership admission")
    .is_err());
    assert!(path_ref_is_incomplete(&PathRef::Curves(Vec::new())));
    assert!(path_ref_is_incomplete(
        &cadmpeg_ir::features::NativeSelections::try_from(vec!["nx:path-selection#0".into()])
            .map(
                |selections| cadmpeg_ir::features::PathRef::SpatialSketchSelection {
                    sketch: cadmpeg_ir::sketches::SpatialSketchId::mint(
                        "test:test:spatial-sketch#0"
                    )
                    .unwrap(),
                    selections
                }
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
        LoftPointSection::Native(
            cadmpeg_core::text::NonBlankString::try_from("nx:point-selection#0").unwrap(),
        )
    )));
    assert!(!loft_section_is_incomplete(&LoftSection::Point(
        LoftPointSection::Point(
            cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0))
                .unwrap(),
        )
    )));
}

#[test]
fn nx_pattern_completeness_requires_distinct_seeds() {
    use cadmpeg_ir::features::{patterns::PatternSeed, BodySelection, FaceSelection};

    let seed_id =
        cadmpeg_ir::features::FeatureId::mint("test:test:feature#seed").expect("identity grammar");
    let seed = cadmpeg_ir::features::patterns::PatternSeed::Feature(seed_id.clone());
    let pattern = cadmpeg_ir::features::patterns::PatternKind::new(
        cadmpeg_ir::features::patterns::PatternTransform::Mirror {
            plane_origin: cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(
                0.0, 0.0, 0.0,
            ))
            .unwrap(),
            plane_normal: cadmpeg_ir::features::FeatureDirection3::new(
                cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        },
    )
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
        &[PatternSeed::Bodies(BodySelection::Bodies(
            cadmpeg_ir::features::DistinctMembers::try_from(
                vec![cadmpeg_ir::ids::BodyId::mint("test:model:body#seed")
                    .expect("identity grammar"),],
                &cadmpeg_test_support::service_decode_context()
            )
            .expect("distinct bodies")
        ))],
        &pattern,
        &[],
    ));
}

#[test]
fn nx_replace_face_completeness_requires_resolved_disjoint_operands() {
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, FeatureOperation};
    use cadmpeg_ir::ids::FaceId;

    let target = FaceId::mint("test:model:face#target").expect("identity grammar");
    let complete_targets = FaceSelection::Faces(vec![target.clone()]);
    let complete_replacements =
        FaceSelection::Faces(vec![
            FaceId::mint("test:model:face#replacement").expect("identity grammar")
        ]);

    assert_eq!(
        FeatureDefinition::Operation(FeatureOperation::ReplaceFace {
            operands: cadmpeg_ir::features::ReplaceFaceOperands::new(
                complete_targets.clone(),
                complete_replacements.clone(),
                &cadmpeg_test_support::service_decode_context(),
            )
            .expect("operand admission")
            .unwrap(),
        })
        .body_output_family(),
        Some("replace face")
    );
    assert!(!face_selection_is_incomplete(&complete_targets));
    assert!(!face_selection_is_incomplete(&complete_replacements));
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
        vec![GeneratedCurveRef::new(
            producer.clone(),
            "curve-0".into(),
            &cadmpeg_test_support::service_decode_context(),
        )
        .expect("selection reference admission")
        .unwrap()],
        "test:profile-selection".into(),
        &cadmpeg_test_support::service_decode_context(),
    )
    .expect("selection reference admission")
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
        features::{
            holes::{HoleKind, HolePlacement},
            FaceSelection, LinearTermination,
        },
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
fn nx_body_operation_completeness_requires_distinct_members() {
    use cadmpeg_ir::features::BodySelection;
    use cadmpeg_ir::ids::BodyId;

    let shared = BodyId::mint("test:model:body#shared").expect("identity grammar");
    let target = BodySelection::Bodies(
        cadmpeg_ir::features::DistinctMembers::try_from(
            vec![shared.clone()],
            &cadmpeg_test_support::service_decode_context(),
        )
        .expect("distinct bodies"),
    );

    assert!(cadmpeg_ir::features::DistinctMembers::try_from(
        vec![shared.clone(), shared.clone(),],
        &cadmpeg_test_support::service_decode_context()
    )
    .is_err());
    assert!(!body_selection_is_incomplete(&target));
}
