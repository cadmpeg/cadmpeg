// SPDX-License-Identifier: Apache-2.0
//! Feature-completeness predicates owned by `decode`.

#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]

#[test]
fn nx_hole_completeness_accepts_independent_placement_and_rejects_opaque_operands() {
    use cadmpeg_ir::features::{
        FaceSelection, HoleKind, HolePlacement, Length, LinearTermination, ProfileRef,
    };
    use cadmpeg_ir::math::{Point3, Vector3};

    let directed = HolePlacement::Directed {
        position: Point3::new(1.0, 2.0, 3.0),
        direction: Vector3::new(0.0, 0.0, 1.0),
    };
    let invalid_directed = HolePlacement::Directed {
        position: Point3::new(f64::NAN, 2.0, 3.0),
        direction: Vector3::new(0.0, 0.0, 1.0),
    };
    assert!(!super::hole_feature_is_incomplete(
        None,
        None,
        Some(std::slice::from_ref(&directed)),
        (&HoleKind::Simple, None),
        Some(Length(5.0)),
        Some(&LinearTermination::ThroughAll),
    ));
    assert!(super::hole_feature_is_incomplete(
        None,
        None,
        Some(std::slice::from_ref(&invalid_directed)),
        (&HoleKind::Simple, None),
        Some(Length(5.0)),
        Some(&LinearTermination::ThroughAll),
    ));
    assert!(!super::hole_feature_is_incomplete(
        None,
        None,
        Some(std::slice::from_ref(&directed)),
        (&HoleKind::Simple, None),
        Some(Length(5.0)),
        Some(&LinearTermination::ThroughAll),
    ));
    let axis = HolePlacement::Axis {
        origin: Point3::new(1.0, 2.0, 3.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
    };
    assert!(!super::hole_feature_is_incomplete(
        None,
        None,
        Some(std::slice::from_ref(&axis)),
        (&HoleKind::Simple, None),
        Some(Length(5.0)),
        Some(&LinearTermination::ThroughAll),
    ));
    for (placements, exit, extent) in [
        (
            vec![axis.clone()],
            None,
            LinearTermination::Blind {
                length: Length(10.0),
            },
        ),
        (
            vec![axis],
            Some(HoleKind::Chamfer {
                diameter: Length(7.0),
                angle: cadmpeg_ir::features::Angle(0.5),
            }),
            LinearTermination::ThroughAll,
        ),
        (
            vec![directed.clone(), directed.clone()],
            None,
            LinearTermination::ThroughAll,
        ),
    ] {
        assert!(super::hole_feature_is_incomplete(
            None,
            None,
            Some(&placements),
            (&HoleKind::Simple, exit.as_ref()),
            Some(Length(5.0)),
            Some(&extent),
        ));
    }
    assert!(super::hole_feature_is_incomplete(
        Some(&ProfileRef::Unresolved("hole".into())),
        Some(&FaceSelection::Unresolved),
        None,
        (&HoleKind::Simple, None),
        Some(Length(5.0)),
        Some(&LinearTermination::ThroughAll),
    ));
    assert!(super::hole_feature_is_incomplete(
        None,
        None,
        Some(std::slice::from_ref(&directed)),
        (&HoleKind::Simple, None),
        Some(Length(5.0)),
        Some(&LinearTermination::Unresolved),
    ));
    assert!(super::hole_feature_is_incomplete(
        None,
        None,
        Some(std::slice::from_ref(&directed)),
        (
            &HoleKind::Simple,
            Some(&HoleKind::Unresolved(Some(
                cadmpeg_ir::features::HoleForm::Chamfer,
            ))),
        ),
        Some(Length(5.0)),
        Some(&LinearTermination::ThroughAll),
    ));
    assert!(super::hole_feature_is_incomplete(
        None,
        None,
        Some(std::slice::from_ref(&directed)),
        (&HoleKind::Simple, None),
        Some(Length(0.0)),
        Some(&LinearTermination::ThroughAll),
    ));
    assert!(super::hole_feature_is_incomplete(
        None,
        None,
        Some(std::slice::from_ref(&directed)),
        (
            &HoleKind::Chamfer {
                diameter: Length(7.0),
                angle: cadmpeg_ir::features::Angle(f64::NAN),
            },
            None,
        ),
        Some(Length(5.0)),
        Some(&LinearTermination::ThroughAll),
    ));
    for kind in [
        HoleKind::Chamfer {
            diameter: Length(5.0),
            angle: cadmpeg_ir::features::Angle(0.5),
        },
        HoleKind::Counterbore {
            diameter: Length(4.0),
            depth: Length(2.0),
        },
        HoleKind::CounterboreDrilled {
            diameter: Length(5.0),
            depth: Length(2.0),
            drill_point_angle: cadmpeg_ir::features::Angle(0.5),
        },
        HoleKind::Countersink {
            diameter: Length(4.0),
            angle: cadmpeg_ir::features::Angle(0.5),
        },
        HoleKind::Counterdrill {
            diameter: Length(5.0),
            entry_diameter: None,
            depth: Length(2.0),
            angle: cadmpeg_ir::features::Angle(0.5),
        },
    ] {
        assert!(super::hole_feature_is_incomplete(
            None,
            None,
            Some(std::slice::from_ref(&directed)),
            (&kind, None),
            Some(Length(5.0)),
            Some(&LinearTermination::ThroughAll),
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

    assert!(!super::datum_plane_is_incomplete(origin, z_axis, x_axis,));
    assert!(super::datum_plane_is_incomplete(
        origin,
        z_axis,
        Vector3::new(1.0, 0.0, 1.0),
    ));
    assert!(super::datum_plane_is_incomplete(
        Point3::new(f64::NAN, 2.0, 3.0),
        z_axis,
        x_axis,
    ));

    assert!(!super::datum_coordinate_system_is_incomplete(
        origin, x_axis, y_axis, z_axis,
    ));
    assert!(super::datum_coordinate_system_is_incomplete(
        origin,
        x_axis,
        y_axis,
        Vector3::new(0.0, 0.0, -1.0),
    ));
    assert!(super::datum_coordinate_system_is_incomplete(
        origin,
        Vector3::new(2.0, 0.0, 0.0),
        y_axis,
        z_axis,
    ));
    assert!(super::datum_coordinate_system_is_incomplete(
        origin,
        x_axis,
        Vector3::new(1.0e-6, 1.0, 0.0),
        z_axis,
    ));
}

#[test]
fn nx_hole_completeness_checks_nested_auxiliary_semantics() {
    use cadmpeg_ir::features::{
        Angle, HoleBottom, HoleProfileFilter, HoleSpecification, HoleThreadDepth, Length,
        ThreadHand,
    };

    assert!(!super::hole_auxiliary_semantics_are_incomplete(
        Some(&HoleProfileFilter {
            points: true,
            circles: false,
            arcs: false,
        }),
        Some(&HoleBottom::Angled {
            included_angle: Angle(0.5),
            depth_to_tip: true,
        }),
        Some(Angle(0.1)),
        None,
    ));
    assert!(super::hole_auxiliary_semantics_are_incomplete(
        Some(&HoleProfileFilter {
            points: false,
            circles: false,
            arcs: false,
        }),
        None,
        None,
        None,
    ));
    assert!(super::hole_auxiliary_semantics_are_incomplete(
        None,
        Some(&HoleBottom::Angled {
            included_angle: Angle(f64::NAN),
            depth_to_tip: false,
        }),
        None,
        None,
    ));
    assert!(super::hole_auxiliary_semantics_are_incomplete(
        None,
        None,
        Some(Angle(std::f64::consts::PI)),
        None,
    ));
    let invalid_specification = HoleSpecification::Threaded {
        standard: " ".into(),
        designation: None,
        class: None,
        modeled: false,
        cosmetic: true,
        pitch: Some(Length(0.0)),
        major_diameter: Some(Length(5.0)),
        hand: ThreadHand::Right,
        depth: HoleThreadDepth::Blind { depth: Length(0.0) },
        clearance: None,
    };
    assert!(super::hole_auxiliary_semantics_are_incomplete(
        None,
        None,
        None,
        Some(&invalid_specification),
    ));
}

#[test]
fn nx_projected_curve_completeness_requires_a_valid_direction_law() {
    use cadmpeg_ir::features::{CurveProjectionDirection, CurveProjectionDirectionState};
    use cadmpeg_ir::math::Vector3;

    assert!(!super::projected_curve_direction_is_incomplete(
        CurveProjectionDirection::State(CurveProjectionDirectionState::TargetNormal),
    ));
    assert!(!super::projected_curve_direction_is_incomplete(
        CurveProjectionDirection::Vector(Vector3::new(0.0, 0.0, 1.0)),
    ));
    assert!(super::projected_curve_direction_is_incomplete(
        CurveProjectionDirection::State(CurveProjectionDirectionState::Unresolved),
    ));
    assert!(super::projected_curve_direction_is_incomplete(
        CurveProjectionDirection::Vector(Vector3::new(0.0, 0.0, 0.0)),
    ));
    assert!(super::projected_curve_direction_is_incomplete(
        CurveProjectionDirection::Vector(Vector3::new(f64::NAN, 0.0, 1.0)),
    ));
}

#[test]
fn nx_extent_completeness_checks_nested_and_face_termination() {
    use cadmpeg_ir::features::FeatureId;
    use cadmpeg_ir::features::{
        ExtrudeExtent, ExtrudeSide, ExtrudeStart, FaceSelection, GeneratedVertexRef, Length,
        LinearTermination, VertexSelection,
    };

    let side = |termination: LinearTermination| ExtrudeSide {
        termination,
        draft: None,
    };

    assert!(!super::extrude_extent_is_incomplete(
        &ExtrudeExtent::TwoSided {
            first: side(LinearTermination::Blind {
                length: Length(5.0),
            }),
            second: side(LinearTermination::ThroughAll),
        },
        &[],
    ));
    assert!(super::extrude_extent_is_incomplete(
        &ExtrudeExtent::Symmetric {
            side: side(LinearTermination::Unresolved),
        },
        &[],
    ));
    assert!(super::extrude_extent_is_incomplete(
        &ExtrudeExtent::OneSided {
            side: side(LinearTermination::Blind {
                length: Length(f64::NAN),
            }),
        },
        &[],
    ));
    assert!(super::extrude_extent_is_incomplete(
        &ExtrudeExtent::OneSided {
            side: ExtrudeSide {
                termination: LinearTermination::ThroughAll,
                draft: Some(cadmpeg_ir::features::Angle(std::f64::consts::FRAC_PI_2,)),
            },
        },
        &[],
    ));
    assert!(super::termination_is_incomplete(
        &LinearTermination::ToFace {
            face: FaceSelection::Native("nx:face-selection#0".to_string()),
            offset: None,
        }
    ));
    assert!(super::termination_is_incomplete(
        &LinearTermination::ToShape {
            target: FaceSelection::Resolved {
                faces: Vec::new(),
                native: "nx:face-selection#1".to_string(),
            },
        }
    ));
    assert!(super::termination_is_incomplete(
        &LinearTermination::OffsetFromFace {
            face: FaceSelection::Native("nx:face-selection#2".to_string()),
            offset: Length(1.0),
        }
    ));
    assert!(super::termination_is_incomplete(
        &LinearTermination::ToVertex {
            vertex: VertexSelection::Native("nx:vertex-selection#0".to_string()),
        }
    ));
    let vertex_feature = FeatureId("test:feature#0".into());
    let generated_vertex = LinearTermination::ToVertex {
        vertex: VertexSelection::Generated {
            vertex: GeneratedVertexRef {
                feature: vertex_feature.clone(),
                local_id: "vertex-0".into(),
            },
            native: "nx:vertex-selection#1".into(),
        },
    };
    assert!(!super::termination_is_incomplete(&generated_vertex));
    assert!(super::termination_dependency_is_incomplete(
        &generated_vertex,
        &[],
    ));
    assert!(!super::termination_dependency_is_incomplete(
        &generated_vertex,
        &[vertex_feature],
    ));
    assert!(super::extrude_start_is_incomplete(
        &ExtrudeStart::FromFace {
            face: FaceSelection::Native("nx:face-selection#3".to_string()),
            offset: None,
        }
    ));
    assert!(!super::extrude_start_is_incomplete(
        &ExtrudeStart::FromFace {
            face: FaceSelection::Faces(vec![cadmpeg_ir::ids::FaceId("test:face#start".into(),)]),
            offset: None,
        }
    ));
    assert!(super::extrude_start_is_incomplete(
        &ExtrudeStart::OffsetProfilePlane {
            offset: Length(f64::INFINITY),
        }
    ));
}

#[test]
fn nx_rib_completeness_requires_a_resolved_profile() {
    use cadmpeg_ir::features::{BooleanOp, Length, ProfileRef, RibConstruction, RibDraft, RibSide};
    use cadmpeg_ir::math::Vector3;

    let mut construction = RibConstruction {
        profile: Some(ProfileRef::Native("nx:profile#0".to_string())),
        direction: Some(Vector3::new(0.0, 0.0, 1.0)),
        thickness: Some(Length(2.0)),
        side: Some(RibSide::Centered),
        draft: RibDraft::None,
    };
    assert!(super::rib_feature_is_incomplete(
        &construction,
        BooleanOp::Join,
    ));
    construction.profile = Some(ProfileRef::Faces(vec![cadmpeg_ir::ids::FaceId(
        "face#0".to_string(),
    )]));
    assert!(!super::rib_feature_is_incomplete(
        &construction,
        BooleanOp::Join,
    ));
    construction.direction = Some(Vector3::new(0.0, 0.0, 0.0));
    assert!(super::rib_feature_is_incomplete(
        &construction,
        BooleanOp::Join,
    ));
    construction.direction = Some(Vector3::new(0.0, 0.0, 1.0));
    construction.thickness = Some(Length(0.0));
    assert!(super::rib_feature_is_incomplete(
        &construction,
        BooleanOp::Join,
    ));
    construction.thickness = Some(Length(2.0));
    construction.profile = Some(ProfileRef::Faces(Vec::new()));
    assert!(super::rib_feature_is_incomplete(
        &construction,
        BooleanOp::Join,
    ));
    construction.profile = Some(ProfileRef::Faces(vec![cadmpeg_ir::ids::FaceId(
        "face#0".to_string(),
    )]));
    construction.draft = RibDraft::Angle(cadmpeg_ir::features::Angle(std::f64::consts::FRAC_PI_2));
    assert!(super::rib_feature_is_incomplete(
        &construction,
        BooleanOp::Join,
    ));
}

#[test]
fn nx_loft_completeness_validates_point_sections() {
    use cadmpeg_ir::features::{LoftPointSection, LoftSection};
    use cadmpeg_ir::ids::VertexId;
    use cadmpeg_ir::math::Point3;

    assert!(!super::loft_section_is_incomplete(&LoftSection::Point(
        LoftPointSection::Point(Point3::new(1.0, 2.0, 3.0))
    ),));
    assert!(super::loft_section_is_incomplete(&LoftSection::Point(
        LoftPointSection::Point(Point3::new(1.0, f64::NAN, 3.0,))
    ),));
    assert!(super::loft_section_is_incomplete(&LoftSection::Point(
        LoftPointSection::Vertex(VertexId(" ".into()))
    ),));
}

#[test]
fn nx_sweep_completeness_checks_nested_mode_and_orientation_operands() {
    use cadmpeg_ir::features::{PathRef, SweepMode, SweepOrientation};

    assert!(super::sweep_mode_is_incomplete(SweepMode::Unresolved));
    assert!(!super::sweep_mode_is_incomplete(SweepMode::NewBody));
    assert!(super::sweep_orientation_is_incomplete(
        &SweepOrientation::Auxiliary {
            path: PathRef::Native("nx:auxiliary-path#0".into()),
            tangent: false,
            curvilinear: false,
        }
    ));
    assert!(!super::sweep_orientation_is_incomplete(
        &SweepOrientation::Auxiliary {
            path: PathRef::Curves(vec![cadmpeg_ir::ids::CurveId(
                "test:curve#auxiliary".into(),
            )]),
            tangent: false,
            curvilinear: false,
        }
    ));
    assert!(super::sweep_orientation_is_incomplete(
        &SweepOrientation::Binormal {
            direction: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 0.0),
        }
    ));
}

#[test]
fn nx_pattern_completeness_requires_every_regeneration_operand() {
    use cadmpeg_ir::features::{
        Length, PathRef, PatternKind, PatternStage, PatternStageCombination,
    };
    use cadmpeg_ir::math::Vector3;

    let linear = PatternKind::Linear {
        direction: Some(Vector3::new(1.0, 0.0, 0.0)),
        spacing: Length(10.0),
        count: 3,
        second: None,
    };
    assert!(!super::pattern_is_incomplete(&linear));
    assert!(super::pattern_is_incomplete(&PatternKind::Linear {
        direction: None,
        spacing: Length(10.0),
        count: 3,
        second: None,
    }));
    assert!(super::pattern_is_incomplete(&PatternKind::Linear {
        direction: Some(Vector3::new(1.0, 0.0, 0.0)),
        spacing: Length(0.0),
        count: 3,
        second: None,
    }));
    assert!(super::pattern_is_incomplete(&PatternKind::Linear {
        direction: Some(Vector3::new(0.0, 0.0, 0.0)),
        spacing: Length(10.0),
        count: 3,
        second: None,
    }));
    assert!(super::pattern_is_incomplete(&PatternKind::Linear {
        direction: Some(Vector3::new(1.0, 0.0, 0.0)),
        spacing: Length(10.0),
        count: 1,
        second: None,
    }));
    assert!(super::pattern_is_incomplete(&PatternKind::CurveDriven {
        path: Some(PathRef::Native("nx:path".into())),
        spacing: Length(10.0),
        count: 3,
    }));
    assert!(super::pattern_is_incomplete(&PatternKind::Composite {
        stages: vec![PatternStage {
            pattern: Box::new(PatternKind::Linear {
                direction: None,
                spacing: Length(10.0),
                count: 3,
                second: None,
            }),
            combination: PatternStageCombination::Initialize,
        }],
    }));
    assert!(super::pattern_is_incomplete(&PatternKind::Composite {
        stages: vec![
            PatternStage {
                pattern: Box::new(linear.clone()),
                combination: PatternStageCombination::Initialize,
            },
            PatternStage {
                pattern: Box::new(PatternKind::Scale {
                    center: cadmpeg_ir::features::PatternScaleCenter::FirstSeedCentroid,
                    final_factor: 2.0,
                    count: 2,
                }),
                combination: PatternStageCombination::AlignedSlices,
            },
        ],
    }));
    let composite = PatternKind::Composite {
        stages: vec![
            PatternStage {
                pattern: Box::new(linear),
                combination: PatternStageCombination::Initialize,
            },
            PatternStage {
                pattern: Box::new(PatternKind::Mirror {
                    plane_origin: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                    plane_normal: Vector3::new(1.0, 0.0, 0.0),
                }),
                combination: PatternStageCombination::CartesianProduct,
            },
        ],
    };
    assert!(!super::pattern_is_incomplete(&composite));
    assert_eq!(super::pattern_occurrence_count(&composite), Some(6));
}

#[test]
fn nx_variable_radius_completeness_requires_a_law_interval() {
    use cadmpeg_ir::features::{Length, RadiusSpec, VariableRadius};

    assert!(super::radius_spec_is_incomplete(&RadiusSpec::Variable {
        points: Vec::new()
    }));
    assert!(super::radius_spec_is_incomplete(&RadiusSpec::Variable {
        points: vec![VariableRadius {
            parameter: 0.0,
            radius: Length(2.0),
        }],
    }));
    assert!(super::radius_spec_is_incomplete(&RadiusSpec::Variable {
        points: vec![
            VariableRadius {
                parameter: 0.5,
                radius: Length(2.0),
            },
            VariableRadius {
                parameter: 0.5,
                radius: Length(3.0),
            },
        ],
    }));
    assert!(!super::radius_spec_is_incomplete(&RadiusSpec::Variable {
        points: vec![
            VariableRadius {
                parameter: 0.0,
                radius: Length(2.0),
            },
            VariableRadius {
                parameter: 1.0,
                radius: Length(3.0),
            },
        ],
    }));
    assert!(!super::radius_spec_is_incomplete(&RadiusSpec::Constant {
        radius: Length(2.0),
    }));
    assert!(super::radius_spec_is_incomplete(&RadiusSpec::Constant {
        radius: Length(0.0),
    }));
}

#[test]
fn nx_selection_completeness_requires_nonempty_unique_identities() {
    use cadmpeg_ir::features::{
        BodySelection, EdgeSelection, FaceSelection, LoftPointSection, LoftSection, PathRef,
        ProfileRef,
    };

    assert!(super::body_selection_is_incomplete(&BodySelection::Bodies(
        Vec::new()
    )));
    assert!(!super::body_selection_is_incomplete(
        &BodySelection::Local {
            bodies: vec!["nx:om-body-object#12".into()],
            native: "nx:om-object-index#12".into(),
        }
    ));
    assert!(super::body_selection_is_incomplete(&BodySelection::Local {
        bodies: vec!["nx:om-body-object#12".into(), "nx:om-body-object#12".into()],
        native: "nx:om-object-indices#12,13".into(),
    }));
    assert!(super::face_selection_is_incomplete(
        &FaceSelection::Resolved {
            faces: Vec::new(),
            native: "nx:faces".into(),
        }
    ));
    assert!(super::edge_selection_is_incomplete(&EdgeSelection::Edges(
        Vec::new()
    )));
    assert!(!super::edge_selection_is_incomplete(&EdgeSelection::All));
    assert!(super::profile_ref_is_incomplete(&ProfileRef::Faces(
        Vec::new()
    )));
    assert!(super::profile_ref_is_incomplete(
        &ProfileRef::SketchSelection {
            sketch: cadmpeg_ir::sketches::SketchId("test:sketch#0".into()),
            selections: vec!["nx:sketch-selection#0".into()],
        }
    ));
    assert!(super::profile_ref_is_incomplete(
        &ProfileRef::SketchProfiles {
            sketch: cadmpeg_ir::sketches::SketchId("test:sketch#0".into()),
            profiles: Vec::new(),
        }
    ));
    assert!(super::path_ref_is_incomplete(&PathRef::Curves(Vec::new())));
    assert!(super::path_ref_is_incomplete(
        &PathRef::SpatialSketchSelection {
            sketch: cadmpeg_ir::sketches::SpatialSketchId("test:spatial-sketch#0".into()),
            selections: vec!["nx:path-selection#0".into()],
        }
    ));
    let edge = cadmpeg_ir::ids::EdgeId("edge#0".into());
    assert!(super::path_ref_is_incomplete(&PathRef::Edges(vec![
        edge.clone(),
        edge
    ])));
    let curve = cadmpeg_ir::ids::CurveId("curve#0".into());
    assert!(super::path_ref_is_incomplete(&PathRef::Curves(vec![
        curve.clone(),
        curve
    ])));
    assert!(super::loft_section_is_incomplete(&LoftSection::Point(
        LoftPointSection::Native("nx:point-selection#0".into(),)
    )));
    assert!(!super::loft_section_is_incomplete(&LoftSection::Point(
        LoftPointSection::Point(cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0),)
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
    let definition = |sections, centerline| FeatureDefinition::Loft {
        sections,
        centerline,
        guides: Vec::new(),
        op: BooleanOp::NewBody,
        closed: false,
        solid: true,
        ruled: false,
        linearize: false,
        max_degree: None,
        allow_multi_profile_faces: None,
    };
    ir.model.features.push(Feature {
        id: FeatureId("test:feature#loft".into()),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: Vec::new(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: vec![output],
        definition: definition(
            vec![
                LoftSection::Point(LoftPointSection::Point(Point3::new(0.0, 0.0, 0.0))),
                LoftSection::Point(LoftPointSection::Point(Point3::new(0.0, 0.0, 1.0))),
            ],
            Some(PathRef::Native("nx:centerline#0".into())),
        ),
        native_ref: None,
    });

    let mut losses = Vec::new();
    super::append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("loft (1)"));

    ir.model.features[0].definition = definition(
        vec![
            LoftSection::Point(LoftPointSection::Native("nx:point#0".into())),
            LoftSection::Point(LoftPointSection::Point(Point3::new(0.0, 0.0, 1.0))),
        ],
        None,
    );
    losses.clear();
    super::append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("loft (1)"));

    ir.model.features[0].definition = definition(
        vec![
            LoftSection::Point(LoftPointSection::Point(Point3::new(0.0, 0.0, 0.0))),
            LoftSection::Point(LoftPointSection::Point(Point3::new(0.0, 0.0, 1.0))),
        ],
        None,
    );
    losses.clear();
    super::append_design_intent_losses(&ir, &mut losses);
    assert!(losses.is_empty());
}

#[test]
fn nx_pattern_completeness_requires_distinct_seeds() {
    use cadmpeg_ir::features::{BodySelection, FaceSelection, PatternSeed};

    let seed_id = cadmpeg_ir::features::FeatureId("test:feature#seed".into());
    let seed = cadmpeg_ir::features::PatternSeed::Feature(seed_id.clone());
    let pattern = cadmpeg_ir::features::PatternKind::Mirror {
        plane_origin: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
        plane_normal: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
    };

    assert!(!super::pattern_feature_is_incomplete(
        std::slice::from_ref(&seed),
        &pattern,
        std::slice::from_ref(&seed_id),
    ));
    assert!(super::pattern_feature_is_incomplete(
        std::slice::from_ref(&seed),
        &pattern,
        &[],
    ));
    assert!(super::pattern_feature_is_incomplete(
        &[seed.clone(), seed],
        &pattern,
        std::slice::from_ref(&seed_id),
    ));
    assert!(super::pattern_feature_is_incomplete(
        &[PatternSeed::Faces(FaceSelection::Native(
            "nx:pattern-face-selection#0".into(),
        ))],
        &pattern,
        &[],
    ));
    assert!(super::pattern_feature_is_incomplete(
        &[PatternSeed::Bodies(BodySelection::Unresolved)],
        &pattern,
        &[],
    ));
    assert!(!super::pattern_feature_is_incomplete(
        &[PatternSeed::Bodies(BodySelection::Bodies(vec![
            cadmpeg_ir::ids::BodyId("test:body#seed".into()),
        ]))],
        &pattern,
        &[],
    ));
}

#[test]
fn nx_face_blend_completeness_requires_disjoint_supports() {
    use cadmpeg_ir::features::FaceSelection;
    use cadmpeg_ir::ids::FaceId;

    let shared = FaceId("test:face#shared".into());
    let distinct = FaceId("test:face#distinct".into());
    let first = FaceSelection::Faces(vec![shared.clone()]);

    assert!(super::face_selections_overlap(
        &first,
        &FaceSelection::Resolved {
            faces: vec![shared],
            native: "test:first-support".into(),
        },
    ));
    assert!(!super::face_selections_overlap(
        &first,
        &FaceSelection::Faces(vec![distinct]),
    ));
    assert!(!super::face_selections_overlap(
        &first,
        &FaceSelection::Unresolved,
    ));
}

#[test]
fn nx_replace_face_completeness_requires_resolved_disjoint_operands() {
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition};
    use cadmpeg_ir::ids::FaceId;

    let target = FaceId("test:face#target".into());
    let complete_targets = FaceSelection::Faces(vec![target.clone()]);
    let complete_replacements = FaceSelection::Faces(vec![FaceId("test:face#replacement".into())]);
    let overlapping_replacements = FaceSelection::Resolved {
        faces: vec![target],
        native: "test:replacement".into(),
    };

    assert_eq!(
        FeatureDefinition::ReplaceFace {
            targets: complete_targets.clone(),
            replacements: complete_replacements.clone(),
        }
        .body_output_family(),
        Some("replace face")
    );
    assert!(!super::face_selection_is_incomplete(&complete_targets));
    assert!(!super::face_selection_is_incomplete(&complete_replacements));
    assert!(!super::face_selections_overlap(
        &complete_targets,
        &complete_replacements
    ));
    assert!(super::face_selections_overlap(
        &complete_targets,
        &overlapping_replacements
    ));
}

#[test]
fn nx_extrude_completeness_requires_direction_start_and_solid_state() {
    use cadmpeg_ir::features::{
        BooleanOp, ExtrudeDirection, ExtrudeExtent, ExtrudeSide, ExtrudeStart, Feature,
        FeatureDefinition, FeatureId, FeatureResultTopology, Length, LinearTermination, ProfileRef,
    };
    use cadmpeg_ir::ids::FeatureResultTopologyId;

    let mut ir = cadmpeg_ir::examples::unit_cube();
    let output = ir.model.bodies[0].id.clone();
    let definition = |direction, start, solid| FeatureDefinition::Extrude {
        profile: ProfileRef::Sketch(cadmpeg_ir::sketches::SketchId("test:sketch#0".into())),
        direction,
        start,
        extent: ExtrudeExtent::OneSided {
            side: ExtrudeSide {
                termination: LinearTermination::Blind {
                    length: Length(5.0),
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
        ExtrudeDirection::ProfileNormal,
        ExtrudeStart::ProfilePlane,
        Some(true),
    );
    ir.model.features.push(Feature {
        id: FeatureId("test:feature#extrude".into()),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: Vec::new(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: vec![output],
        definition: complete.clone(),
        native_ref: None,
    });

    let mut losses = Vec::new();
    super::append_design_intent_losses(&ir, &mut losses);
    assert!(losses.is_empty());

    for incomplete in [
        definition(
            ExtrudeDirection::Unresolved,
            ExtrudeStart::ProfilePlane,
            Some(true),
        ),
        definition(
            ExtrudeDirection::Explicit {
                vector: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 0.0),
                source: None,
            },
            ExtrudeStart::ProfilePlane,
            Some(true),
        ),
        definition(
            ExtrudeDirection::ProfileNormal,
            ExtrudeStart::Unresolved,
            Some(true),
        ),
        definition(
            ExtrudeDirection::ProfileNormal,
            ExtrudeStart::ProfilePlane,
            None,
        ),
    ] {
        ir.model.features[0].definition = incomplete;
        losses.clear();
        super::append_design_intent_losses(&ir, &mut losses);
        assert_eq!(losses.len(), 1);
        assert!(losses[0].message.contains("extrude (1)"));
    }

    ir.model.features[0].definition = complete;
    ir.model.features[0].outputs.clear();
    losses.clear();
    super::append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("extrude (1)"));

    ir.model
        .feature_result_topologies
        .push(FeatureResultTopology {
            id: FeatureResultTopologyId("test:feature-result#extrude".into()),
            output_of: ir.model.features[0].id.clone(),
            bodies: vec!["test:feature-local-body#0".into()],
            faces: Vec::new(),
            edges: Vec::new(),
            vertices: Vec::new(),
            native_ref: Some("test:native-body-writer#0".into()),
        });
    losses.clear();
    super::append_design_intent_losses(&ir, &mut losses);
    assert!(losses.is_empty());
}

#[test]
fn nx_revolve_completeness_checks_construction_and_output_lineage() {
    use cadmpeg_ir::features::{
        Angle, AngularTermination, BooleanOp, Feature, FeatureDefinition, FeatureId,
        GeneratedVertexRef, PathRef, ProfileRef, RevolutionAxis, RevolveConstruction,
        RevolveExtent, VertexSelection,
    };
    use cadmpeg_ir::math::{Point3, Vector3};

    let mut ir = cadmpeg_ir::examples::unit_cube();
    let output = ir.model.bodies[0].id.clone();
    let face = ir.model.faces[0].id.clone();
    let complete = RevolveConstruction::new(
        Some(ProfileRef::Faces(vec![face])),
        Some(RevolutionAxis {
            origin: Point3::new(0.0, 0.0, 0.0),
            direction: Vector3::new(0.0, 0.0, 1.0),
            reference: None,
        }),
        Some(RevolveExtent::OneSided {
            termination: AngularTermination::Angle { angle: Angle(1.0) },
        }),
        Some(true),
        None,
        None,
        None,
    );
    assert!(!super::revolve_feature_is_incomplete(
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
    assert!(super::revolve_feature_is_incomplete(
        &incomplete,
        BooleanOp::NewBody,
        &[],
    ));
    incomplete = complete.clone();
    incomplete.set_axis(None);
    assert!(super::revolve_feature_is_incomplete(
        &incomplete,
        BooleanOp::NewBody,
        &[],
    ));
    incomplete = complete.clone();
    incomplete.axis_mut().unwrap().direction = Vector3::new(0.0, 0.0, 2.0);
    assert!(super::revolve_feature_is_incomplete(
        &incomplete,
        BooleanOp::NewBody,
        &[],
    ));
    incomplete = complete.clone();
    incomplete.set_extent(None);
    assert!(super::revolve_feature_is_incomplete(
        &incomplete,
        BooleanOp::NewBody,
        &[],
    ));
    incomplete = complete.clone();
    incomplete.set_extent(Some(RevolveExtent::OneSided {
        termination: AngularTermination::Angle { angle: Angle(0.0) },
    }));
    assert!(super::revolve_feature_is_incomplete(
        &incomplete,
        BooleanOp::NewBody,
        &[],
    ));
    incomplete = complete.clone();
    incomplete.axis_mut().unwrap().reference = Some(PathRef::Native("test:axis".into()));
    assert!(super::revolve_feature_is_incomplete(
        &incomplete,
        BooleanOp::NewBody,
        &[],
    ));
    incomplete = complete.clone();
    incomplete.set_solid(None);
    assert!(super::revolve_feature_is_incomplete(
        &incomplete,
        BooleanOp::NewBody,
        &[],
    ));
    let source = FeatureId("test:feature#vertex-source".into());
    incomplete = complete.clone();
    incomplete.set_extent(Some(RevolveExtent::OneSided {
        termination: AngularTermination::ToVertex {
            vertex: VertexSelection::Generated {
                vertex: GeneratedVertexRef {
                    feature: source.clone(),
                    local_id: "vertex-0".into(),
                },
                native: "test:vertex-selection".into(),
            },
        },
    }));
    assert!(super::revolve_feature_is_incomplete(
        &incomplete,
        BooleanOp::NewBody,
        &[],
    ));
    assert!(!super::revolve_feature_is_incomplete(
        &incomplete,
        BooleanOp::NewBody,
        &[source],
    ));
    assert!(super::revolve_feature_is_incomplete(
        &complete,
        BooleanOp::Unresolved,
        &[],
    ));

    ir.model.features.push(Feature {
        id: FeatureId("test:feature#revolve".into()),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: Vec::new(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: vec![output],
        definition: FeatureDefinition::Revolve {
            construction: complete,
            op: BooleanOp::NewBody,
        },
        native_ref: None,
    });
    let mut losses = Vec::new();
    super::append_design_intent_losses(&ir, &mut losses);
    assert!(losses.is_empty());

    ir.model.features[0].outputs.clear();
    super::append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("revolve (1)"));
}

#[test]
fn nx_selection_completeness_rejects_repeated_faces_and_edges() {
    use cadmpeg_ir::features::{
        EdgeSelection, FaceSelection, FeatureId, GeneratedCurveRef, ProfileRef,
    };
    use cadmpeg_ir::ids::{EdgeId, FaceId};

    let face = FaceId("test:face#repeated".into());
    assert!(super::face_selection_is_incomplete(&FaceSelection::Faces(
        vec![face.clone(), face]
    ),));

    let face = FaceId("test:profile-face#repeated".into());
    assert!(super::profile_ref_is_incomplete(&ProfileRef::Faces(vec![
        face.clone(),
        face
    ]),));
    let producer = FeatureId("test:feature#profile-producer".into());
    let generated = ProfileRef::Generated {
        curves: vec![GeneratedCurveRef {
            feature: producer.clone(),
            local_id: "curve-0".into(),
        }],
        native: "test:profile-selection".into(),
    };
    assert!(!super::profile_ref_is_incomplete(&generated));
    assert!(super::profile_dependency_is_incomplete(&generated, &[],));
    assert!(!super::profile_dependency_is_incomplete(
        &generated,
        std::slice::from_ref(&producer),
    ));
    let direct = ProfileRef::Feature(producer.clone());
    assert!(super::profile_dependency_is_incomplete(&direct, &[],));
    assert!(!super::profile_dependency_is_incomplete(
        &direct,
        &[producer],
    ));

    let edge = EdgeId("test:edge#repeated".into());
    assert!(super::edge_selection_is_incomplete(&EdgeSelection::Edges(
        vec![edge.clone(), edge]
    ),));
}

#[test]
fn nx_hole_completeness_rejects_opaque_supplied_operands() {
    use cadmpeg_ir::features::{
        FaceSelection, HoleKind, HolePlacement, Length, LinearTermination, ProfileRef,
    };
    use cadmpeg_ir::math::{Point3, Vector3};

    let placement = HolePlacement::Directed {
        position: Point3::new(0.0, 0.0, 0.0),
        direction: Vector3::new(0.0, 0.0, 1.0),
    };
    let incomplete = |profile, face| {
        super::hole_feature_is_incomplete(
            profile,
            face,
            Some(std::slice::from_ref(&placement)),
            (&HoleKind::Simple, None),
            Some(Length(1.0)),
            Some(&LinearTermination::ThroughAll),
        )
    };

    assert!(!incomplete(None, None));
    let unresolved_profile = ProfileRef::Unresolved("hole".into());
    assert!(incomplete(Some(&unresolved_profile), None));
    assert!(incomplete(None, Some(&FaceSelection::Unresolved)));
}

#[test]
fn nx_sketch_completeness_reports_native_geometry_and_constraints() {
    use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId};
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::sketches::{
        Sketch, SketchConstraint, SketchConstraintDefinition, SketchConstraintId, SketchEntity,
        SketchEntityId, SketchGeometry, SketchId,
    };

    let mut ir = cadmpeg_ir::examples::unit_cube();
    let sketch_id = SketchId("test:sketch#0".into());
    ir.model.features.push(Feature {
        id: FeatureId("test:feature#sketch".into()),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: Vec::new(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Sketch {
            sketch: Some(sketch_id.clone()),
        },
        native_ref: None,
    });
    ir.model.sketches.push(Sketch {
        id: sketch_id.clone(),
        name: None,
        configuration: None,
        visible: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::Resolved {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        profiles: Vec::new(),
        native_ref: None,
    });
    let entity_id = SketchEntityId("test:sketch-entity#0".into());
    ir.model.sketch_entities.push(SketchEntity::new(
        entity_id.clone(),
        sketch_id.clone(),
        SketchGeometry::Native {
            native_kind: "test".into(),
        },
    ));
    ir.model.sketch_constraints.push(SketchConstraint {
        id: SketchConstraintId("test:sketch-constraint#0".into()),
        sketch: sketch_id,
        definition: SketchConstraintDefinition::Native {
            native_kind: "test".into(),
            entities: vec![entity_id],
            parameter: None,
            operands: Vec::new(),
            native_state: None,
            native_flags: None,
            native_properties: std::collections::BTreeMap::new(),
        },
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
    super::append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0]
        .message
        .contains("1 NX sketch geometry record(s) and 1 sketch constraint"));
}

#[test]
fn nx_body_operation_completeness_requires_disjoint_roles() {
    use cadmpeg_ir::features::BodySelection;
    use cadmpeg_ir::ids::BodyId;

    let shared = BodyId("test:body#shared".into());
    let distinct = BodyId("test:body#distinct".into());
    let target = BodySelection::Bodies(vec![shared.clone()]);

    assert!(super::body_selection_is_incomplete(&BodySelection::Bodies(
        vec![shared.clone(), shared.clone()]
    ),));
    assert!(!super::body_selection_is_incomplete(&target));

    assert!(super::body_selections_overlap(
        &target,
        &BodySelection::Resolved {
            bodies: vec![shared],
            native: "test:tools".into(),
        },
    ));
    assert!(!super::body_selections_overlap(
        &target,
        &BodySelection::Bodies(vec![distinct]),
    ));
    assert!(!super::body_selections_overlap(
        &target,
        &BodySelection::Unresolved,
    ));
    assert!(super::body_selections_overlap(
        &BodySelection::Local {
            bodies: vec!["nx:om-body-object#10".into()],
            native: "nx:om-object-index#10".into(),
        },
        &BodySelection::Local {
            bodies: vec!["nx:om-body-object#10".into()],
            native: "nx:om-object-index#20".into(),
        },
    ));
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
        id: ConfigurationId("test:configuration#0".into()),
        ordinal: 0,
        active: true,
        source_index: Some(0),
        name: "Model".into(),
        material: None,
        properties: Default::default(),
        parameter_overrides: Default::default(),
        bodies: ConfigurationBodies::Resolved(Vec::new()),
        parameter_values: Default::default(),
        feature_states: Default::default(),
        native_ref: None,
    });

    let mut losses = Vec::new();
    super::append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("1 NX design configuration"));

    ir.model.configurations[0].bodies = ConfigurationBodies::Resolved(bodies);
    losses.clear();
    super::append_design_intent_losses(&ir, &mut losses);
    assert!(losses.is_empty());

    ir.model.configurations[0].active = false.into();
    losses.clear();
    super::append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("1 NX design configuration"));

    ir.model.configurations[0].active = true.into();
    let output = ir.model.bodies[0].id.clone();
    let feature = Feature {
        id: FeatureId("test:feature#base".into()),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: Vec::new(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: vec![output.clone()],
        definition: FeatureDefinition::BaseFeature {
            bodies: BodySelection::Bodies(vec![output]),
        },
        native_ref: None,
    };
    ir.model.features.push(feature.clone());
    losses.clear();
    super::append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("1 NX design configuration"));

    ir.model.configurations[0].feature_states.insert(
        feature.id.clone(),
        ConfigurationFeatureState {
            suppressed: false,
            dependencies: feature.dependencies.clone(),
            outputs: feature.outputs.clone(),
            definition: feature.definition.clone(),
        },
    );
    losses.clear();
    super::append_design_intent_losses(&ir, &mut losses);
    assert!(losses.is_empty());

    let parameter = DesignParameter {
        id: ParameterId("test:parameter#length".into()),
        owner: Some(feature.id),
        ordinal: 0,
        name: "length".into(),
        expression: "2".into(),
        display: None,
        value: Some(ParameterValue::Real(2.0)),
        dependencies: Vec::new(),
        properties: Default::default(),
        pmi: None,
        native_ref: None,
    };
    ir.model.parameters.push(parameter.clone());
    losses.clear();
    super::append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("1 NX design configuration"));

    ir.model.configurations[0]
        .parameter_values
        .insert(parameter.id, parameter.value.expect("evaluated parameter"));
    losses.clear();
    super::append_design_intent_losses(&ir, &mut losses);
    assert!(losses.is_empty());

    let suppressed = Feature {
        id: FeatureId("test:feature#suppressed".into()),
        ordinal: 1,
        name: None,
        suppressed: Some(true),
        dependencies: Vec::new(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::DatumPoint {
            position: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            construction: None,
        },
        native_ref: None,
    };
    ir.model.features.push(suppressed.clone());
    losses.clear();
    super::append_design_intent_losses(&ir, &mut losses);
    assert!(losses
        .iter()
        .any(|loss| loss.message.contains("1 NX design configuration")));

    ir.model.configurations[0].feature_states.insert(
        suppressed.id.clone(),
        ConfigurationFeatureState {
            suppressed: true,
            dependencies: suppressed.dependencies,
            outputs: Vec::new(),
            definition: suppressed.definition,
        },
    );
    losses.clear();
    super::append_design_intent_losses(&ir, &mut losses);
    assert!(losses.is_empty());
}

#[test]
fn nx_body_producing_feature_families_require_history_outputs() {
    use cadmpeg_ir::features::{BooleanOp, Feature, FeatureDefinition, FeatureId, Length};
    use std::collections::BTreeMap;

    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.model.features.push(Feature {
        id: FeatureId("test:feature#block".into()),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Block {
            dimensions: Some([Length(1.0), Length(2.0), Length(3.0)]),
            placement: Some(cadmpeg_ir::transform::Transform::identity()),
            op: BooleanOp::NewBody,
        },
        native_ref: None,
    });

    let mut losses = Vec::new();
    super::append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("block (1)"));

    let output = cadmpeg_ir::ids::BodyId("test:body#output".into());
    ir.model.features[0].outputs = vec![output.clone()];
    losses.clear();
    super::append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("block (1)"));

    ir.model.features[0].outputs = vec![output.clone(), output.clone()];
    losses.clear();
    super::append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("block (1)"));

    ir.model.features[0].suppressed = Some(true);
    losses.clear();
    super::append_design_intent_losses(&ir, &mut losses);
    assert!(losses.is_empty());

    for invalid_placement in [
        {
            let mut placement = cadmpeg_ir::transform::Transform::identity();
            placement.rows[3][0] = 1.0;
            placement
        },
        {
            let mut placement = cadmpeg_ir::transform::Transform::identity();
            placement.rows[0][0] = 2.0;
            placement
        },
    ] {
        ir.model.features[0].definition = FeatureDefinition::Block {
            dimensions: Some([Length(1.0), Length(2.0), Length(3.0)]),
            placement: Some(invalid_placement),
            op: BooleanOp::NewBody,
        };
        super::append_design_intent_losses(&ir, &mut losses);
        assert_eq!(losses.len(), 1);
        assert!(losses[0].message.contains("block (1)"));
        losses.clear();
    }

    ir.model.features[0].definition = FeatureDefinition::Loft {
        sections: Vec::new(),
        centerline: None,
        guides: Vec::new(),
        op: cadmpeg_ir::features::BooleanOp::Unresolved,
        closed: false,
        solid: false,
        ruled: false,
        linearize: false,
        max_degree: None,
        allow_multi_profile_faces: None,
    };
    super::append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("loft (1)"));

    ir.model.features[0].definition = FeatureDefinition::Draft {
        faces: cadmpeg_ir::features::FaceSelection::Unresolved,
        anchor: cadmpeg_ir::features::DraftAnchor::NeutralPlane {
            plane: cadmpeg_ir::features::FaceSelection::Unresolved,
            pull: Some(cadmpeg_ir::features::DraftPull {
                direction: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
                plane: None,
            }),
        },
        angle: Some(cadmpeg_ir::features::Angle(0.1)),
        outward: Some(false),
    };
    losses.clear();
    super::append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("draft (1)"));

    let draft = |pull_direction: Option<cadmpeg_ir::math::Vector3>, angle, outward| {
        FeatureDefinition::Draft {
            faces: cadmpeg_ir::features::FaceSelection::Faces(vec![cadmpeg_ir::ids::FaceId(
                "test:face#draft".into(),
            )]),
            anchor: cadmpeg_ir::features::DraftAnchor::NeutralPlane {
                plane: cadmpeg_ir::features::FaceSelection::Faces(vec![cadmpeg_ir::ids::FaceId(
                    "test:face#neutral".into(),
                )]),
                pull: pull_direction.map(|direction| cadmpeg_ir::features::DraftPull {
                    direction,
                    plane: None,
                }),
            },
            angle,
            outward,
        }
    };
    for incomplete in [
        draft(None, Some(cadmpeg_ir::features::Angle(0.1)), Some(false)),
        draft(
            Some(cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0)),
            None,
            Some(false),
        ),
        draft(
            Some(cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0)),
            Some(cadmpeg_ir::features::Angle(0.1)),
            None,
        ),
        draft(
            Some(cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0)),
            Some(cadmpeg_ir::features::Angle(std::f64::consts::FRAC_PI_2)),
            Some(false),
        ),
    ] {
        ir.model.features[0].definition = incomplete;
        losses.clear();
        super::append_design_intent_losses(&ir, &mut losses);
        assert_eq!(losses.len(), 1);
        assert!(losses[0].message.contains("draft (1)"));
    }

    ir.model.features[0].definition = FeatureDefinition::DatumOffsetPlane {
        reference: None,
        distance: Length(5.0),
    };
    losses.clear();
    super::append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("datum plane (1)"));

    let datum = FeatureId("test:feature#datum-source".into());
    ir.model.features[0].definition = FeatureDefinition::DatumOffsetPlane {
        reference: Some(cadmpeg_ir::features::DatumPlaneReference::Feature(
            datum.clone(),
        )),
        distance: Length(5.0),
    };
    losses.clear();
    super::append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("datum plane (1)"));

    ir.model.features[0].ordinal = 1;
    ir.model.features.push(Feature {
        id: datum.clone(),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::DatumPrincipalPlane {
            plane: cadmpeg_ir::features::PrincipalPlane::Top,
        },
        native_ref: None,
    });
    losses.clear();
    super::append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("datum plane (1)"));

    ir.model.features[0].dependencies.push(datum);
    losses.clear();
    super::append_design_intent_losses(&ir, &mut losses);
    assert!(losses.is_empty());

    ir.model.features[0].definition = FeatureDefinition::SewBodies {
        bodies: cadmpeg_ir::features::BodySelection::Bodies(vec![output.clone()]),
        gap_tolerance: Some(Length(0.01)),
    };
    losses.clear();
    super::append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("sew bodies (1)"));

    ir.model.features[0].definition = FeatureDefinition::SewBodies {
        bodies: cadmpeg_ir::features::BodySelection::Local {
            bodies: vec![output.0.clone()],
            native: "nx:body-selection#sew".into(),
        },
        gap_tolerance: Some(Length(0.01)),
    };
    losses.clear();
    super::append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("sew bodies (1)"));

    ir.model.features[0].definition = FeatureDefinition::Combine {
        target: cadmpeg_ir::features::BodySelection::Local {
            bodies: vec!["target-a".into(), "target-b".into()],
            native: "nx:body-selection#targets".into(),
        },
        tools: cadmpeg_ir::features::BodySelection::Local {
            bodies: vec!["tool".into()],
            native: "nx:body-selection#tools".into(),
        },
        op: cadmpeg_ir::features::BooleanKind::Join,
        keep_tools: false,
    };
    losses.clear();
    super::append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("body combine (1)"));

    ir.model.features[0].definition = FeatureDefinition::BaseFeature {
        bodies: cadmpeg_ir::features::BodySelection::Unresolved,
    };
    losses.clear();
    super::append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("base feature (1)"));

    assert_eq!(
        FeatureDefinition::DatumPointUnresolved.body_output_family(),
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
            centerline: None,
            guides: Vec::new(),
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
                    direction: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
                    plane: None,
                }),
            },
            angle: Some(cadmpeg_ir::features::Angle(0.1)),
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
        id: FeatureId("test:feature#initial-bodies".into()),
        ordinal: 0,
        name: Some("Retained history input".into()),
        suppressed: Some(false),
        dependencies: Vec::new(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::BaseFeature {
            bodies: BodySelection::Resolved {
                bodies: Vec::new(),
                native: "nx:segment-body-bindings".into(),
            },
        },
        native_ref: None,
    });

    let mut losses = Vec::new();
    super::append_design_intent_losses(&ir, &mut losses);

    assert!(losses.is_empty());
}

