// SPDX-License-Identifier: Apache-2.0
use crate::examples::unit_cube;
use crate::features::{DistinctMembers, FeatureContent};
use crate::features::{PlanarProfileRef, ProfileRef};
use crate::math::{Point3, Vector3};
use crate::validate::validate_neutral;

#[test]
fn a_standalone_feature_wire_declares_no_regeneration_parent() {
    use crate::features::{Feature, FeatureDefinition, FeatureId, FeatureOperation};

    let feature = Feature::new(
        FeatureId::mint("test:model:feature#child").expect("identity grammar"),
        0,
        FeatureDefinition::Operation(FeatureOperation::StoredGeometry {}),
    );
    let mut wire = serde_json::to_value(&feature).unwrap();
    assert_eq!(
        serde_json::from_value::<Feature>(wire.clone()).unwrap(),
        feature
    );

    wire["regeneration_parent"] = serde_json::json!("test:model:feature#parent");
    let error = serde_json::from_value::<Feature>(wire)
        .expect_err("a parent edge belongs to the model that owns it")
        .to_string();
    assert!(
        error.contains("unknown field `regeneration_parent`"),
        "{error}"
    );
}

#[test]
fn a_datum_plane_reference_states_which_support_it_names() {
    use crate::features::{DatumPlaneReference, FaceSelection, FeatureSupportPlaneFrame};

    let feature = DatumPlaneReference::Feature {
        feature: crate::features::FeatureId::mint("test:model:feature#feature")
            .expect("identity grammar"),
    };
    assert_eq!(
        serde_json::to_value(&feature).unwrap(),
        serde_json::json!({
            "reference": "feature",
            "feature": "test:model:feature#feature"
        })
    );

    let face = DatumPlaneReference::Face {
        face: FaceSelection::Faces(vec![
            crate::ids::FaceId::mint("test:model:face#face").expect("valid identity")
        ]),
    };
    assert_eq!(
        serde_json::to_value(&face).unwrap(),
        serde_json::json!({
            "reference": "face",
            "face": {"kind": "faces", "value": ["test:model:face#face"]}
        })
    );

    let resolved = DatumPlaneReference::ResolvedPlane {
        frame: FeatureSupportPlaneFrame::new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
    };
    assert_eq!(
        serde_json::to_value(&resolved).unwrap(),
        serde_json::json!({
            "reference": "resolved_plane",
            "frame": {
                "origin": {"x": 0.0, "y": 0.0, "z": 0.0},
                "normal": {"x": 0.0, "y": 0.0, "z": 1.0},
                "u_axis": {"x": 1.0, "y": 0.0, "z": 0.0}
            }
        })
    );

    for reference in [&feature, &face, &resolved] {
        let wire = serde_json::to_value(reference).unwrap();
        assert_eq!(
            &serde_json::from_value::<DatumPlaneReference>(wire).unwrap(),
            reference
        );
    }

    let mut legacy = serde_json::to_value(&resolved).unwrap();
    legacy["face"] = serde_json::json!({"kind": "unresolved"});
    let error = serde_json::from_value::<DatumPlaneReference>(legacy)
        .unwrap_err()
        .to_string();
    assert!(error.contains("face"), "{error}");

    let mut stray = serde_json::to_value(&face).unwrap();
    stray["origin"] = serde_json::json!({"x": 0.0, "y": 0.0, "z": 0.0});
    let error = serde_json::from_value::<DatumPlaneReference>(stray)
        .unwrap_err()
        .to_string();
    assert!(error.contains("origin"), "{error}");

    let error = serde_json::from_value::<DatumPlaneReference>(serde_json::json!(
        "test:model:feature#feature"
    ))
    .unwrap_err()
    .to_string();
    assert!(error.contains("invalid type: string"), "{error}");
}

#[test]
fn feature_extents_round_trip_through_json() {
    use crate::ids::FaceId;
    use crate::{
        features::{
            AngularTermination, ExtrudeExtent, ExtrudeSide, FaceSelection, LinearTermination,
            RevolveExtent,
        },
        scalar::Length,
    };

    let extents = vec![
        ExtrudeExtent::OneSided {
            side: ExtrudeSide {
                termination: LinearTermination::Blind {
                    length: crate::scalar::NonZeroLength::new(12.5).unwrap(),
                },
                draft: Some(crate::scalar::SlopeAngle::new(0.1).unwrap()),
            },
        },
        ExtrudeExtent::Symmetric {
            side: ExtrudeSide {
                termination: LinearTermination::Blind {
                    length: crate::scalar::NonZeroLength::new(25.0).unwrap(),
                },
                draft: None,
            },
        },
        ExtrudeExtent::TwoSided {
            first: ExtrudeSide {
                termination: LinearTermination::Blind {
                    length: crate::scalar::NonZeroLength::new(10.0).unwrap(),
                },
                draft: Some(crate::scalar::SlopeAngle::new(0.2).unwrap()),
            },
            second: ExtrudeSide {
                termination: LinearTermination::ToFace {
                    face: FaceSelection::Faces(vec![
                        FaceId::mint("synthetic:test:face#0").expect("valid identity")
                    ]),
                    offset: Some(Length::new(-2.0).unwrap()),
                },
                draft: None,
            },
        },
        ExtrudeExtent::OneSided {
            side: ExtrudeSide {
                termination: LinearTermination::ThroughAll {},
                draft: None,
            },
        },
    ];
    let json = serde_json::to_string(&extents).unwrap();
    assert_eq!(
        serde_json::from_str::<Vec<ExtrudeExtent>>(&json).unwrap(),
        extents
    );

    let revolve_extents = vec![
        RevolveExtent::OneSided {
            termination: AngularTermination::Angle {
                angle: crate::scalar::PositiveAngle::new(std::f64::consts::PI).unwrap(),
            },
        },
        RevolveExtent::Symmetric {
            termination: AngularTermination::Angle {
                angle: crate::scalar::PositiveAngle::new(std::f64::consts::FRAC_PI_2).unwrap(),
            },
        },
        RevolveExtent::TwoSided {
            first: AngularTermination::Angle {
                angle: crate::scalar::PositiveAngle::new(0.25).unwrap(),
            },
            second: AngularTermination::Angle {
                angle: crate::scalar::PositiveAngle::new(0.75).unwrap(),
            },
        },
    ];
    let json = serde_json::to_string(&revolve_extents).unwrap();
    assert_eq!(
        serde_json::from_str::<Vec<RevolveExtent>>(&json).unwrap(),
        revolve_extents
    );
}

