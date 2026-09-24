// SPDX-License-Identifier: Apache-2.0
use crate::{
    features::edge_treatments::{ChamferSpec, RadiusSpec, VariableRadii, VariableRadius},
    scalar::Length,
};
use serde_json::json;

fn point(parameter: f64, radius: f64) -> VariableRadius {
    VariableRadius {
        parameter,
        radius: Length::new(radius).unwrap(),
    }
}

#[test]
fn variable_radius_law_admits_ordered_nonnegative_samples() {
    for points in [
        vec![],
        vec![point(0.0, 1.0)],
        vec![point(0.5, 1.0), point(0.25, 2.0)],
        vec![point(0.5, 1.0), point(0.5, 2.0)],
        vec![point(-0.5, 1.0), point(1.0, 2.0)],
        vec![point(0.0, 1.0), point(1.5, 2.0)],
        vec![point(0.0, -1.0), point(1.0, 2.0)],
        vec![point(0.0, 0.0), point(1.0, 0.0)],
        vec![point(f64::NAN, 1.0), point(1.0, 2.0)],
    ] {
        assert!(VariableRadii::new(points).is_err());
    }
    let points = vec![point(0.2, 0.0), point(0.4, 2.0), point(0.8, 0.0)];
    let admitted = VariableRadii::new(points.clone()).unwrap();
    assert_eq!(
        admitted
            .as_slice()
            .iter()
            .map(VariableRadius::to_raw)
            .collect::<Vec<_>>(),
        points
    );
    let wire = json!({"kind":"variable","points":[
        {"parameter":0.2,"radius":0.0},
        {"parameter":0.4,"radius":2.0},
        {"parameter":0.8,"radius":0.0}
    ]});
    let radius = RadiusSpec::Variable { points: admitted };
    assert_eq!(serde_json::to_value(&radius).unwrap(), wire);
    assert_eq!(serde_json::from_value::<RadiusSpec>(wire).unwrap(), radius);
}

#[test]
fn edge_treatment_wire_rejects_invalid_dimensions_and_laws() {
    for wire in [
        json!({"kind":"constant","radius":0.0}),
        json!({"kind":"chordal","chord_length":-1.0}),
        json!({"kind":"asymmetric","offset_one":1.0,"offset_two":0.0}),
        json!({"kind":"variable","points":[]}),
        json!({"kind":"variable","points":[{"parameter":0.0,"radius":1.0}]}),
        json!({"kind":"variable","points":[{"parameter":0.5,"radius":1.0},{"parameter":0.25,"radius":2.0}]}),
        json!({"kind":"variable","points":[{"parameter":0.0,"radius":0.0},{"parameter":1.0,"radius":0.0}]}),
        json!({"kind":"variable","points":[{"parameter":0.0,"radius":-1.0},{"parameter":1.0,"radius":2.0}]}),
    ] {
        assert!(serde_json::from_value::<RadiusSpec>(wire).is_err());
    }
    for wire in [
        json!({"kind":"distance","distance":0.0}),
        json!({"kind":"two_distances","first":1.0,"second":-1.0}),
        json!({"kind":"distance_angle","distance":1.0,"angle":0.0}),
        json!({"kind":"distance_angle","distance":1.0,"angle":std::f64::consts::PI}),
        json!({"kind":"distance_angle","distance":-1.0,"angle":1.0}),
    ] {
        assert!(serde_json::from_value::<ChamferSpec>(wire).is_err());
    }
}

#[test]
fn edge_treatment_wire_preserves_resolved_and_unresolved_forms() {
    for wire in [
        json!({"kind":"unresolved"}),
        json!({"kind":"unresolved","form":"constant"}),
        json!({"kind":"unresolved","form":"chordal"}),
        json!({"kind":"unresolved","form":"asymmetric"}),
        json!({"kind":"unresolved","form":"variable"}),
        json!({"kind":"constant","radius":1.0}),
        json!({"kind":"chordal","chord_length":1.0}),
        json!({"kind":"asymmetric","offset_one":1.0,"offset_two":2.0}),
    ] {
        let radius: RadiusSpec = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(radius).unwrap(), wire);
    }
    for wire in [
        json!({"kind":"unresolved"}),
        json!({"kind":"unresolved","form":"distance"}),
        json!({"kind":"unresolved","form":"two_distances"}),
        json!({"kind":"unresolved","form":"distance_angle"}),
        json!({"kind":"distance","distance":1.0}),
        json!({"kind":"two_distances","first":1.0,"second":2.0}),
        json!({"kind":"distance_angle","distance":1.0,"angle":1.0}),
    ] {
        let spec: ChamferSpec = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(spec).unwrap(), wire);
    }
}

#[test]
fn variable_radii_hold_admitted_samples_and_take_admitted_parts() {
    use crate::scalar::{Fraction, NonNegativeLength};
    let admitted = VariableRadii::new(vec![point(0.0, 0.0), point(1.0, 2.0)]).unwrap();
    assert_eq!(
        admitted.as_slice()[1].parameter,
        Fraction::new(1.0).unwrap()
    );
    assert_eq!(
        admitted.as_slice()[1].radius,
        NonNegativeLength::new(2.0).unwrap()
    );
    let sample = |parameter: f64, radius: f64| VariableRadius {
        parameter: Fraction::new(parameter).unwrap(),
        radius: NonNegativeLength::new(radius).unwrap(),
    };
    assert_eq!(
        VariableRadii::from_parts(vec![sample(0.0, 0.0), sample(1.0, 2.0)]).unwrap(),
        admitted
    );
    for points in [
        vec![sample(0.0, 1.0)],
        vec![sample(0.0, 0.0), sample(1.0, 0.0)],
        vec![sample(1.0, 1.0), sample(0.5, 2.0)],
    ] {
        assert!(VariableRadii::from_parts(points).is_err());
    }
}

/// A radius map keeps the sample count and the parameters, so only the one
/// positive radius is tested again; a refusal of the map discards it.
#[test]
fn a_radius_map_keeps_the_parameters_and_tests_one_positive_radius() {
    use crate::scalar::{NonNegativeLength, PositiveReal};
    let law = VariableRadii::new(vec![point(0.0, 0.0), point(0.5, 1.0e-320), point(1.0, 2.0)])
        .expect("an admitted law");
    let scale = |value: f64| PositiveReal::new(value).expect("a positive scale");
    let scaled =
        |value: f64| move |radius: NonNegativeLength| radius.scaled(scale(value)).ok_or("overflow");

    let doubled = law
        .try_map_radii(scaled(2.0))
        .expect("finite radii")
        .expect("one positive radius");
    assert_eq!(
        doubled
            .as_slice()
            .iter()
            .map(|sample| (sample.parameter.get(), sample.radius.get()))
            .collect::<Vec<_>>(),
        vec![(0.0, 0.0), (0.5, 2.0e-320), (1.0, 4.0)]
    );

    let tiny =
        VariableRadii::new(vec![point(0.0, 0.0), point(1.0, 1.0e-320)]).expect("an admitted law");
    assert!(tiny
        .try_map_radii(scaled(1.0e-10))
        .expect("finite radii")
        .is_err());
    assert_eq!(
        law.try_map_radii(scaled(f64::MAX)).map(|law| law.is_ok()),
        Err("overflow")
    );
}
