// SPDX-License-Identifier: Apache-2.0
use crate::math::{Point3, Vector3};
use crate::{
    features::{
        patterns::{
            CompositePattern, LinearPatternDirection, PatternKind, PatternScaleCenter,
            PatternStage, PatternTransform,
        },
        FaceSelection, FeatureDirection3, FinitePoint3,
    },
    scalar::{Angle, Length, PositiveAngle, PositiveLength, PositiveReal},
    units::UnitVector3,
};
use serde_json::json;

fn direction(x: f64, y: f64, z: f64) -> FeatureDirection3 {
    FeatureDirection3::new(Vector3::new(x, y, z)).unwrap()
}

fn point(x: f64, y: f64, z: f64) -> FinitePoint3 {
    FinitePoint3::new(Point3::new(x, y, z)).unwrap()
}

fn factor(value: f64) -> PositiveReal {
    PositiveReal::new(value).unwrap()
}

fn linear<C>(count: u32) -> PatternTransform<C> {
    PatternTransform::Linear {
        direction: None,
        spacing: PositiveLength::new(1.0).unwrap(),
        count,
        second: None,
    }
}

fn stage(
    transform: PatternTransform<crate::features::patterns::NoNestedComposite>,
) -> PatternStage {
    PatternStage {
        pattern: Box::new(PatternKind::new(transform).unwrap()),
    }
}

#[test]
fn pattern_admission_rejects_invalid_numeric_operands() {
    let origin = point(0.0, 0.0, 0.0);
    let axis = direction(0.0, 0.0, 1.0);
    for transform in [
        linear::<CompositePattern>(0),
        PatternTransform::Linear {
            direction: None,
            spacing: PositiveLength::new(1.0).unwrap(),
            count: 1,
            second: Some(LinearPatternDirection {
                direction: axis,
                spacing: PositiveLength::new(1.0).unwrap(),
                count: 0,
            }),
        },
        PatternTransform::Circular {
            axis_origin: origin,
            axis_dir: axis,
            angle: PositiveAngle::FULL_TURN,
            count: 0,
        },
        PatternTransform::MirrorReference {
            plane: FaceSelection::Native(String::new()),
        },
        PatternTransform::Scale {
            center: PatternScaleCenter::FirstSeedCentroid,
            final_factor: factor(2.0),
            count: 1,
        },
    ] {
        assert!(PatternKind::new(transform).is_err());
    }
    // A degenerate direction, a non-finite point, a non-positive scale
    // factor, a non-positive spacing and a non-positive angle have no
    // spelling as a `PatternTransform`: the field carries
    // `FeatureDirection3`, `FinitePoint3`, `PositiveReal`, `PositiveLength`
    // or `PositiveAngle`, which is where those values are refused.
    assert!(FeatureDirection3::new(Vector3::new(0.0, 0.0, 0.0)).is_none());
    assert!(FeatureDirection3::new(Vector3::new(f64::MAX, 0.0, 0.0)).is_none());
    assert!(FinitePoint3::new(Point3::new(f64::NAN, 0.0, 0.0)).is_none());
    assert!(FinitePoint3::new(Point3::new(0.0, f64::INFINITY, 0.0)).is_none());
    assert!(PositiveReal::new(f64::INFINITY).is_none());
    assert!(PositiveReal::new(0.0).is_none());
    assert!(PositiveLength::new(0.0).is_none());
    assert!(PositiveLength::new(-1.0).is_none());
    assert!(PositiveAngle::new(0.0).is_none());
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
        assert!(
            PatternKind::<CompositePattern>::new(PatternTransform::LinearOffsets {
                direction: None,
                offsets
            })
            .is_err()
        );
        assert!(
            PatternKind::<CompositePattern>::new(PatternTransform::CircularAngles {
                axis_origin: point(0.0, 0.0, 0.0),
                axis_dir: UnitVector3::new(Vector3::new(0.0, 0.0, 1.0)).unwrap(),
                angles,
            })
            .is_err()
        );
    }
    assert!(
        PatternKind::<CompositePattern>::new(PatternTransform::LinearOffsets {
            direction: None,
            offsets: vec![Length::ZERO],
        })
        .is_ok()
    );
    assert!(
        PatternKind::<CompositePattern>::new(PatternTransform::CircularAngles {
            axis_origin: point(0.0, 0.0, 0.0),
            axis_dir: UnitVector3::new(Vector3::new(0.0, 0.0, 1.0)).unwrap(),
            angles: vec![Angle::ZERO],
        })
        .is_ok()
    );
}