#[test]
fn termination_families_preserve_wire_and_reject_cross_family_variants() {
    use crate::features::{AngularTermination, LinearTermination};

    let blind_wire = serde_json::json!({"kind": "blind", "length": 12.5});
    let blind: LinearTermination = serde_json::from_value(blind_wire.clone()).unwrap();
    assert_eq!(
        blind,
        LinearTermination::Blind {
            length: crate::scalar::NonZeroLength::new(12.5).unwrap()
        }
    );
    assert_eq!(serde_json::to_value(blind).unwrap(), blind_wire);
    assert!(serde_json::from_value::<AngularTermination>(blind_wire).is_err());

    let angle_wire = serde_json::json!({"kind": "angle", "angle": 1.25});
    let angle: AngularTermination = serde_json::from_value(angle_wire.clone()).unwrap();
    assert_eq!(
        angle,
        AngularTermination::Angle {
            angle: crate::scalar::PositiveAngle::new(1.25).unwrap()
        }
    );
    assert_eq!(serde_json::to_value(angle).unwrap(), angle_wire);
    assert!(serde_json::from_value::<LinearTermination>(angle_wire).is_err());
}

#[test]
fn loft_sections_preserve_profile_shape() {
    use crate::features::{
        BooleanOp, FeatureDefinition, FeatureOperation, LoftSection, PlanarProfileRef, ProfileRef,
    };

    let wire = serde_json::json!({
        "definition": "loft",
        "sections": [{"kind": "native", "value": "native:section"}],
        "guidance": {"kind": "guides", "path": []},
        "op": "new_body",
        "closed": false
    });
    let definition: FeatureDefinition = serde_json::from_value(wire).unwrap();
    assert!(matches!(
        &definition,
        FeatureDefinition::Operation(FeatureOperation::Loft {
            sections,
            guidance: crate::features::LoftGuidance::Guides(guides),
            op: BooleanOp::NewBody,
            closed: false,
            ..
        }) if sections == &vec![LoftSection::Profile(ProfileRef::Planar(PlanarProfileRef::Native("native:section".into())))]
            && guides.is_empty()
    ));
    let encoded = serde_json::to_value(definition).unwrap();
    assert_eq!(
        encoded["sections"][0],
        serde_json::json!({"kind": "native", "value": "native:section"})
    );
}

#[test]
fn generated_sweep_sections_round_trip_and_validate() {
    use crate::features::{
        Feature, FeatureDefinition, FeatureId, FeatureOperation, GeneratedSweepSection, SweepMode,
        SweepSection,
    };

    let definition = FeatureDefinition::Operation(FeatureOperation::Sweep {
        shape: crate::features::SweepShape::Solid {
            op: crate::features::SolidSweepOperation::NewBody,
            section: SweepSection::Generated(GeneratedSweepSection::CircularRegion {
                region: crate::features::SweepCircularRegion::new(
                    crate::scalar::PositiveLength::new(3.0).unwrap(),
                    Some(crate::scalar::PositiveLength::new(1.0).unwrap()),
                )
                .unwrap(),
            }),
            sections: Vec::new(),
        },

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
    });
    let json = serde_json::to_string(&definition).unwrap();
    assert!(json.contains("\"kind\":\"generated\""));
    assert!(json.contains("\"shape\":\"circular_region\""));
    assert_eq!(
        serde_json::from_str::<FeatureDefinition>(&json).unwrap(),
        definition
    );

    let validate_definition = |definition| {
        let mut ir = unit_cube();
        ir.model.features.push(Feature {
            id: FeatureId::mint("synthetic:test:feature#generated-sweep")
                .expect("identity grammar"),
            ordinal: 0,
            name: None,
            suppressed: Some(false),
            dependencies: DistinctMembers::default(),
            source_properties: std::collections::BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: FeatureContent::default(),

            evaluation: crate::features::FeatureEvaluation::from_definition(definition),
            native_ref: None,
        });
        ir.finalize();
        validate_neutral(&ir, Vec::new())
    };
    let report = validate_definition(definition.clone());
    assert!(report.is_ok(), "{report:#?}");

    assert!(crate::features::SweepCircularRegion::new(
        crate::scalar::PositiveLength::new(2.0).unwrap(),
        Some(crate::scalar::PositiveLength::new(2.0).unwrap()),
    )
    .is_err());
    let FeatureDefinition::Operation(FeatureOperation::Sweep { mut shape, .. }) = definition else {
        panic!("sweep fixture");
    };
    let before = shape.clone();
    assert!(shape.set_mode(SweepMode::Surface {}).is_err());
    assert_eq!(shape, before);
}

#[test]
fn full_round_fillet_keeps_automatic_side_semantics() {
    use crate::features::{
        FaceSelection, Feature, FeatureDefinition, FeatureId, FeatureOperation,
        FullRoundSideSelection,
    };

    let mut ir = unit_cube();
    let center = ir.model.faces[0].id.clone();
    let definition = FeatureDefinition::Operation(FeatureOperation::FullRoundFillet {
        groups: crate::features::NonEmptyMembers::one(
            crate::features::FullRoundFilletGroup::new(
                FaceSelection::Faces(vec![center.clone()]),
                FullRoundSideSelection::Automatic,
                FullRoundSideSelection::Automatic,
            )
            .unwrap(),
        ),
    });
    assert_eq!(
        serde_json::from_value::<FeatureDefinition>(serde_json::to_value(&definition).unwrap())
            .unwrap(),
        definition
    );
    ir.model.features.push(Feature {
        id: FeatureId::mint("synthetic:test:feature#full-round").expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: Some("Fillet".into()),
        source_text: None,
        source_content: FeatureContent::default(),

        evaluation: crate::features::FeatureEvaluation::from_definition(definition),
        native_ref: None,
    });
    assert!(!validate_neutral(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| {
            finding.entity.as_deref() == Some("synthetic:test:feature#full-round")
                && finding.message == "full-round fillet face sets are invalid"
        }));

    assert!(crate::features::FullRoundFilletGroup::new(
        FaceSelection::Faces(vec![center.clone()]),
        FullRoundSideSelection::Explicit(FaceSelection::Faces(vec![center])),
        FullRoundSideSelection::Automatic,
    )
    .is_err());
}

