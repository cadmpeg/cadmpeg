// SPDX-License-Identifier: Apache-2.0

use crate::{
    features::edge_treatments::{ChamferSpec, RadiusSpec, VariableRadii, VariableRadius},
    scalar::Length,
};
use serde_json::json;

fn point(parameter: f64, radius: f64) -> VariableRadius {
    VariableRadius {
        parameter,
        radius: Length::new(radius).unwrap().into(),
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
        {"parameter":0.2,"radius":{"kind":"literal","value":0.0}},
        {"parameter":0.4,"radius":{"kind":"literal","value":2.0}},
        {"parameter":0.8,"radius":{"kind":"literal","value":0.0}}
    ]});
    let radius = RadiusSpec::Variable { points: admitted };
    assert_eq!(serde_json::to_value(&radius).unwrap(), wire);
    assert_eq!(serde_json::from_value::<RadiusSpec>(wire).unwrap(), radius);
}

#[test]
fn edge_treatment_wire_rejects_invalid_dimensions_and_laws() {
    for wire in [
        json!({"kind":"constant","radius":{"kind":"literal","value":0.0}}),
        json!({"kind":"chordal","chord_length":{"kind":"literal","value":-1.0}}),
        json!({"kind":"asymmetric","offset_one":{"kind":"literal","value":1.0},"offset_two":{"kind":"literal","value":0.0}}),
        json!({"kind":"variable","points":[]}),
        json!({"kind":"variable","points":[{"parameter":0.0,"radius":{"kind":"literal","value":1.0}}]}),
        json!({"kind":"variable","points":[{"parameter":0.5,"radius":{"kind":"literal","value":1.0}},{"parameter":0.25,"radius":{"kind":"literal","value":2.0}}]}),
        json!({"kind":"variable","points":[{"parameter":0.0,"radius":{"kind":"literal","value":0.0}},{"parameter":1.0,"radius":{"kind":"literal","value":0.0}}]}),
        json!({"kind":"variable","points":[{"parameter":0.0,"radius":{"kind":"literal","value":-1.0}},{"parameter":1.0,"radius":{"kind":"literal","value":2.0}}]}),
    ] {
        assert!(serde_json::from_value::<RadiusSpec>(wire).is_err());
    }
    for wire in [
        json!({"kind":"distance","distance":{"kind":"literal","value":0.0}}),
        json!({"kind":"two_distances","first":{"kind":"literal","value":1.0},"second":{"kind":"literal","value":-1.0}}),
        json!({"kind":"distance_angle","distance":{"kind":"literal","value":1.0},"angle":{"kind":"literal","value":0.0}}),
        json!({"kind":"distance_angle","distance":{"kind":"literal","value":1.0},"angle":{"kind":"literal","value":std::f64::consts::PI}}),
        json!({"kind":"distance_angle","distance":{"kind":"literal","value":-1.0},"angle":{"kind":"literal","value":1.0}}),
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
        json!({"kind":"constant","radius":{"kind":"literal","value":1.0}}),
        json!({"kind":"chordal","chord_length":{"kind":"literal","value":1.0}}),
        json!({"kind":"asymmetric","offset_one":{"kind":"literal","value":1.0},"offset_two":{"kind":"literal","value":2.0}}),
    ] {
        let radius: RadiusSpec = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(radius).unwrap(), wire);
    }
    for wire in [
        json!({"kind":"unresolved"}),
        json!({"kind":"unresolved","form":"distance"}),
        json!({"kind":"unresolved","form":"two_distances"}),
        json!({"kind":"unresolved","form":"distance_angle"}),
        json!({"kind":"distance","distance":{"kind":"literal","value":1.0}}),
        json!({"kind":"two_distances","first":{"kind":"literal","value":1.0},"second":{"kind":"literal","value":2.0}}),
        json!({"kind":"distance_angle","distance":{"kind":"literal","value":1.0},"angle":{"kind":"literal","value":1.0}}),
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
        super::Dimension::Literal(NonNegativeLength::new(2.0).unwrap())
    );
    let sample = |parameter: f64, radius: f64| VariableRadius {
        parameter: Fraction::new(parameter).unwrap(),
        radius: NonNegativeLength::new(radius).unwrap().into(),
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
        .clone()
        .try_map_radii(scaled(2.0))
        .expect("finite radii with one positive radius");
    assert_eq!(
        doubled
            .as_slice()
            .iter()
            .map(|sample| (
                sample.parameter.get(),
                sample.radius.literal().expect("literal sample").get()
            ))
            .collect::<Vec<_>>(),
        vec![(0.0, 0.0), (0.5, 2.0e-320), (1.0, 4.0)]
    );

    let tiny =
        VariableRadii::new(vec![point(0.0, 0.0), point(1.0, 1.0e-320)]).expect("an admitted law");
    assert!(matches!(
        tiny.try_map_radii(scaled(SMALL_RADIUS_SCALE)),
        Err(super::VariableRadiiMapError::Admission(_))
    ));
    assert_eq!(
        law.try_map_radii(scaled(f64::MAX)),
        Err(super::VariableRadiiMapError::Radius("overflow"))
    );
}