#[test]
fn pattern_admission_preserves_singletons_and_unresolved_references() {
    for transform in [
        linear::<CompositePattern>(1),
        PatternTransform::Mirror {
            plane_origin: point(0.0, 0.0, 0.0),
            plane_normal: direction(f64::EPSILON / 2.0, 0.0, 0.0),
        },
        PatternTransform::MirrorReference {
            plane: FaceSelection::Native(" ".into()),
        },
        PatternTransform::MirrorReference {
            plane: FaceSelection::Unresolved,
        },
        PatternTransform::Scale {
            center: PatternScaleCenter::Native(String::new()),
            final_factor: factor(2.0),
            count: 2,
        },
    ] {
        assert!(PatternKind::new(transform).is_ok());
    }
}

#[test]
fn composite_pattern_admission_enforces_stage_structure_and_counts() {
    let scale = || PatternTransform::Scale {
        center: PatternScaleCenter::FirstSeedCentroid,
        final_factor: factor(2.0),
        count: 2,
    };
    // A stage's combination rule is its position and its transform, so a
    // mismatched rule has no spelling; what remains to refuse is the count
    // composition and a nested composite.
    for stages in [
        vec![],
        vec![stage(linear(3)), stage(scale())],
        vec![
            stage(linear(u32::MAX)),
            stage(linear(u32::MAX)),
            stage(linear(u32::MAX)),
        ],
    ] {
        assert!(CompositePattern::new(stages).is_err());
    }
    // A stage that applies another sequence of stages does not compile: the
    // stage transform's composite arm carries an uninhabited type.
    assert!(CompositePattern::new(vec![stage(linear(4)), stage(scale())]).is_ok());
    assert!(CompositePattern::new(vec![
        PatternStage {
            pattern: Box::new(crate::features::patterns::StagePatternKind::UNRESOLVED)
        },
        stage(linear(2)),
    ])
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
        json!({"kind":"composite","stages":[{"pattern":{"kind":"unresolved"}}]}),
    ] {
        let pattern: PatternKind = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(&pattern).unwrap(), wire);
    }
}

#[test]
fn composite_pattern_counts_use_primary_instance_counts() {
    let first = PatternTransform::Linear {
        direction: None,
        spacing: PositiveLength::new(1.0).unwrap(),
        count: 2,
        second: Some(LinearPatternDirection {
            direction: direction(0.0, 1.0, 0.0),
            spacing: PositiveLength::new(1.0).unwrap(),
            count: 3,
        }),
    };
    let scale = PatternTransform::Scale {
        center: PatternScaleCenter::FirstSeedCentroid,
        final_factor: factor(2.0),
        count: 3,
    };
    assert!(CompositePattern::new(vec![stage(first), stage(scale),]).is_err());
}

#[test]
fn pattern_wire_error_identifies_the_rejected_field() {
    let error = serde_json::from_value::<PatternKind>(json!({
        "kind":"linear","spacing":1.0,"count":0
    }))
    .unwrap_err();
    assert!(error.to_string().contains("count"));
}