#[test]
fn flex_modes_round_trip_and_validate() {
    use crate::{
        features::FlexMode,
        scalar::{Angle, Length},
    };

    let modes = vec![
        FlexMode::Bending {
            angle: Angle::new(0.5).unwrap(),
        },
        FlexMode::Twisting {
            angle: Angle::new(1.0).unwrap(),
        },
        FlexMode::Tapering {
            factor: crate::scalar::PositiveReal::new(1.5).unwrap(),
        },
        FlexMode::Stretching {
            distance: Length::new(12.0).unwrap(),
        },
    ];
    let json = serde_json::to_string(&modes).unwrap();
    assert_eq!(serde_json::from_str::<Vec<FlexMode>>(&json).unwrap(), modes);
}

#[test]
fn unresolved_hole_and_flex_wire_forms_preserve_their_layout() {
    use crate::features::{FlexMode, HoleKind, PartialPair};

    let counterbore = serde_json::json!({
        "kind": "partial_counterbore",
        "dimension": {"kind": "diameter", "diameter": 10.0}
    });
    let kind: HoleKind = serde_json::from_value(counterbore.clone()).unwrap();
    assert_eq!(
        kind,
        HoleKind::PartialCounterbore(PartialPair::First(
            crate::scalar::PositiveLength::new(10.0).unwrap()
        ))
    );
    assert_eq!(serde_json::to_value(kind).unwrap(), counterbore);

    let flex = serde_json::json!({"kind": "unresolved", "form": "twisting"});
    let mode: FlexMode = serde_json::from_value(flex.clone()).unwrap();
    assert_eq!(
        mode,
        FlexMode::Unresolved {
            form: Some(crate::features::FlexForm::Twisting)
        }
    );
    assert_eq!(serde_json::to_value(mode).unwrap(), flex);
}

#[test]
fn an_unresolved_hole_wire_keeps_its_form_and_carries_no_dimensions() {
    use crate::features::{HoleForm, HoleKind};

    let unresolved = serde_json::json!({"kind": "unresolved", "form": "counterbore"});
    let kind: HoleKind = serde_json::from_value(unresolved.clone()).unwrap();
    assert_eq!(kind, HoleKind::Unresolved(Some(HoleForm::Counterbore)));
    assert_eq!(serde_json::to_value(kind).unwrap(), unresolved);

    let mut with_dimension = unresolved.clone();
    with_dimension["countersink_angle"] = serde_json::json!(0.5);
    let error = serde_json::from_value::<HoleKind>(with_dimension)
        .unwrap_err()
        .to_string();
    assert!(error.contains("countersink_angle"), "{error}");

    let formless = serde_json::json!({"kind": "unresolved"});
    let kind: HoleKind = serde_json::from_value(formless.clone()).unwrap();
    assert_eq!(kind, HoleKind::Unresolved(None));
    assert_eq!(serde_json::to_value(kind).unwrap(), formless);
}

#[test]
fn unresolved_flex_wire_forms_reject_cross_family_payloads() {
    use crate::features::FlexMode;

    assert!(serde_json::from_value::<FlexMode>(serde_json::json!({
        "kind": "unresolved",
        "form": "twisting",
        "factor": 2.0
    }))
    .is_err());
}

#[test]
fn hole_construction_forms_preserve_the_nested_shape_wire_layout() {
    use crate::features::{
        FeatureDefinition, FeatureOperation, HoleConstruction, HoleKind, HoleSpecification,
    };

    let standard = serde_json::json!({
        "definition": "hole",
        "shape": {
            "construction": {
            "construction": "form",
            "kind": {"kind": "simple"},
            "specification": {
                "kind": "clearance",
                "standard": "ISO metric",
                "designation": "M8",
                "fit": "normal",
                "modeled": false,
                "cosmetic": false,
                "hand": "right",
                "depth": {"kind": "hole_depth"}
            }
            }
        }
    });
    let definition: FeatureDefinition = serde_json::from_value(standard.clone()).unwrap();
    assert!(matches!(
        &definition,
        FeatureDefinition::Operation(FeatureOperation::Hole { shape, .. }) if matches!(shape.construction(),
            HoleConstruction::Form { kind: HoleKind::Simple, specification: Some(specification) }
            if matches!(specification.as_ref(), HoleSpecification::Clearance { .. }))
    ));
    assert_eq!(serde_json::to_value(definition).unwrap(), standard);

    let native_thread = serde_json::json!({
        "definition": "hole",
        "shape": {
            "construction": {
                "construction": "native_thread",
                "major_diameter": 8.0,
                "thread_depth": 12.0,
                "pitch": 1.25,
                "drill_point_angle": 2.0
            },
            "diameter": 6.0
        }
    });
    let definition: FeatureDefinition = serde_json::from_value(native_thread.clone()).unwrap();
    assert!(matches!(
        &definition,
        FeatureDefinition::Operation(FeatureOperation::Hole { shape, .. }) if matches!(shape.construction(), HoleConstruction::NativeThread { .. })
    ));
    assert_eq!(serde_json::to_value(definition).unwrap(), native_thread);
}

#[test]
fn unit_hole_wire_variants_reject_an_unknown_key_by_name() {
    use crate::features::{HoleBottom, HoleKind, HoleThreadDepth};

    fn round_trips_and_rejects_unknown_keys<T>(wire: serde_json::Value)
    where
        T: serde::Serialize + serde::de::DeserializeOwned + std::fmt::Debug,
    {
        let admitted: T = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(admitted).unwrap(), wire);

        let mut unknown = wire;
        unknown["zz_bogus"] = serde_json::json!(1);
        let error = serde_json::from_value::<T>(unknown)
            .unwrap_err()
            .to_string();
        assert!(error.contains("zz_bogus"), "{error}");
    }

    round_trips_and_rejects_unknown_keys::<HoleKind>(serde_json::json!({"kind": "simple"}));
    round_trips_and_rejects_unknown_keys::<HoleBottom>(serde_json::json!({"kind": "flat"}));
    round_trips_and_rejects_unknown_keys::<HoleThreadDepth>(
        serde_json::json!({"kind": "hole_depth"}),
    );
    round_trips_and_rejects_unknown_keys::<HoleThreadDepth>(
        serde_json::json!({"kind": "tapped_standard"}),
    );
}

#[test]
fn an_unknown_key_beside_the_hole_shape_is_rejected_by_name() {
    use crate::features::{FeatureDefinition, FeatureOperation};

    let hole = serde_json::json!({
        "definition": "hole",
        "shape": {"construction": {"construction": "form", "kind": {"kind": "simple"}}, "diameter": 3.0}
    });
    assert!(serde_json::from_value::<FeatureDefinition>(hole.clone()).is_ok());

    let mut beside_shape = hole.clone();
    beside_shape["zz_bogus"] = serde_json::json!(1);
    let error = serde_json::from_value::<FeatureOperation>(beside_shape)
        .unwrap_err()
        .to_string();
    assert!(error.contains("zz_bogus"), "{error}");

    let mut inside_shape = hole;
    inside_shape["shape"]["zz_bogus"] = serde_json::json!(1);
    let error = serde_json::from_value::<FeatureOperation>(inside_shape)
        .unwrap_err()
        .to_string();
    assert!(error.contains("zz_bogus"), "{error}");
}

