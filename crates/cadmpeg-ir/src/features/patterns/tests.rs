// SPDX-License-Identifier: Apache-2.0
use crate::math::{Point3, Vector3};
use crate::{
    features::{
        patterns::{
            CompositePattern, LinearPatternDirection, PatternDistance, PatternKind,
            PatternLengthField, PatternScaleCenter, PatternStage, PatternTransform,
        },
        Dimension, FaceSelection, FeatureDirection3, FinitePoint3,
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
        distance: PositiveLength::new(1.0).unwrap().into(),
        count: Dimension::Literal(count),
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
            distance: PositiveLength::new(1.0).unwrap().into(),
            count: Dimension::Literal(1),
            second: Some(LinearPatternDirection {
                direction: axis,
                distance: PositiveLength::new(1.0).unwrap().into(),
                count: Dimension::Literal(0),
            }),
        },
        PatternTransform::Circular {
            axis_origin: origin,
            axis_dir: axis,
            angle: PositiveAngle::FULL_TURN.into(),
            count: Dimension::Literal(0),
        },
        PatternTransform::MirrorReference {
            plane: FaceSelection::Native(String::new()),
        },
        PatternTransform::Scale {
            center: PatternScaleCenter::FirstSeedCentroid,
            final_factor: factor(2.0).into(),
            count: Dimension::Literal(1),
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
            .map(|value| Dimension::Literal(Length::new(*value).unwrap()))
            .collect();
        let angles = locations
            .iter()
            .map(|value| Dimension::Literal(Angle::new(*value).unwrap()))
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
            offsets: vec![Dimension::Literal(Length::ZERO)],
        })
        .is_ok()
    );
    assert!(
        PatternKind::<CompositePattern>::new(PatternTransform::CircularAngles {
            axis_origin: point(0.0, 0.0, 0.0),
            axis_dir: UnitVector3::new(Vector3::new(0.0, 0.0, 1.0)).unwrap(),
            angles: vec![Dimension::Literal(Angle::ZERO)],
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
            final_factor: factor(2.0).into(),
            count: Dimension::Literal(2),
        },
    ] {
        assert!(PatternKind::new(transform).is_ok());
    }
}

