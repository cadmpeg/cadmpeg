// SPDX-License-Identifier: Apache-2.0

use crate::math::{Point3, Vector3};
use crate::{
    features::{
        FaceSelection, LinearPatternDirection, PatternKind, PatternScaleCenter, PatternStage,
        PatternStageCombination, PatternTransform,
    },
    scalar::{Angle, Length},
};
use serde_json::json;

fn linear(count: u32) -> PatternTransform {
    PatternTransform::Linear {
        direction: None,
        spacing: Length::new(1.0).unwrap(),
        count,
        second: None,
    }
}

fn stage(transform: PatternTransform, combination: PatternStageCombination) -> PatternStage {
    PatternStage {
        pattern: Box::new(PatternKind::new(transform).unwrap()),
        combination,
    }
}

#[test]
fn pattern_admission_rejects_invalid_numeric_operands() {
    let origin = Point3::new(0.0, 0.0, 0.0);
    let axis = Vector3::new(0.0, 0.0, 1.0);
    for transform in [
        linear(0),
        PatternTransform::Linear {
            direction: None,
            spacing: Length::ZERO,
            count: 1,
            second: None,
        },
        PatternTransform::Linear {
            direction: Some(Vector3::new(0.0, 0.0, 0.0)),
            spacing: Length::new(1.0).unwrap(),
            count: 1,
            second: None,
        },
        PatternTransform::Linear {
            direction: None,
            spacing: Length::new(1.0).unwrap(),
            count: 1,
            second: Some(LinearPatternDirection {
                direction: axis,
                spacing: Length::new(1.0).unwrap(),
                count: 0,
            }),
        },
        PatternTransform::Circular {
            axis_origin: origin,
            axis_dir: axis,
            angle: Angle::ZERO,
            count: 1,
        },
        PatternTransform::Circular {
            axis_origin: origin,
            axis_dir: axis,
            angle: Angle::FULL_TURN,
            count: 0,
        },
        PatternTransform::CurveDriven {
            path: None,
            spacing: Length::new(-1.0).unwrap(),
            count: 1,
        },
        PatternTransform::Mirror {
            plane_origin: Point3::new(f64::NAN, 0.0, 0.0),
            plane_normal: axis,
        },
        PatternTransform::Mirror {
            plane_origin: origin,
            plane_normal: Vector3::new(f64::MAX, 0.0, 0.0),
        },
        PatternTransform::MirrorReference {
            plane: FaceSelection::Native(String::new()),
        },
        PatternTransform::Scale {
            center: PatternScaleCenter::FirstSeedCentroid,
            final_factor: 2.0,
            count: 1,
        },
        PatternTransform::Scale {
            center: PatternScaleCenter::FirstSeedCentroid,
            final_factor: f64::INFINITY,
            count: 2,
        },
        PatternTransform::Scale {
            center: PatternScaleCenter::Point(Point3::new(0.0, f64::INFINITY, 0.0)),
            final_factor: 2.0,
            count: 2,
        },
    ] {
        assert!(PatternKind::new(transform).is_err());
    }
}

#[test]
fn pattern_locations_start_at_zero_and_increase() {
    for locations in [vec![], vec![1.0], vec![0.0, 0.0], vec![0.0, -1.0]] {
        let offsets = locations
            .iter()
            .map(|value| Length::new(*value).unwrap())
            .collect();
        let angles = locations
            .iter()
            .map(|value| Angle::new(*value).unwrap())
            .collect();
        assert!(PatternKind::new(PatternTransform::LinearOffsets {
            direction: None,
            offsets
        })
        .is_err());
        assert!(PatternKind::new(PatternTransform::CircularAngles {
            axis_origin: Point3::new(0.0, 0.0, 0.0),
            axis_dir: Vector3::new(0.0, 0.0, 1.0),
            angles,
        })
        .is_err());
    }
    assert!(PatternKind::new(PatternTransform::LinearOffsets {
        direction: None,
        offsets: vec![Length::ZERO],
    })
    .is_ok());
    assert!(PatternKind::new(PatternTransform::CircularAngles {
        axis_origin: Point3::new(0.0, 0.0, 0.0),
        axis_dir: Vector3::new(0.0, 0.0, 1.0),
        angles: vec![Angle::ZERO],
    })
    .is_ok());
}

#[test]
fn pattern_admission_preserves_singletons_and_unresolved_references() {
    for transform in [
        linear(1),
        PatternTransform::Mirror {
            plane_origin: Point3::new(0.0, 0.0, 0.0),
            plane_normal: Vector3::new(f64::EPSILON / 2.0, 0.0, 0.0),
        },
        PatternTransform::MirrorReference {
            plane: FaceSelection::Native(" ".into()),
        },
        PatternTransform::MirrorReference {
            plane: FaceSelection::Unresolved,
        },
        PatternTransform::Scale {
            center: PatternScaleCenter::Native(String::new()),
            final_factor: 2.0,
            count: 2,
        },
    ] {
        assert!(PatternKind::new(transform).is_ok());
    }
}