#[test]
fn hole_wire_rejects_cross_form_thread_fields() {
    use crate::features::{FeatureDefinition, HoleSpecification};

    let specification = |kind| {
        serde_json::json!({
            "kind": kind,
            "standard": "ISO metric",
            "modeled": false,
            "cosmetic": false,
            "hand": "right",
            "depth": {"kind": "hole_depth"}
        })
    };

    // Thread data has no key on the clearance arm and a fit has none on the
    // threaded arm, so each is an unknown field rather than a refused value.
    for (kind, orphan, value) in [
        ("clearance", "class", serde_json::json!("6H")),
        ("clearance", "pitch", serde_json::json!(1.25)),
        ("clearance", "major_diameter", serde_json::json!(8.0)),
        ("threaded", "fit", serde_json::json!("normal")),
    ] {
        let mut wire = specification(kind);
        wire[orphan] = value;
        let error = serde_json::from_value::<HoleSpecification>(wire)
            .unwrap_err()
            .to_string();
        assert!(error.contains("unknown field"), "{kind}/{orphan}: {error}");
        assert!(error.contains(orphan), "{kind}/{orphan}: {error}");
    }
    assert!(
        serde_json::from_value::<HoleSpecification>(specification("threaded")).is_ok(),
        "the threaded arm reads without the thread fields"
    );
    let error = serde_json::from_value::<HoleSpecification>(serde_json::json!({
        "standard": "ISO metric",
        "threaded": true,
        "modeled": false,
        "cosmetic": false,
        "hand": "right",
        "depth": {"kind": "hole_depth"}
    }))
    .unwrap_err()
    .to_string();
    assert!(error.contains("kind"), "{error}");

    let mut native_with_standard = serde_json::json!({
        "definition": "hole",
        "shape": {
            "kind": {
                "kind": "threaded",
                "major_diameter": 8.0,
                "thread_depth": 12.0,
                "drill_point_angle": 2.0
            }
        }
    });
    native_with_standard["shape"]["specification"] = specification("threaded");
    assert!(serde_json::from_value::<FeatureDefinition>(native_with_standard).is_err());
}

#[test]
fn filled_surface_continuity_preserves_aggregate_and_component_wire_fields() {
    use crate::features::{
        EdgeSelection, FaceSelection, FeatureDefinition, FeatureOperation, SurfaceBoundary,
        SurfaceContinuity,
    };

    let definition = FeatureDefinition::Operation(FeatureOperation::FilledSurface {
        boundary: SurfaceBoundary::Edges(EdgeSelection::Unresolved),
        support_faces: FaceSelection::Faces(Vec::new()),
        continuity: crate::features::FilledSurfaceContinuityState::per_boundary(vec![
            SurfaceContinuity::Contact,
            SurfaceContinuity::Contact,
        ]),
        merge_result: Some(false),
    });
    let wire = serde_json::to_value(&definition).unwrap();
    assert_eq!(
        wire["continuity"],
        serde_json::json!(["contact", "contact"])
    );
    assert_eq!(
        serde_json::from_value::<FeatureDefinition>(wire.clone()).unwrap(),
        definition
    );

    let FeatureDefinition::Operation(FeatureOperation::FilledSurface { continuity, .. }) =
        &definition
    else {
        panic!("filled-surface fixture");
    };
    assert_eq!(continuity.uniform_value(), Some(SurfaceContinuity::Contact));

    let mut mixed = wire;
    mixed["continuity"] = serde_json::json!(["contact", "curvature"]);
    let FeatureDefinition::Operation(FeatureOperation::FilledSurface { continuity, .. }) =
        serde_json::from_value::<FeatureDefinition>(mixed).unwrap()
    else {
        panic!("filled-surface fixture");
    };
    assert_eq!(continuity.uniform_value(), None);
}

#[test]
fn unresolved_filled_surface_continuity_omits_both_wire_fields() {
    use crate::features::{
        EdgeSelection, FaceSelection, FeatureDefinition, FeatureOperation, SurfaceBoundary,
    };

    let definition = FeatureDefinition::Operation(FeatureOperation::FilledSurface {
        boundary: SurfaceBoundary::Edges(EdgeSelection::Unresolved),
        support_faces: FaceSelection::Faces(Vec::new()),
        continuity: crate::features::FilledSurfaceContinuityState::unresolved(),
        merge_result: None,
    });
    let wire = serde_json::to_value(&definition).unwrap();
    assert!(wire.get("continuity").is_none());
    assert_eq!(
        serde_json::from_value::<FeatureDefinition>(wire).unwrap(),
        definition
    );
}

#[test]
fn scale_factor_forms_name_themselves_on_the_wire() {
    use crate::features::ScaleFactors;

    for wire in [
        serde_json::json!({"kind": "unresolved"}),
        serde_json::json!({"kind": "uniform", "factor": 2.0}),
        serde_json::json!({"kind": "per_axis", "factors": [1.0, 2.0, 3.0]}),
    ] {
        let factors: ScaleFactors = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(factors).unwrap(), wire);
    }
}

#[test]
fn scale_factor_wire_rejects_mixed_and_partial_forms() {
    use crate::features::ScaleFactors;

    // The old flat wire spelled the form by which of four optional keys were
    // present, so a mixed or partial set was representable. Each form now
    // names itself and carries only its own field.
    for wire in [
        serde_json::json!({"uniform": 2.0, "x": 1.0}),
        serde_json::json!({"x": 1.0, "z": 3.0}),
        serde_json::json!({"kind": "uniform", "factor": 2.0, "factors": [1.0, 2.0, 3.0]}),
        serde_json::json!({"kind": "per_axis", "factors": [1.0, 3.0]}),
        serde_json::json!({}),
    ] {
        assert!(serde_json::from_value::<ScaleFactors>(wire).is_err());
    }
}