#[test]
fn owned_variable_radius_scaling_matches_the_service_result() {
    let law = || VariableRadii::new(vec![point(0.0, 1.0), point(1.0, 2.0)]).expect("law");
    let multiplier = crate::scalar::PositiveReal::new(2.0).expect("scale");
    let actual = law().try_map_radii(|radius| radius.scaled(multiplier).ok_or("overflow"));
    assert_eq!(
        actual,
        law().try_map_radii(|radius| { radius.scaled(multiplier).ok_or("overflow") })
    );
    let expected = law().try_map_radii(|_| Ok::<_, ()>(crate::scalar::NonNegativeLength::ZERO));
    assert_eq!(
        law().try_map_radii(|_| Ok::<_, ()>(crate::scalar::NonNegativeLength::ZERO)),
        expected
    );
}

#[test]
fn owned_variable_radius_mapping_keeps_its_sample_storage() {
    let original =
        VariableRadii::new(vec![point(0.0, 1.0), point(0.5, 2.0), point(1.0, 3.0)]).expect("law");
    let address = original.as_slice().as_ptr();
    let mapped = original.try_map_radii(Ok::<_, ()>).expect("valid");
    assert_eq!(mapped.as_slice().as_ptr(), address);
}

mod construction;

const SMALL_RADIUS_SCALE: f64 = 1.0e-10;

#[test]
fn bound_variable_radii_keep_their_parameter_and_visit_its_identity() {
    use crate::features::{Dimension, ParameterId};
    use crate::scalar::{Fraction, NonNegativeLength};
    use crate::schema::rewrite::typed::RewriteIdentities;
    let parameter = ParameterId::mint("test:model:parameter#radius").expect("parameter id");
    let law = VariableRadii::from_parts(vec![
        VariableRadius {
            parameter: Fraction::new(0.0).expect("fraction"),
            radius: Dimension::Parameter(parameter.clone()),
        },
        VariableRadius {
            parameter: Fraction::new(1.0).expect("fraction"),
            radius: Dimension::Literal(NonNegativeLength::ZERO),
        },
    ])
    .expect("bound positivity is checked after parameter resolution");
    let mut calls = 0;
    let mapped = law
        .try_map_radii(|radius| {
            calls += 1;
            Ok::<_, ()>(radius)
        })
        .expect("bound radius retained");
    assert_eq!(calls, 1);
    assert_eq!(mapped.as_slice()[0].radius.parameter(), Some(&parameter));
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("context");
    let mut references = Vec::new();
    mapped
        .visit_identity_references(&ctx, &mut |id| {
            references.push(id.to_owned());
            Ok(())
        })
        .expect("visit");
    assert_eq!(references, vec![parameter.as_str()]);
}