#[test]
fn nx_master_snapshot_base_feature_is_an_output_free_replay_boundary() {
    use std::collections::BTreeMap;

    use cadmpeg_ir::features::{BodySelection, Feature, FeatureDefinition, FeatureId};

    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.model.features.push(Feature {
        id: FeatureId("test:feature#snapshot".into()),
        ordinal: 0,
        name: Some("MASTER SNAPSHOT BODY".into()),
        suppressed: Some(false),
        dependencies: Vec::new(),
        source_properties: BTreeMap::from([(
            String::from("operation_record"),
            String::from("record"),
        )]),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::BaseFeature {
            bodies: BodySelection::Unresolved,
        },
        native_ref: None,
    });

    let mut losses = Vec::new();
    super::append_design_intent_losses(&ir, &mut losses);

    assert!(losses.is_empty());
}

#[test]
fn nx_sew_completeness_does_not_invent_a_gap_tolerance() {
    use cadmpeg_ir::features::{BodySelection, Feature, FeatureDefinition, FeatureId, Length};

    let mut ir = cadmpeg_ir::examples::unit_cube();
    let first = ir.model.bodies[0].id.clone();
    let mut second_body = ir.model.bodies[0].clone();
    second_body.id = cadmpeg_ir::ids::BodyId("test:body#second".into());
    let second = second_body.id.clone();
    ir.model.bodies.push(second_body);
    ir.model.features.push(Feature {
        id: FeatureId("test:feature#sew".into()),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: Vec::new(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: vec![first.clone()],
        definition: FeatureDefinition::SewBodies {
            bodies: BodySelection::Bodies(vec![first, second]),
            gap_tolerance: None,
        },
        native_ref: None,
    });

    let mut losses = Vec::new();
    super::append_design_intent_losses(&ir, &mut losses);
    assert!(losses.is_empty());

    for tolerance in [Length(0.0), Length(-0.01), Length(f64::NAN)] {
        let FeatureDefinition::SewBodies { gap_tolerance, .. } =
            &mut ir.model.features[0].definition
        else {
            unreachable!();
        };
        *gap_tolerance = Some(tolerance);
        losses.clear();
        super::append_design_intent_losses(&ir, &mut losses);
        assert_eq!(losses.len(), 1);
        assert!(losses[0].message.contains("sew bodies (1)"));
    }
}

#[test]
fn nx_shell_completeness_requires_each_construction_field() {
    use cadmpeg_ir::features::{
        BodySelection, FaceSelection, FeatureDefinition, Length, ShellJoin, ShellMode,
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
    assert!(super::shell_definition_is_incomplete(&incomplete));

    let complete = FeatureDefinition::Shell {
        bodies: Some(BodySelection::Bodies(vec![BodyId(
            "test:body#shell".into(),
        )])),
        removed_faces: FaceSelection::Faces(vec![FaceId("test:face#opening".into())]),
        thickness: Some(Length(2.0)),
        outward: Some(false),
        mode: Some(ShellMode::Skin),
        join: Some(ShellJoin::Intersection),
        resolve_intersections: Some(true),
        allow_self_intersections: Some(false),
    };
    assert!(!super::shell_definition_is_incomplete(&complete));
    assert_eq!(complete.body_output_family(), Some("shell"));
}