#[test]
fn edge_selections_round_trip_through_json() {
    use crate::features::EdgeSelection;
    use crate::ids::{EdgeId, FeatureInputTopologyId, HistoricalEdgeId};

    let selections = vec![
        EdgeSelection::Unresolved,
        EdgeSelection::Edges(vec![
            EdgeId::mint("synthetic:test:edge#0").expect("valid identity")
        ]),
        EdgeSelection::Resolved {
            edges: vec![EdgeId::mint("synthetic:test:edge#0").expect("valid identity")],
            native: "edge:10".into(),
        },
        EdgeSelection::historical(
            FeatureInputTopologyId::mint("synthetic:history-input:state#0")
                .expect("valid identity"),
            vec![HistoricalEdgeId::mint("synthetic:history-input:edge#0").expect("valid identity")],
            "edge:9".into(),
        )
        .unwrap(),
        EdgeSelection::historical_partial(
            FeatureInputTopologyId::mint("synthetic:history-input:state#0")
                .expect("valid identity"),
            vec![HistoricalEdgeId::mint("synthetic:history-input:edge#0").expect("valid identity")],
            vec!["native:edge-operand#1".into()],
            "edge:9".into(),
        )
        .unwrap(),
        EdgeSelection::Native("sldprt:history:feature#10:0".into()),
    ];
    let json = serde_json::to_string(&selections).unwrap();
    assert_eq!(
        serde_json::from_str::<Vec<EdgeSelection>>(&json).unwrap(),
        selections
    );
}

#[test]
fn historical_edge_paths_round_trip_through_json() {
    use crate::features::PathRef;
    use crate::ids::{FeatureInputTopologyId, HistoricalEdgeId};

    let path = PathRef::historical_edges(
        FeatureInputTopologyId::mint("synthetic:history-input:state#0").expect("valid identity"),
        vec![
            HistoricalEdgeId::mint("synthetic:history-input:edge#0").expect("valid identity"),
            HistoricalEdgeId::mint("synthetic:history-input:edge#1").expect("valid identity"),
        ],
        "native:path#0".into(),
    )
    .unwrap();
    let json = serde_json::to_string(&path).unwrap();
    assert_eq!(serde_json::from_str::<PathRef>(&json).unwrap(), path);
}

#[test]
fn face_selections_round_trip_through_json() {
    use crate::features::FaceSelection;
    use crate::ids::{FaceId, FeatureInputTopologyId, HistoricalFaceId};

    let selections = vec![
        FaceSelection::Unresolved,
        FaceSelection::Faces(vec![
            FaceId::mint("synthetic:test:face#0").expect("valid identity")
        ]),
        FaceSelection::Resolved {
            faces: vec![FaceId::mint("synthetic:test:face#0").expect("valid identity")],
            native: "face:14".into(),
        },
        FaceSelection::historical(
            FeatureInputTopologyId::mint("synthetic:history-input:state#0")
                .expect("valid identity"),
            vec![HistoricalFaceId::mint("synthetic:history-input:face#0").expect("valid identity")],
            "face:13".into(),
        )
        .unwrap(),
        FaceSelection::historical_partial(
            FeatureInputTopologyId::mint("synthetic:history-input:state#0")
                .expect("valid identity"),
            vec![HistoricalFaceId::mint("synthetic:history-input:face#0").expect("valid identity")],
            vec!["native:face-operand#1".into()],
            "face:12".into(),
        )
        .unwrap(),
        FaceSelection::Native("sldprt:history:feature#14:0".into()),
    ];
    let json = serde_json::to_string(&selections).unwrap();
    assert_eq!(
        serde_json::from_str::<Vec<FaceSelection>>(&json).unwrap(),
        selections
    );
}

#[test]
fn historical_face_profiles_round_trip_through_json() {
    use crate::ids::{FeatureInputTopologyId, HistoricalFaceId};

    let profile = PlanarProfileRef::historical_faces(
        FeatureInputTopologyId::mint("synthetic:history-input:state#0").expect("valid identity"),
        vec![HistoricalFaceId::mint("synthetic:history-input:face#0").expect("valid identity")],
        vec!["native:profile-group#0".into()],
    )
    .unwrap();
    let json = serde_json::to_string(&profile).unwrap();
    assert_eq!(
        serde_json::from_str::<ProfileRef>(&json).unwrap(),
        ProfileRef::Planar(profile)
    );
}

#[test]
fn body_selections_round_trip_through_json() {
    use crate::features::BodySelection;
    use crate::ids::{BodyId, FeatureInputTopologyId, HistoricalBodyId};

    let selections = vec![
        BodySelection::Unresolved,
        BodySelection::Bodies(vec![
            BodyId::mint("synthetic:test:body#0").expect("valid identity")
        ]),
        BodySelection::Resolved {
            bodies: vec![BodyId::mint("synthetic:test:body#0").expect("valid identity")],
            native: "body:17".into(),
        },
        BodySelection::ResolvedSet {
            members: crate::features::BodyMembers::try_from_parts(
                vec![
                    BodyId::mint("synthetic:test:body#0").expect("valid identity"),
                    BodyId::mint("synthetic:test:body#1").expect("valid identity"),
                ],
                vec!["body:17".into(), "body:18".into()],
            )
            .expect("valid body selection rows"),
        },
        BodySelection::historical(
            FeatureInputTopologyId::mint("synthetic:history-input:state#0")
                .expect("valid identity"),
            vec![HistoricalBodyId::mint("synthetic:history-input:body#0").expect("valid identity")],
            "body:16".into(),
        )
        .unwrap(),
        BodySelection::HistoricalSet {
            state: FeatureInputTopologyId::mint("synthetic:history-input:state#0")
                .expect("valid identity"),
            members: crate::features::BodyMembers::try_from_parts(
                vec![
                    HistoricalBodyId::mint("synthetic:history-input:body#0")
                        .expect("valid identity"),
                    HistoricalBodyId::mint("synthetic:history-input:body#1")
                        .expect("valid identity"),
                ],
                vec!["body:16".into(), "body:17".into()],
            )
            .expect("valid historical body selection rows"),
        },
        BodySelection::Native("body:17,body:18".into()),
        BodySelection::NativeSet(vec!["body:17".into(), "body:18".into()].try_into().unwrap()),
    ];
    let json = serde_json::to_string(&selections).unwrap();
    assert_eq!(
        serde_json::from_str::<Vec<BodySelection>>(&json).unwrap(),
        selections
    );
}

#[test]
fn body_selection_members_reject_blank_native_rows() {
    use crate::features::{BodyMember, BodyMembers};
    use crate::ids::BodyId;

    let body = BodyId::mint("synthetic:test:body#blank").expect("identity grammar");
    assert!(BodyMember::new(body.clone(), " \t".into()).is_err());
    assert!(BodyMembers::try_from_parts(vec![body.clone()], vec!["\n".into()]).is_err());
    assert!(
        serde_json::from_value::<BodyMembers<BodyId>>(serde_json::json!([
            {"body": body, "native": " "}
        ]))
        .is_err()
    );
}