#[test]
fn composite_pattern_admission_enforces_stage_structure_and_counts() {
    let scale = || PatternTransform::Scale {
        center: PatternScaleCenter::FirstSeedCentroid,
        final_factor: factor(2.0).into(),
        count: Dimension::Literal(2),
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
fn composite_length_mapping_carries_stage_counts_and_scale_factor() {
    let stages = CompositePattern::new(vec![
        stage(linear(4)),
        stage(PatternTransform::Scale {
            center: PatternScaleCenter::FirstSeedCentroid,
            final_factor: factor(2.0).into(),
            count: Dimension::Literal(2),
        }),
    ])
    .unwrap();
    let original = PatternKind::new(PatternTransform::Composite { stages }).unwrap();
    let mapped = original
        .try_map_lengths(&mut |field| {
            if let PatternLengthField::Length(spacing) = field {
                *spacing = Length::new(spacing.get() * 2.0).unwrap();
            }
            Ok::<(), ()>(())
        })
        .unwrap();
    let PatternTransform::Composite { stages } = mapped.definition() else {
        panic!("the composite arm is carried");
    };
    assert!(matches!(
        stages[0].pattern.definition(),
        PatternTransform::Linear { distance, count: Dimension::Literal(4), .. } if distance.dimension().literal().is_some_and(|value| value.get() == 2.0)
    ));
    assert!(matches!(
        stages[1].pattern.definition(),
        PatternTransform::Scale { final_factor, count: Dimension::Literal(2), .. } if final_factor.literal().is_some_and(|value| value.get() == 2.0)
    ));
    let PatternTransform::Composite { stages } = original.definition() else {
        panic!("the source composite is carried");
    };
    assert!(matches!(
        stages[0].pattern.definition(),
        PatternTransform::Linear { distance, count: Dimension::Literal(4), .. } if distance.dimension().literal().is_some_and(|value| value.get() == 1.0)
    ));
}

#[test]
fn pattern_wire_rejects_invalid_counts_locations_and_composition() {
    for wire in [
        json!({"kind":"linear","distance":{"kind":"spacing","distance":{"kind":"literal","value":1.0}},"count":{"kind":"literal","value":0}}),
        json!({"kind":"linear","distance":{"kind":"spacing","distance":{"kind":"literal","value":-1.0}},"count":{"kind":"literal","value":1}}),
        json!({"kind":"linear","distance":{"kind":"spacing","distance":{"kind":"literal","value":1.0}},"count":{"kind":"literal","value":1},"second":{"direction":{"x":1.0,"y":0.0,"z":0.0},"distance":{"kind":"spacing","distance":{"kind":"literal","value":1.0}},"count":{"kind":"literal","value":0}}}),
        json!({"kind":"linear_offsets","offsets":[]}),
        json!({"kind":"linear_offsets","offsets":[{"kind":"literal","value":0.0},{"kind":"literal","value":0.0}]}),
        json!({"kind":"curve_driven","spacing":{"kind":"literal","value":1.0},"count":{"kind":"literal","value":0}}),
        json!({"kind":"scale","center":{"kind":"first_seed_centroid"},"final_factor":{"kind":"literal","value":0.0},"count":{"kind":"literal","value":2}}),
        json!({"kind":"scale","center":{"kind":"first_seed_centroid"},"final_factor":{"kind":"literal","value":2.0},"count":{"kind":"literal","value":1}}),
        json!({"kind":"composite","stages":[]}),
        json!({"kind":"composite","stages":[{"pattern":{"kind":"linear","distance":{"kind":"spacing","distance":{"kind":"literal","value":1.0}},"count":{"kind":"literal","value":1}},"combination":"cartesian_product"}]}),
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
        json!({"kind":"linear","distance":{"kind":"spacing","distance":{"kind":"literal","value":1.0}},"count":{"kind":"literal","value":1}}),
        json!({"kind":"linear_offsets","offsets":[{"kind":"literal","value":0.0}]}),
        json!({"kind":"circular","axis_origin":{"x":0.0,"y":0.0,"z":0.0},"axis_dir":{"x":0.0,"y":0.0,"z":1.0},"angle":{"kind":"span","angle":{"kind":"literal","value":1.0},"distribution":"inclusive"},"count":{"kind":"literal","value":1}}),
        json!({"kind":"circular_angles","axis_origin":{"x":0.0,"y":0.0,"z":0.0},"axis_dir":{"x":0.0,"y":0.0,"z":1.0},"angles":[{"kind":"literal","value":0.0}]}),
        json!({"kind":"curve_driven","spacing":{"kind":"literal","value":1.0},"count":{"kind":"literal","value":1}}),
        json!({"kind":"mirror","plane_origin":{"x":0.0,"y":0.0,"z":0.0},"plane_normal":{"x":0.0,"y":0.0,"z":1.0}}),
        json!({"kind":"mirror_reference","plane":{"kind":"native","value":"plane"}}),
        json!({"kind":"scale","center":{"kind":"native","value":""},"final_factor":{"kind":"literal","value":2.0},"count":{"kind":"literal","value":2}}),
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
        distance: PositiveLength::new(1.0).unwrap().into(),
        count: Dimension::Literal(2),
        second: Some(LinearPatternDirection {
            direction: direction(0.0, 1.0, 0.0),
            distance: PositiveLength::new(1.0).unwrap().into(),
            count: Dimension::Literal(3),
        }),
    };
    let scale = PatternTransform::Scale {
        center: PatternScaleCenter::FirstSeedCentroid,
        final_factor: factor(2.0).into(),
        count: Dimension::Literal(3),
    };
    assert!(CompositePattern::new(vec![stage(first), stage(scale),]).is_err());
}

#[test]
fn pattern_wire_error_identifies_the_rejected_field() {
    let error = serde_json::from_value::<PatternKind>(json!({
        "kind":"linear","distance":{"kind":"spacing","distance":{"kind":"literal","value":1.0}},"count":{"kind":"literal","value":0}
    }))
    .unwrap_err();
    assert!(error.to_string().contains("count"));
}

#[test]
fn pattern_wire_refuses_a_degenerate_direction_point_or_scale_factor() {
    for wire in [
        json!({"kind":"linear","direction":{"x":0.0,"y":0.0,"z":0.0},"distance":{"kind":"spacing","distance":{"kind":"literal","value":1.0}},"count":{"kind":"literal","value":1}}),
        json!({"kind":"linear_offsets","direction":{"x":0.0,"y":0.0,"z":0.0},"offsets":[{"kind":"literal","value":0.0}]}),
        json!({"kind":"linear","distance":{"kind":"spacing","distance":{"kind":"literal","value":1.0}},"count":{"kind":"literal","value":1},"second":{"direction":{"x":0.0,"y":0.0,"z":0.0},"distance":{"kind":"spacing","distance":{"kind":"literal","value":1.0}},"count":{"kind":"literal","value":1}}}),
        json!({"kind":"circular","axis_origin":{"x":0.0,"y":0.0,"z":0.0},"axis_dir":{"x":0.0,"y":0.0,"z":0.0},"angle":{"kind":"span","angle":{"kind":"literal","value":1.0},"distribution":"inclusive"},"count":{"kind":"literal","value":1}}),
        json!({"kind":"circular_angles","axis_origin":{"x":0.0,"y":0.0,"z":0.0},"axis_dir":{"x":0.0,"y":0.0,"z":0.0},"angles":[{"kind":"literal","value":0.0}]}),
        json!({"kind":"mirror","plane_origin":{"x":0.0,"y":0.0,"z":0.0},"plane_normal":{"x":0.0,"y":0.0,"z":0.0}}),
        json!({"kind":"scale","center":{"kind":"first_seed_centroid"},"final_factor":{"kind":"literal","value":-1.0},"count":{"kind":"literal","value":2}}),
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
        json!({"kind":"linear","direction":{"x":0.0,"y":0.0,"z":1.0},"distance":{"kind":"spacing","distance":{"kind":"literal","value":1.0}},"count":{"kind":"literal","value":2},"second":{"direction":{"x":0.0,"y":1.0,"z":0.0},"distance":{"kind":"spacing","distance":{"kind":"literal","value":2.0}},"count":{"kind":"literal","value":3}}}),
        json!({"kind":"linear_offsets","direction":{"x":1.0,"y":0.0,"z":0.0},"offsets":[{"kind":"literal","value":0.0},{"kind":"literal","value":1.0}]}),
        json!({"kind":"circular","axis_origin":{"x":1.0,"y":2.0,"z":3.0},"axis_dir":{"x":0.0,"y":0.0,"z":1.0},"angle":{"kind":"span","angle":{"kind":"literal","value":1.0},"distribution":"inclusive"},"count":{"kind":"literal","value":2}}),
        json!({"kind":"circular_angles","axis_origin":{"x":1.0,"y":2.0,"z":3.0},"axis_dir":{"x":0.0,"y":0.0,"z":1.0},"angles":[{"kind":"literal","value":0.0},{"kind":"literal","value":1.0}]}),
        json!({"kind":"mirror","plane_origin":{"x":1.0,"y":2.0,"z":3.0},"plane_normal":{"x":0.0,"y":0.0,"z":1.0}}),
        json!({"kind":"scale","center":{"kind":"point","value":{"x":1.0,"y":2.0,"z":3.0}},"final_factor":{"kind":"literal","value":2.0},"count":{"kind":"literal","value":2}}),
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
        angles: vec![
            Dimension::Literal(Angle::ZERO),
            Dimension::Literal(Angle::QUARTER_TURN),
        ],
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

#[test]
fn bound_pattern_dimensions_retain_identity_during_length_mapping() {
    let spacing = crate::features::ParameterId::mint("test:parameter:spacing#0").unwrap();
    let count = crate::features::ParameterId::mint("test:parameter:count#0").unwrap();
    let pattern = PatternKind::<CompositePattern>::new(PatternTransform::Linear {
        direction: None,
        distance: Dimension::<Length>::Parameter(spacing.clone()).into(),
        count: Dimension::Parameter(count.clone()),
        second: None,
    })
    .unwrap();
    let mut visits = 0;
    let mapped = pattern
        .try_map_lengths(&mut |_| {
            visits += 1;
            Ok::<(), ()>(())
        })
        .unwrap();
    assert_eq!(
        visits, 0,
        "the canonical parameters are scaled in their own arena"
    );
    assert!(matches!(mapped.definition(), PatternTransform::Linear {
        distance: PatternDistance::Spacing { distance: Dimension::Parameter(actual_spacing) },
        count: Dimension::Parameter(actual_count), ..
    } if actual_spacing == &spacing && actual_count == &count));
    let wire = serde_json::to_value(&mapped).unwrap();
    assert_eq!(
        wire["distance"]["distance"],
        json!({"kind":"parameter","value":spacing})
    );
    assert_eq!(wire["count"], json!({"kind":"parameter","value":count}));
    assert_eq!(serde_json::from_value::<PatternKind>(wire).unwrap(), mapped);
}

#[test]
fn an_unknown_stage_count_does_not_claim_a_known_composite_count() {
    let id = crate::features::ParameterId::mint("test:parameter:count#0").unwrap();
    let unknown = PatternTransform::Linear {
        direction: None,
        distance: PositiveLength::new(1.0).unwrap().into(),
        count: Dimension::Parameter(id),
        second: None,
    };
    let scale = PatternTransform::Scale {
        center: PatternScaleCenter::FirstSeedCentroid,
        final_factor: factor(2.0).into(),
        count: Dimension::Literal(2),
    };
    assert!(CompositePattern::new(vec![stage(linear(3)), stage(unknown), stage(scale)]).is_ok());
}

#[test]
fn authored_spans_keep_their_sign_and_resolve_adjacent_spacing() {
    use super::{CircularPatternDistribution, PatternAngle, PatternDistance};
    let ir = crate::CadIr::empty();
    let count = Dimension::Literal(4);
    let span = PatternDistance::Span {
        distance: Length::new(-6.0).unwrap().into(),
    };
    assert_eq!(span.resolved_spacing(&ir, &count).unwrap().get(), -2.0);
    let pattern = PatternKind::<CompositePattern>::new(PatternTransform::Linear {
        direction: Some(direction(1.0, 0.0, 0.0)),
        distance: span,
        count,
        second: None,
    })
    .unwrap();
    let wire = serde_json::to_value(pattern).unwrap();
    assert_eq!(
        wire["distance"],
        json!({"kind":"span","distance":{"kind":"literal","value":-6.0}})
    );
    let zero = PatternDistance::Span {
        distance: Length::new(0.0).unwrap().into(),
    };
    assert!(zero.resolved_spacing(&ir, &Dimension::Literal(1)).is_none());
    let periodic = PatternAngle::Span {
        angle: Dimension::Literal(PositiveAngle::FULL_TURN),
        distribution: CircularPatternDistribution::Periodic,
    };
    let inclusive = PatternAngle::Span {
        angle: Dimension::Literal(PositiveAngle::FULL_TURN),
        distribution: CircularPatternDistribution::Inclusive,
    };
    assert_eq!(
        periodic
            .resolved_spacing(&ir, &Dimension::Literal(4))
            .unwrap()
            .get(),
        std::f64::consts::FRAC_PI_2
    );
    assert_eq!(
        inclusive
            .resolved_spacing(&ir, &Dimension::Literal(5))
            .unwrap()
            .get(),
        std::f64::consts::FRAC_PI_2
    );
    let signed = PatternAngle::Span {
        angle: Dimension::Literal(Angle::new(-std::f64::consts::PI).unwrap()),
        distribution: CircularPatternDistribution::Inclusive,
    };
    assert_eq!(
        signed
            .resolved_spacing(&ir, &Dimension::Literal(3))
            .unwrap()
            .get(),
        -std::f64::consts::FRAC_PI_2
    );
}