#[test]
fn composite_pattern_admission_enforces_stage_structure_and_counts() {
    use PatternStageCombination::{AlignedSlices, CartesianProduct, Initialize};
    let scale = || PatternTransform::Scale {
        center: PatternScaleCenter::FirstSeedCentroid,
        final_factor: 2.0,
        count: 2,
    };
    for stages in [
        vec![],
        vec![stage(linear(1), CartesianProduct)],
        vec![
            stage(linear(1), Initialize),
            stage(linear(2), AlignedSlices),
        ],
        vec![
            stage(linear(2), Initialize),
            stage(scale(), CartesianProduct),
        ],
        vec![stage(linear(3), Initialize), stage(scale(), AlignedSlices)],
        vec![
            stage(linear(u32::MAX), Initialize),
            stage(linear(u32::MAX), CartesianProduct),
            stage(linear(u32::MAX), CartesianProduct),
        ],
        vec![stage(
            PatternTransform::Composite {
                stages: vec![stage(linear(1), Initialize)],
            },
            Initialize,
        )],
    ] {
        assert!(PatternKind::new(PatternTransform::Composite { stages }).is_err());
    }
    assert!(PatternKind::new(PatternTransform::Composite {
        stages: vec![stage(linear(4), Initialize), stage(scale(), AlignedSlices)],
    })
    .is_ok());
    assert!(PatternKind::new(PatternTransform::Composite {
        stages: vec![
            PatternStage {
                pattern: Box::new(PatternKind::UNRESOLVED),
                combination: Initialize
            },
            stage(linear(2), CartesianProduct),
        ],
    })
    .is_ok());
}

#[test]
fn pattern_wire_rejects_invalid_counts_locations_and_composition() {
    for wire in [
        json!({"kind":"linear","spacing":1.0,"count":0}),
        json!({"kind":"linear","spacing":-1.0,"count":1}),
        json!({"kind":"linear","spacing":1.0,"count":1,"second":{"direction":{"x":1.0,"y":0.0,"z":0.0},"spacing":1.0,"count":0}}),
        json!({"kind":"linear_offsets","offsets":[]}),
        json!({"kind":"linear_offsets","offsets":[0.0,0.0]}),
        json!({"kind":"curve_driven","spacing":1.0,"count":0}),
        json!({"kind":"scale","center":{"kind":"first_seed_centroid"},"final_factor":0.0,"count":2}),
        json!({"kind":"scale","center":{"kind":"first_seed_centroid"},"final_factor":2.0,"count":1}),
        json!({"kind":"composite","stages":[]}),
        json!({"kind":"composite","stages":[{"pattern":{"kind":"linear","spacing":1.0,"count":1},"combination":"cartesian_product"}]}),
    ] {
        assert!(serde_json::from_value::<PatternKind>(wire).is_err());
    }
}

#[test]
fn admitted_pattern_wire_preserves_tags_and_optional_fields() {
    for wire in [
        json!({"kind":"unresolved"}),
        json!({"kind":"unresolved","form":"linear"}),
        json!({"kind":"unresolved","form":"circular"}),
        json!({"kind":"unresolved","form":"curve_driven"}),
        json!({"kind":"unresolved","form":"mirror"}),
        json!({"kind":"unresolved","form":"scale"}),
        json!({"kind":"unresolved","form":"composite"}),
        json!({"kind":"linear","spacing":1.0,"count":1}),
        json!({"kind":"linear_offsets","offsets":[0.0]}),
        json!({"kind":"circular","axis_origin":{"x":0.0,"y":0.0,"z":0.0},"axis_dir":{"x":0.0,"y":0.0,"z":1.0},"angle":1.0,"count":1}),
        json!({"kind":"circular_angles","axis_origin":{"x":0.0,"y":0.0,"z":0.0},"axis_dir":{"x":0.0,"y":0.0,"z":1.0},"angles":[0.0]}),
        json!({"kind":"curve_driven","spacing":1.0,"count":1}),
        json!({"kind":"mirror","plane_origin":{"x":0.0,"y":0.0,"z":0.0},"plane_normal":{"x":0.0,"y":0.0,"z":1.0}}),
        json!({"kind":"mirror_reference","plane":{"kind":"native","value":"plane"}}),
        json!({"kind":"scale","center":{"kind":"native","value":""},"final_factor":2.0,"count":2}),
        json!({"kind":"composite","stages":[{"pattern":{"kind":"unresolved"},"combination":"initialize"}]}),
    ] {
        let pattern: PatternKind = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(&pattern).unwrap(), wire);
    }
}

#[test]
fn composite_pattern_counts_use_primary_instance_counts() {
    let first = PatternTransform::Linear {
        direction: None,
        spacing: Length::new(1.0).unwrap(),
        count: 2,
        second: Some(LinearPatternDirection {
            direction: Vector3::new(0.0, 1.0, 0.0),
            spacing: Length::new(1.0).unwrap(),
            count: 3,
        }),
    };
    let scale = PatternTransform::Scale {
        center: PatternScaleCenter::FirstSeedCentroid,
        final_factor: 2.0,
        count: 3,
    };
    assert!(PatternKind::new(PatternTransform::Composite {
        stages: vec![
            stage(first, PatternStageCombination::Initialize),
            stage(scale, PatternStageCombination::AlignedSlices),
        ],
    })
    .is_err());
}

#[test]
fn pattern_wire_error_identifies_the_rejected_field() {
    let error = serde_json::from_value::<PatternKind>(json!({
        "kind":"linear","spacing":1.0,"count":0
    }))
    .unwrap_err();
    assert!(error.to_string().contains("count"));
}