#[test]
fn loft_guidance_rejects_mixed_wire_members() {
    use crate::features::{LoftGuidance, PathRef};

    let guidance = LoftGuidance::Centerline(PathRef::Native("test:centerline".into()));
    let wire = serde_json::to_value(&guidance).unwrap();
    assert_eq!(
        serde_json::from_value::<LoftGuidance>(wire).unwrap(),
        guidance
    );
    assert!(serde_json::from_value::<LoftGuidance>(serde_json::json!({
        "kind": "guides",
        "path": [],
        "centerline": {"kind": "native", "value": "test:centerline"}
    }))
    .is_err());
}

#[test]
fn feature_result_topology_round_trips_without_current_model_bodies() {
    use crate::features::{FeatureId, FeatureResultTopology};
    use crate::ids::FeatureResultTopologyId;

    let state = FeatureResultTopology::new(
        FeatureResultTopologyId::mint("synthetic:history-result:state#0").expect("valid identity"),
        FeatureId::mint("synthetic:model:feature#0").expect("identity grammar"),
        vec!["body:17".into()],
        vec!["face:3".into()],
        vec!["edge:5".into()],
        vec!["vertex:8".into()],
        Some("native:result#0".into()),
    )
    .unwrap();
    let json = serde_json::to_string(&state).unwrap();
    assert_eq!(
        serde_json::from_str::<FeatureResultTopology>(&json).unwrap(),
        state
    );
}

#[test]
fn combine_omits_the_default_keep_tools_flag_from_json() {
    use crate::features::{BodySelection, BooleanKind, FeatureDefinition, FeatureOperation};

    let definition = FeatureDefinition::Operation(FeatureOperation::Combine {
        operands: crate::features::CombineOperands::new(
            BodySelection::Native("body:17".into()),
            BodySelection::Native("body:18".into()),
        )
        .unwrap(),

        op: BooleanKind::Join,
        keep_tools: false,
    });
    let json = serde_json::to_value(definition).unwrap();
    assert_eq!(json.get("keep_tools"), None);
}

/// `new_body` is one solid sweep operation among four, so it has exactly one
/// spelling and an unknown key beside `op` is refused by name.
#[test]
fn sweep_mode_spells_new_body_once_and_refuses_a_key_beside_op() {
    use crate::features::{SolidSweepOperation, SweepMode};

    for (wire, expected) in [
        (
            serde_json::json!({"mode": "solid", "op": "new_body"}),
            SweepMode::Solid {
                op: SolidSweepOperation::NewBody,
            },
        ),
        (
            serde_json::json!({"mode": "solid", "op": "join"}),
            SweepMode::Solid {
                op: SolidSweepOperation::Join,
            },
        ),
        (
            serde_json::json!({"mode": "unresolved"}),
            SweepMode::Unresolved {},
        ),
        (
            serde_json::json!({"mode": "surface"}),
            SweepMode::Surface {},
        ),
    ] {
        assert_eq!(
            serde_json::from_value::<SweepMode>(wire.clone()).unwrap(),
            expected
        );
        assert_eq!(serde_json::to_value(expected).unwrap(), wire);
    }

    assert!(serde_json::from_value::<SweepMode>(
        serde_json::json!({"mode": "solid", "op": "unresolved"})
    )
    .is_err());

    let error = serde_json::from_value::<SweepMode>(
        serde_json::json!({"mode": "solid", "op": "join", "zz_bogus": 1}),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("unknown field `zz_bogus`"), "{error}");
}

/// The identified-but-undimensioned form is one state on the wire and in the
/// IR, so a dimension beside it is refused by name.
#[test]
fn an_unresolved_form_refuses_a_dimension_beside_it() {
    use crate::features::{ChamferSpec, PatternKind, RadiusSpec};

    let error = serde_json::from_value::<RadiusSpec>(
        serde_json::json!({"kind": "unresolved", "form": "chordal", "radius": 1}),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("unknown field `radius`"), "{error}");

    let error = serde_json::from_value::<ChamferSpec>(
        serde_json::json!({"kind": "unresolved", "form": "distance", "distance": 1}),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("unknown field `distance`"), "{error}");

    let error = serde_json::from_value::<PatternKind>(
        serde_json::json!({"kind": "unresolved", "form": "linear", "count": 2}),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("unknown field `count`"), "{error}");
}

#[test]
fn unresolved_feature_forms_preserve_the_legacy_wire_shape() {
    use crate::features::{ChamferSpec, PatternKind, RadiusSpec};

    for (wire, expected) in [
        (
            serde_json::json!({"kind": "unresolved"}),
            RadiusSpec::Unresolved { form: None },
        ),
        (
            serde_json::json!({"kind": "unresolved", "form": "constant"}),
            RadiusSpec::Unresolved {
                form: Some(crate::features::RadiusForm::Constant),
            },
        ),
        (
            serde_json::json!({"kind": "unresolved", "form": "variable"}),
            RadiusSpec::Unresolved {
                form: Some(crate::features::RadiusForm::Variable),
            },
        ),
    ] {
        assert_eq!(
            serde_json::from_value::<RadiusSpec>(wire.clone()).unwrap(),
            expected
        );
        assert_eq!(serde_json::to_value(expected).unwrap(), wire);
    }

    for (wire, expected) in [
        (
            serde_json::json!({"kind": "unresolved"}),
            ChamferSpec::Unresolved { form: None },
        ),
        (
            serde_json::json!({"kind": "unresolved", "form": "distance"}),
            ChamferSpec::Unresolved {
                form: Some(crate::features::ChamferForm::Distance),
            },
        ),
        (
            serde_json::json!({"kind": "unresolved", "form": "distance_angle"}),
            ChamferSpec::Unresolved {
                form: Some(crate::features::ChamferForm::DistanceAngle),
            },
        ),
    ] {
        assert_eq!(
            serde_json::from_value::<ChamferSpec>(wire.clone()).unwrap(),
            expected
        );
        assert_eq!(serde_json::to_value(expected).unwrap(), wire);
    }

    for (wire, expected) in [
        (
            serde_json::json!({"kind": "unresolved"}),
            PatternKind::UNRESOLVED,
        ),
        (
            serde_json::json!({"kind": "unresolved", "form": "linear"}),
            PatternKind::UNRESOLVED_LINEAR,
        ),
        (
            serde_json::json!({"kind": "unresolved", "form": "mirror"}),
            PatternKind::UNRESOLVED_MIRROR,
        ),
    ] {
        assert_eq!(
            serde_json::from_value::<PatternKind>(wire.clone()).unwrap(),
            expected
        );
        assert_eq!(serde_json::to_value(expected).unwrap(), wire);
    }
}

