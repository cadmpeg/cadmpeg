// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use crate::examples::unit_cube;
use crate::math::{Point3, Vector3};
use crate::validate::validate_neutral;

#[test]
fn feature_operation_geometry_is_validated() {
    use crate::features::{
        BooleanOp, EdgeSelection, FaceSelection, Feature, FeatureDefinition, FeatureId,
        FilletGroup, HoleKind, Length, PatternKind, ProfileRef, RadiusSpec, RibConstruction,
        RibDraft, RibSide, ScaleCenter, ScaleFactors, Termination, ThickenSide, VariableRadius,
    };

    let definitions = vec![
        FeatureDefinition::Form { cages: Vec::new() },
        FeatureDefinition::Form {
            cages: vec![crate::ids::SubdId("synthetic:test:subd#missing".into())],
        },
        FeatureDefinition::Fillet {
            groups: vec![FilletGroup {
                edges: EdgeSelection::Unresolved,
                radius: RadiusSpec::Variable {
                    points: vec![
                        VariableRadius {
                            parameter: 0.5,
                            radius: Length(2.0),
                        },
                        VariableRadius {
                            parameter: 0.25,
                            radius: Length(-1.0),
                        },
                    ],
                },
                tangency_weight: None,
            }],
        },
        FeatureDefinition::Rib {
            construction: RibConstruction {
                profile: Some(ProfileRef::Native("profile".into())),
                direction: Some(Vector3::new(0.0, 0.0, 1.0)),
                thickness: Some(Length(1.0)),
                side: Some(RibSide::OneSided),
                draft: RibDraft::Angle(crate::features::Angle(std::f64::consts::FRAC_PI_2)),
            },
            op: BooleanOp::Join,
        },
        FeatureDefinition::Draft {
            faces: FaceSelection::Unresolved,
            neutral_plane: FaceSelection::Unresolved,
            parting_tool: None,
            pull_direction: Some(Vector3::new(0.0, 0.0, 1.0)),
            pull_plane: None,
            angle: Some(crate::features::Angle(std::f64::consts::FRAC_PI_2)),
            outward: Some(false),
        },
        FeatureDefinition::Hole {
            profile: None,
            profile_filter: None,
            face: Some(FaceSelection::Unresolved),
            position: None,
            direction: None,
            kind: HoleKind::Simple,
            exit_kind: None,
            diameter: Some(Length(0.0)),
            extent: Some(Termination::ThroughAll),
            bottom: None,
            taper_angle: None,
            specification: None,
            placements: Vec::new(),
            allow_multi_profile_faces: None,
        },
        FeatureDefinition::Hole {
            profile: None,
            profile_filter: None,
            face: None,
            position: Some(Point3::new(0.0, 0.0, 0.0)),
            direction: Some(Vector3::new(0.0, 0.0, 1.0)),
            kind: HoleKind::Simple,
            exit_kind: Some(HoleKind::Countersink {
                diameter: Length(5.0),
                angle: crate::features::Angle(0.5),
            }),
            diameter: Some(Length(5.0)),
            extent: Some(Termination::ThroughAll),
            bottom: None,
            taper_angle: None,
            specification: None,
            placements: Vec::new(),
            allow_multi_profile_faces: None,
        },
        FeatureDefinition::Thicken {
            faces: FaceSelection::Unresolved,
            thickness: Some(Length(0.0)),
            side: Some(ThickenSide::Forward),
        },
        FeatureDefinition::OffsetSurface {
            faces: FaceSelection::Unresolved,
            distance: Some(Length(f64::NAN)),
        },
        FeatureDefinition::KnitSurface {
            faces: FaceSelection::Unresolved,
            merge_entities: Some(true),
            create_solid: Some(false),
            gap_tolerance: Some(Length(-1.0)),
        },
        FeatureDefinition::ExtendSurface {
            faces: FaceSelection::Unresolved,
            distance: Some(Length(0.0)),
            method: crate::features::SurfaceExtension::Natural,
        },
        FeatureDefinition::RuledSurface {
            edges: EdgeSelection::Unresolved,
            support_faces: FaceSelection::Unresolved,
            mode: crate::features::RuledSurfaceMode::Direction {
                direction: Vector3::new(0.0, 0.0, 0.0),
                distance: Length(0.0),
            },
            angle: None,
            alternate_face: None,
            corner: None,
        },
        FeatureDefinition::Scale {
            bodies: crate::features::BodySelection::Unresolved,
            center: Some(ScaleCenter::Point(Point3::new(0.0, f64::NAN, 0.0))),
            factors: ScaleFactors {
                uniform: None,
                x: Some(1.0),
                y: Some(0.0),
                z: Some(1.0),
            },
        },
        FeatureDefinition::DatumCoordinateSystem {
            origin: Point3::new(0.0, 0.0, 0.0),
            x_axis: Vector3::new(1.0, 0.0, 0.0),
            y_axis: Vector3::new(1.0, 0.0, 0.0),
            z_axis: Vector3::new(0.0, 0.0, 1.0),
        },
        FeatureDefinition::EquationCurve {
            parameter: String::new(),
            x_expression: "t".into(),
            y_expression: "0".into(),
            z_expression: "0".into(),
            start: 1.0,
            end: 0.0,
        },
        FeatureDefinition::ProjectedCurve {
            source: crate::features::PathRef::Native("source".into()),
            target_faces: FaceSelection::Unresolved,
            direction: crate::features::CurveProjectionDirection::Vector(Vector3::new(
                0.0, 0.0, 0.0,
            )),
            bidirectional: Some(false),
        },
        FeatureDefinition::CompositeCurve {
            segments: Vec::new(),
            closed: false,
        },
        FeatureDefinition::Helix {
            axis_origin: Point3::new(0.0, 0.0, 0.0),
            axis_direction: Vector3::new(0.0, 0.0, 0.0),
            radius: Length(-1.0),
            pitch: Length(f64::NAN),
            revolutions: 0.0,
            start_angle: crate::features::Angle(0.0),
            clockwise: false,
            radial_growth: None,
            cone_angle: None,
            segment_turns: None,
            construction_style: None,
        },
        FeatureDefinition::HelixNativeAxis {
            axis_native_ref: String::new(),
            axial_rise: Length(f64::NAN),
            pitch: Length(f64::NAN),
            revolutions: 0.0,
            start_angle: crate::features::Angle(f64::NAN),
            clockwise: false,
        },
        FeatureDefinition::Sphere {
            center: Point3::new(0.0, f64::NAN, 0.0),
            radius: Length(0.0),
            op: BooleanOp::NewBody,
        },
        FeatureDefinition::Torus {
            center: Point3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 0.0),
            major_radius: Length(10.0),
            minor_radius: Length(-1.0),
            op: BooleanOp::NewBody,
        },
        FeatureDefinition::HelicalSweep {
            construction: crate::features::HelicalSweepConstruction {
                profile: ProfileRef::Native("profile".into()),
                axis_origin: Point3::new(0.0, 0.0, 0.0),
                axis_direction: Vector3::new(0.0, 0.0, 0.0),
                law: crate::features::HelicalSweepLaw::HeightTurnsGrowth,
                pitch: Length(0.0),
                height: Length(0.0),
                turns: 0.0,
                radial_growth: Length(0.0),
                cone_angle: crate::features::Angle(0.0),
                left_handed: false,
                reversed: false,
                tolerance: Some(0.0),
                allow_multi_profile_faces: None,
            },
            op: crate::features::BooleanOp::Join,
        },
        FeatureDefinition::Binder {
            sources: vec![crate::features::BinderSource {
                target: crate::features::BinderTarget::Native {
                    reference: String::new(),
                },
                subelements: vec![String::new()],
            }],
            construction: crate::features::BinderConstruction::SubShape {
                lifecycle: crate::features::BinderLifecycle::Synchronized,
                placement: crate::features::BinderPlacement::Relative,
                copy_on_change: crate::features::BinderCopyOnChange::Disabled,
                claim_children: false,
                fuse: false,
                make_face: true,
                partial_load: false,
                refine: true,
                offset: Some(crate::features::BinderOffset {
                    distance: Length(0.0),
                    join: crate::features::BinderOffsetJoin::Arcs,
                    fill: false,
                    open_result: false,
                    intersection: false,
                }),
                context: None,
            },
        },
        FeatureDefinition::Wrap {
            profile: ProfileRef::Native("profile".into()),
            face: FaceSelection::Unresolved,
            mode: crate::features::WrapMode::Emboss,
            depth: None,
        },
        FeatureDefinition::MoveBody {
            bodies: crate::features::BodySelection::Unresolved,
            translation: Vector3::new(f64::NAN, 0.0, 0.0),
            rotation: Some(crate::features::AxisAngle {
                origin: Point3::new(0.0, 0.0, 0.0),
                direction: Vector3::new(0.0, 0.0, 0.0),
                angle: crate::features::Angle(0.5),
            }),
            copies: 0,
        },
        FeatureDefinition::Pattern {
            seeds: Vec::new(),
            pattern: PatternKind::Linear {
                direction: Some(Vector3::new(0.0, 0.0, 0.0)),
                spacing: Length(-1.0),
                count: 0,
                second: None,
            },
        },
        FeatureDefinition::Pattern {
            seeds: Vec::new(),
            pattern: PatternKind::CurveDriven {
                path: None,
                spacing: Length(0.0),
                count: 0,
            },
        },
        FeatureDefinition::Pattern {
            seeds: Vec::new(),
            pattern: PatternKind::Composite {
                stages: vec![
                    crate::features::PatternStage {
                        pattern: Box::new(PatternKind::Linear {
                            direction: Some(Vector3::new(1.0, 0.0, 0.0)),
                            spacing: Length(1.0),
                            count: 3,
                            second: None,
                        }),
                        combination: crate::features::PatternStageCombination::Initialize,
                    },
                    crate::features::PatternStage {
                        pattern: Box::new(PatternKind::Scale {
                            center: crate::features::PatternScaleCenter::FirstSeedCentroid,
                            final_factor: 2.0,
                            count: 2,
                        }),
                        combination: crate::features::PatternStageCombination::AlignedSlices,
                    },
                ],
            },
        },
        FeatureDefinition::Sweep {
            section: crate::features::SweepSection::Unresolved(None),
            sections: Vec::new(),
            path: None,
            mode: crate::features::SweepMode::Unresolved,
            orientation: None,
            transition: None,
            transformation: None,
            path_tangent: false,
            linearize: false,
            twist: None,
            path_extent: None,
            guide_rail: Some(crate::features::SweepGuideRail {
                path: crate::features::PathRef::Native("native:guide-rail#0".into()),
                extent: crate::features::SweepPathExtent {
                    along_fraction: -1.0,
                    against_fraction: 1.0,
                },
            }),
            taper: None,
            scale: None,
            allow_multi_profile_faces: None,
        },
        FeatureDefinition::DatumOffsetPlane {
            reference: None,
            distance: Length(f64::NAN),
        },
    ];
    let expected = [
        "references missing Form control cage `synthetic:test:subd#missing`",
        "fillet radius is invalid",
        "rib geometry is invalid",
        "draft geometry is invalid",
        "hole geometry is invalid",
        "thicken thickness is invalid",
        "surface offset is invalid",
        "knit tolerance is invalid",
        "surface extension is invalid",
        "ruled surface is invalid",
        "scale transform is invalid",
        "coordinate-system frame is invalid",
        "equation curve is invalid",
        "projection direction is invalid",
        "composite curve is empty",
        "helix geometry is invalid",
        "sphere primitive is invalid",
        "torus primitive is invalid",
        "helical sweep is invalid",
        "binder construction is invalid",
        "wrap depth is invalid",
        "body motion is invalid",
        "pattern geometry is invalid",
        "pattern geometry is invalid",
        "pattern geometry is invalid",
        "sweep magnitude is invalid",
        "datum-plane offset is invalid",
    ];
    let mut ir = unit_cube();
    for (ordinal, definition) in definitions.into_iter().enumerate() {
        ir.model.features.push(Feature {
            id: FeatureId(format!("synthetic:test:feature#invalid-{ordinal}")),
            ordinal: ordinal as u64,
            name: None,
            suppressed: Some(false),
            parent: None,
            dependencies: Vec::new(),
            source_properties: std::collections::BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition,
            native_ref: None,
        });
    }
    let findings = validate_neutral(&ir, Vec::new()).findings;
    assert!(!findings
        .iter()
        .any(|finding| { finding.entity.as_deref() == Some("synthetic:test:feature#invalid-0") }));
    for message in expected {
        assert!(findings.iter().any(|finding| finding.message == message));
    }
}
