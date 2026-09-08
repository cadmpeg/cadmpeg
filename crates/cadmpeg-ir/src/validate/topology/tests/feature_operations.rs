// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use crate::examples::unit_cube;
use crate::validate::validate_neutral;

#[test]
fn feature_operation_geometry_is_validated() {
    use crate::features::{
        Feature, FeatureDefinition, FeatureId, HoleKind, Length, LinearTermination,
    };

    let definitions = vec![
        FeatureDefinition::Form { cages: Vec::new() },
        FeatureDefinition::Form {
            cages: vec![
                crate::ids::SubdId::mint("synthetic:test:subd#missing").expect("valid identity")
            ],
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
    ];
    let expected = [
        "references missing Form control cage `synthetic:test:subd#missing`",
        "hole geometry is invalid",
        "composite curve is empty",
        "binder construction is invalid",
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