#[test]
fn an_unknown_hole_wire_key_is_rejected_by_name() {
    use crate::features::{HoleKind, HoleShape, HoleSpecification};

    let error = serde_json::from_value::<HoleKind>(serde_json::json!({
        "kind": "partial_counterbore",
        "diameter": 10.0,
        "counter_bore_depth": 4.0
    }))
    .unwrap_err()
    .to_string();
    assert!(error.contains("counter_bore_depth"), "{error}");

    let error = serde_json::from_value::<HoleShape>(serde_json::json!({
        "construction": {
            "construction": "form",
            "kind": {"kind": "partial_counterbore", "dimension": {"kind": "diameter", "diameter": 10.0}}
        },
        "diameter": 2.0,
        "counter_bore_depth": 4.0
    }))
    .unwrap_err()
    .to_string();
    assert!(error.contains("counter_bore_depth"), "{error}");

    let error = serde_json::from_value::<HoleShape>(serde_json::json!({
        "construction": {
            "construction": "native_thread",
            "major_diameter": 8.0,
            "thread_depth": 12.0,
            "drill_point_angle": 2.0,
            "specification": {
                "kind": "clearance",
                "standard": "ISO metric",
                "modeled": false,
                "cosmetic": false,
                "hand": "right",
                "depth": {"kind": "hole_depth"}
            }
        },
        "diameter": 2.0
    }))
    .unwrap_err()
    .to_string();
    assert!(error.contains("unknown field `specification`"), "{error}");

    let error = serde_json::from_value::<HoleSpecification>(serde_json::json!({
        "kind": "clearance",
        "standard": "iso",
        "modeled": false,
        "cosmetic": false,
        "hand": "right",
        "depth": {"kind": "hole_depth"},
        "clearence": 0.5
    }))
    .unwrap_err()
    .to_string();
    assert!(error.contains("clearence"), "{error}");
}

#[test]
fn every_partial_hole_pair_round_trips_under_its_own_dimension_key() {
    use crate::features::{HoleKind, PartialPair};

    let diameter = crate::scalar::PositiveLength::new(10.0).unwrap();
    let depth = crate::scalar::PositiveLength::new(4.0).unwrap();
    let angle = crate::scalar::InteriorAngle::new(1.5).unwrap();

    let cases = [
        (
            HoleKind::PartialCounterbore(PartialPair::First(diameter)),
            serde_json::json!({
                "kind": "partial_counterbore",
                "dimension": {"kind": "diameter", "diameter": 10.0}
            }),
        ),
        (
            HoleKind::PartialCounterbore(PartialPair::Second(depth)),
            serde_json::json!({
                "kind": "partial_counterbore",
                "dimension": {"kind": "depth", "depth": 4.0}
            }),
        ),
        (
            HoleKind::PartialCountersink(PartialPair::First(diameter)),
            serde_json::json!({
                "kind": "partial_countersink",
                "dimension": {"kind": "diameter", "diameter": 10.0}
            }),
        ),
        (
            HoleKind::PartialCountersink(PartialPair::Second(angle)),
            serde_json::json!({
                "kind": "partial_countersink",
                "dimension": {"kind": "angle", "angle": 1.5}
            }),
        ),
    ];
    for (kind, wire) in cases {
        assert_eq!(serde_json::to_value(kind).unwrap(), wire);
        assert_eq!(serde_json::from_value::<HoleKind>(wire).unwrap(), kind);
    }
}

#[test]
fn a_partial_hole_dimension_carries_exactly_one_measurement() {
    use crate::features::HoleKind;

    for (wire, rejected) in [
        (
            serde_json::json!({
                "kind": "partial_counterbore",
                "dimension": {"kind": "diameter", "diameter": 10.0, "depth": 4.0}
            }),
            "depth",
        ),
        (
            serde_json::json!({
                "kind": "partial_countersink",
                "dimension": {"kind": "diameter", "diameter": 10.0, "angle": 1.5}
            }),
            "angle",
        ),
    ] {
        let error = serde_json::from_value::<HoleKind>(wire)
            .unwrap_err()
            .to_string();
        assert!(error.contains(rejected), "{error}");
    }

    for wire in [
        serde_json::json!({"kind": "partial_counterbore", "dimension": {}}),
        serde_json::json!({"kind": "partial_countersink", "dimension": {}}),
    ] {
        assert!(serde_json::from_value::<HoleKind>(wire).is_err());
    }
}

/// Each of these six variants once flattened a `*_wire` module into the
/// definition object, which made `FeatureDefinition`'s `deny_unknown_fields`
/// inert on the variant. The nested key restores it.
fn assert_key_beside_the_nested_field_is_rejected(document: &serde_json::Value) {
    use crate::features::{FeatureDefinition, FeatureOperation};

    assert!(
        serde_json::from_value::<FeatureDefinition>(document.clone()).is_ok(),
        "{document}"
    );
    let mut beside = document.clone();
    beside["zz_bogus"] = serde_json::json!(1);
    let error = serde_json::from_value::<FeatureOperation>(beside)
        .unwrap_err()
        .to_string();
    assert!(error.contains("zz_bogus"), "{error}");
}

fn assert_key_inside_the_nested_field_is_rejected(document: &serde_json::Value, field: &str) {
    use crate::features::FeatureOperation;

    let mut inside = document.clone();
    inside[field]["zz_bogus"] = serde_json::json!(1);
    let error = serde_json::from_value::<FeatureOperation>(inside)
        .unwrap_err()
        .to_string();
    assert!(error.contains("zz_bogus"), "{error}");
}

#[test]
fn an_unknown_key_beside_the_helix_shape_is_rejected_by_name() {
    let helix = serde_json::json!({
        "definition": "helix",
        "axis_origin": {"x": 0.0, "y": 0.0, "z": 0.0},
        "axis_direction": {"x": 0.0, "y": 0.0, "z": 1.0},
        "radius": 2.0,
        "shape": {"kind": "spiral", "radial_growth": 1.5},
        "revolutions": 4.0,
        "start_angle": 0.0,
        "clockwise": false
    });
    assert_key_beside_the_nested_field_is_rejected(&helix);
    assert_key_inside_the_nested_field_is_rejected(&helix, "shape");
}

