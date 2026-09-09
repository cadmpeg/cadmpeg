// SPDX-License-Identifier: Apache-2.0

use crate::{
    features::{ChamferSpec, RadiusSpec, VariableRadii, VariableRadius},
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
    assert_eq!(admitted.as_slice(), points);
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
