// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use crate::examples::unit_cube;
use crate::math::Vector3;
use crate::validate::validate_neutral;

#[test]
fn feature_operation_geometry_is_validated() {
    use crate::features::{
        EdgeSelection, Feature, FeatureDefinition, FeatureId, FilletGroup, HoleKind, Length,
        LinearTermination, PatternKind, RadiusSpec, VariableRadius,
    };

    let definitions = vec![
        FeatureDefinition::Form { cages: Vec::new() },
        FeatureDefinition::Form {
            cages: vec![
                crate::ids::SubdId::mint("synthetic:test:subd#missing").expect("valid identity")
            ],
        },
        FeatureDefinition::Fillet {
            groups: vec![FilletGroup {
                edges: EdgeSelection::Unresolved,
                radius: RadiusSpec::Variable {
                    points: vec![
                        VariableRadius {
                            parameter: 0.5,
                            radius: Length::new(2.0).unwrap(),
                        },
                        VariableRadius {
                            parameter: 0.25,
                            radius: Length::new(-1.0).unwrap(),
                        },
                    ],
                },
                tangency_weight: None,
            }],
        },
        FeatureDefinition::Hole {
            profile: None,
            profile_filter: None,
            face: None,
            direction: None,
            construction: crate::features::HoleConstruction::Form {
                kind: HoleKind::Simple,
                specification: None,
            },
            exit_kind: Some(HoleKind::Countersink {
                diameter: crate::features::PositiveLength::new(5.0).unwrap(),
                angle: crate::features::InteriorAngle::new(0.5).unwrap(),
            }),
            diameter: Some(crate::features::PositiveLength::new(5.0).unwrap()),
            extent: Some(LinearTermination::ThroughAll),
            bottom: None,
            taper_angle: None,
            placements: None,
            allow_multi_profile_faces: None,
        },
        FeatureDefinition::CompositeCurve {
            segments: Vec::new(),
            closed: false,
        },
        FeatureDefinition::HelixNativeAxis {
            axis_native_ref: String::new(),
            axial_rise: Length::ZERO,
            pitch: Length::ZERO,
            revolutions: crate::features::PositiveReal::new(1.0).unwrap(),
            start_angle: crate::features::Angle::ZERO,
            clockwise: false,
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
                    distance: crate::features::NonZeroLength::new(1.0).unwrap(),
                    join: crate::features::BinderOffsetJoin::Arcs,
                    fill: false,
                    open_result: false,
                    intersection: false,
                }),
                context: None,
            },
        },
        FeatureDefinition::Pattern {
            seeds: Vec::new(),
            pattern: PatternKind::Linear {
                direction: Some(Vector3::new(0.0, 0.0, 0.0)),
                spacing: Length::new(-1.0).unwrap(),
                count: 0,
                second: None,
            },
        },
        FeatureDefinition::Pattern {
            seeds: Vec::new(),
            pattern: PatternKind::CurveDriven {
                path: None,
                spacing: Length::ZERO,
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
                            spacing: Length::new(1.0).unwrap(),
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
    ];
    let expected = [
        "references missing Form control cage `synthetic:test:subd#missing`",
        "fillet radius is invalid",
        "hole geometry is invalid",
        "composite curve is empty",
        "binder construction is invalid",
        "pattern geometry is invalid",
        "pattern geometry is invalid",
        "pattern geometry is invalid",
    ];
    let mut ir = unit_cube();
    for (ordinal, definition) in definitions.into_iter().enumerate() {
        ir.model.features.push(Feature {
            id: FeatureId::mint(format!("synthetic:test:feature#invalid-{ordinal}"))
                .expect("identity grammar"),
            ordinal: ordinal as u64,
            name: None,
            suppressed: Some(false),
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