#[test]
fn an_unknown_key_beside_the_wrap_mode_is_rejected_by_name() {
    let wrap = serde_json::json!({
        "definition": "wrap",
        "profile": {"kind": "native", "value": "wrap:profile"},
        "face": {"kind": "native", "value": "wrap:face"},
        "mode": {"emboss": {"depth": 2.5}}
    });
    assert_key_beside_the_nested_field_is_rejected(&wrap);

    let mut inside = wrap;
    inside["mode"]["emboss"]["zz_bogus"] = serde_json::json!(1);
    let error = serde_json::from_value::<crate::features::FeatureOperation>(inside)
        .unwrap_err()
        .to_string();
    assert!(error.contains("zz_bogus"), "{error}");
}

#[test]
fn an_unknown_key_beside_the_sketch_binding_is_rejected_by_name() {
    let sketch = serde_json::json!({
        "definition": "sketch",
        "sketch": {"space": "planar", "sketch": "test:model:sketch#1"}
    });
    assert_key_beside_the_nested_field_is_rejected(&sketch);
    assert_key_inside_the_nested_field_is_rejected(&sketch, "sketch");
}

#[test]
fn an_unknown_key_beside_the_extrude_direction_is_rejected_by_name() {
    let extrude = serde_json::json!({
        "definition": "extrude",
        "profile": {"kind": "native", "value": "test:profile"},
        "direction": {
            "kind": "explicit",
            "vector": {"x": 0.0, "y": 1.0, "z": 0.0}
        },
        "start": {"kind": "profile_plane"},
        "extent": {
            "kind": "one_sided",
            "side": {"termination": {"kind": "blind", "length": 4.0}}
        },
        "op": "new_body"
    });
    assert_key_beside_the_nested_field_is_rejected(&extrude);
}

#[test]
fn an_unknown_key_beside_the_trim_region_is_rejected_by_name() {
    let trim = serde_json::json!({
        "definition": "trim_surface",
        "faces": {"kind": "unresolved"},
        "tool": {"kind": "unresolved", "value": "test:trim-tool"},
        "keep": {"cells": {"removed": [1, 4], "total": 5}}
    });
    assert_key_beside_the_nested_field_is_rejected(&trim);
}

#[test]
fn an_unknown_key_beside_the_draft_anchor_is_rejected_by_name() {
    let draft = serde_json::json!({
        "definition": "draft",
        "faces": {"kind": "native", "value": "draft:faces"},
        "anchor": {
            "kind": "parting_line",
            "tool": {"kind": "native", "value": "draft:parting-tool"},
            "pull": {"direction": {"x": 0.0, "y": 0.0, "z": 1.0}}
        },
        "angle": 0.1,
        "outward": false
    });
    assert_key_beside_the_nested_field_is_rejected(&draft);
    assert_key_inside_the_nested_field_is_rejected(&draft, "anchor");
}

#[test]
fn an_unknown_key_beside_the_stored_geometry_definition_is_rejected_by_name() {
    let stored = serde_json::json!({"definition": "stored_geometry"});
    assert_key_beside_the_nested_field_is_rejected(&stored);
}

#[test]
fn an_unknown_key_inside_the_extrude_direction_is_rejected_by_name() {
    let extrude = serde_json::json!({
        "definition": "extrude",
        "profile": {"kind": "native", "value": "test:profile"},
        "direction": {"kind": "profile_normal"},
        "start": {"kind": "profile_plane"},
        "extent": {
            "kind": "one_sided",
            "side": {"termination": {"kind": "blind", "length": 4.0}}
        },
        "op": "new_body"
    });
    assert_key_beside_the_nested_field_is_rejected(&extrude);
    assert_key_inside_the_nested_field_is_rejected(&extrude, "direction");

    for kind in ["unresolved", "reversed_profile_normal", "explicit"] {
        let mut variant = extrude.clone();
        variant["direction"] = if kind == "explicit" {
            serde_json::json!({
                "kind": "explicit",
                "vector": {"x": 0.0, "y": 1.0, "z": 0.0}
            })
        } else {
            serde_json::json!({"kind": kind})
        };
        assert_key_beside_the_nested_field_is_rejected(&variant);
        assert_key_inside_the_nested_field_is_rejected(&variant, "direction");
    }
}

#[test]
fn an_unknown_key_inside_the_trim_cell_selection_is_rejected_by_name() {
    use crate::features::{FeatureDefinition, FeatureOperation};

    let trim = serde_json::json!({
        "definition": "trim_surface",
        "faces": {"kind": "unresolved"},
        "tool": {"kind": "unresolved", "value": "test:trim-tool"},
        "keep": {"cells": {"removed": [1, 4], "total": 5}}
    });
    assert!(
        serde_json::from_value::<FeatureDefinition>(trim.clone()).is_ok(),
        "{trim}"
    );
    let mut inside = trim;
    inside["keep"]["cells"]["zz_bogus"] = serde_json::json!(1);
    let error = serde_json::from_value::<FeatureOperation>(inside)
        .unwrap_err()
        .to_string();
    assert!(error.contains("zz_bogus"), "{error}");
}

#[test]
fn the_extrude_start_and_termination_unit_forms_reject_an_unknown_key() {
    use crate::features::{AngularTermination, ExtrudeStart, LinearTermination};

    for wire in [
        serde_json::json!({"kind": "profile_plane"}),
        serde_json::json!({"kind": "unresolved"}),
    ] {
        assert!(
            serde_json::from_value::<ExtrudeStart>(wire.clone()).is_ok(),
            "{wire}"
        );
        let mut bogus = wire;
        bogus["zz_bogus"] = serde_json::json!(1);
        let error = serde_json::from_value::<ExtrudeStart>(bogus)
            .unwrap_err()
            .to_string();
        assert!(error.contains("zz_bogus"), "{error}");
    }

    for kind in [
        "unresolved",
        "through_all",
        "through_next",
        "to_first",
        "to_last",
    ] {
        let bogus = serde_json::json!({"kind": kind, "zz_bogus": 1});
        let error = serde_json::from_value::<LinearTermination>(bogus.clone())
            .unwrap_err()
            .to_string();
        assert!(error.contains("zz_bogus"), "linear {kind}: {error}");
        let error = serde_json::from_value::<AngularTermination>(bogus)
            .unwrap_err()
            .to_string();
        assert!(error.contains("zz_bogus"), "angular {kind}: {error}");
    }
}