#[test]
fn pattern_wire_refuses_a_degenerate_direction_point_or_scale_factor() {
    for wire in [
        json!({"kind":"linear","direction":{"x":0.0,"y":0.0,"z":0.0},"spacing":1.0,"count":1}),
        json!({"kind":"linear_offsets","direction":{"x":0.0,"y":0.0,"z":0.0},"offsets":[0.0]}),
        json!({"kind":"linear","spacing":1.0,"count":1,"second":{"direction":{"x":0.0,"y":0.0,"z":0.0},"spacing":1.0,"count":1}}),
        json!({"kind":"circular","axis_origin":{"x":0.0,"y":0.0,"z":0.0},"axis_dir":{"x":0.0,"y":0.0,"z":0.0},"angle":1.0,"count":1}),
        json!({"kind":"circular_angles","axis_origin":{"x":0.0,"y":0.0,"z":0.0},"axis_dir":{"x":0.0,"y":0.0,"z":0.0},"angles":[0.0]}),
        json!({"kind":"mirror","plane_origin":{"x":0.0,"y":0.0,"z":0.0},"plane_normal":{"x":0.0,"y":0.0,"z":0.0}}),
        json!({"kind":"scale","center":{"kind":"first_seed_centroid"},"final_factor":-1.0,"count":2}),
    ] {
        assert!(serde_json::from_value::<PatternKind>(wire.clone()).is_err());
        assert!(serde_json::from_value::<PatternTransform>(wire).is_err());
    }
    // JSON states no infinity, so a non-finite coordinate is refused on the
    // constructor route, which calls the same `FinitePoint3::new` the derived
    // reader calls.
    assert!(FinitePoint3::new(Point3::new(0.0, f64::NAN, 0.0)).is_none());
}

#[test]
fn admitted_pattern_wire_round_trips_every_carried_field() {
    for wire in [
        json!({"kind":"linear","direction":{"x":0.0,"y":0.0,"z":1.0},"spacing":1.0,"count":2,"second":{"direction":{"x":0.0,"y":1.0,"z":0.0},"spacing":2.0,"count":3}}),
        json!({"kind":"linear_offsets","direction":{"x":1.0,"y":0.0,"z":0.0},"offsets":[0.0,1.0]}),
        json!({"kind":"circular","axis_origin":{"x":1.0,"y":2.0,"z":3.0},"axis_dir":{"x":0.0,"y":0.0,"z":1.0},"angle":1.0,"count":2}),
        json!({"kind":"circular_angles","axis_origin":{"x":1.0,"y":2.0,"z":3.0},"axis_dir":{"x":0.0,"y":0.0,"z":1.0},"angles":[0.0,1.0]}),
        json!({"kind":"mirror","plane_origin":{"x":1.0,"y":2.0,"z":3.0},"plane_normal":{"x":0.0,"y":0.0,"z":1.0}}),
        json!({"kind":"scale","center":{"kind":"point","value":{"x":1.0,"y":2.0,"z":3.0}},"final_factor":2.0,"count":2}),
    ] {
        let pattern: PatternKind = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(&pattern).unwrap(), wire);
        let transform: PatternTransform = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(&transform).unwrap(), wire);
    }
}

#[test]
fn circular_angle_patterns_admit_only_a_unit_axis_on_the_wire() {
    let pattern = PatternKind::<CompositePattern>::new(PatternTransform::CircularAngles {
        axis_origin: point(0.0, 0.0, 0.0),
        axis_dir: UnitVector3::Z_AXIS,
        angles: vec![Angle::ZERO, Angle::QUARTER_TURN],
    })
    .unwrap();
    let wire = serde_json::to_value(&pattern).unwrap();
    for refused in [
        json!({"x": 0.0, "y": 0.0, "z": 2.0}),
        json!({"x": 0.0, "y": 0.0, "z": 0.0}),
        json!({"x": 0.0, "y": 0.0, "z": 1.0 + 2.0e-9}),
    ] {
        let mut refused_wire = wire.clone();
        refused_wire["axis_dir"] = refused;
        let error = serde_json::from_value::<PatternKind>(refused_wire)
            .unwrap_err()
            .to_string();
        assert!(error.contains("direction must have unit length"), "{error}");
    }
    let mut near_unit = wire.clone();
    near_unit["axis_dir"] = json!({"x": 0.0, "y": 0.0, "z": 1.0 + 5.0e-10});
    let decoded = serde_json::from_value::<PatternKind>(near_unit.clone()).unwrap();
    assert_eq!(serde_json::to_value(decoded).unwrap(), near_unit);
}
