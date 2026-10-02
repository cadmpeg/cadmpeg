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
        assert!(VariableRadii::new(points, &cadmpeg_test_support::service_decode_context()).expect("radius construction admission").is_err());
    }
    let points = vec![point(0.2, 0.0), point(0.4, 2.0), point(0.8, 0.0)];
    let admitted = VariableRadii::new(points.clone(), &cadmpeg_test_support::service_decode_context()).expect("radius construction admission").unwrap();
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
    let admitted = VariableRadii::new(vec![point(0.0, 0.0), point(1.0, 2.0)], &cadmpeg_test_support::service_decode_context()).expect("radius construction admission").unwrap();
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
        VariableRadii::from_parts(vec![sample(0.0, 0.0), sample(1.0, 2.0)], &cadmpeg_test_support::service_decode_context()).expect("radius construction admission").unwrap(),
        admitted
    );
    for points in [
        vec![sample(0.0, 1.0)],
        vec![sample(0.0, 0.0), sample(1.0, 0.0)],
        vec![sample(1.0, 1.0), sample(0.5, 2.0)],
    ] {
        assert!(VariableRadii::from_parts(points, &cadmpeg_test_support::service_decode_context()).expect("radius construction admission").is_err());
    }
}

/// A radius map keeps the sample count and the parameters, so only the one
/// positive radius is tested again; a refusal of the map discards it.
#[test]
fn a_radius_map_keeps_the_parameters_and_tests_one_positive_radius() {
    use crate::scalar::{NonNegativeLength, PositiveReal};
    let law = VariableRadii::new(vec![point(0.0, 0.0), point(0.5, 1.0e-320), point(1.0, 2.0)], &cadmpeg_test_support::service_decode_context()).expect("radius construction admission")
        .expect("an admitted law");
    const SMALL_RADIUS_SCALE: f64 = 1.0e-10;
    let scale = |value: f64| PositiveReal::new(value).expect("a positive scale");
    let scaled =
        |value: f64| move |radius: NonNegativeLength| radius.scaled(scale(value)).ok_or("overflow");

    let doubled = law.clone()
        .try_map_radii(&cadmpeg_test_support::service_decode_context(), scaled(2.0)).expect("radius map admission")
        .expect("finite radii with one positive radius");
    assert_eq!(
        doubled
            .as_slice()
            .iter()
            .map(|sample| (sample.parameter.get(), sample.radius.get()))
            .collect::<Vec<_>>(),
        vec![(0.0, 0.0), (0.5, 2.0e-320), (1.0, 4.0)]
    );

    let tiny =
        VariableRadii::new(vec![point(0.0, 0.0), point(1.0, 1.0e-320)], &cadmpeg_test_support::service_decode_context()).expect("radius construction admission").expect("an admitted law");
    assert!(matches!(
        tiny.try_map_radii(&cadmpeg_test_support::service_decode_context(), scaled(SMALL_RADIUS_SCALE)).expect("radius map admission"),
        Err(super::VariableRadiiMapError::Admission(_))
    ));
    assert_eq!(
        law.try_map_radii(&cadmpeg_test_support::service_decode_context(), scaled(f64::MAX)).expect("radius map admission"),
        Err(super::VariableRadiiMapError::Radius("overflow"))
    );
}

#[test]
fn owned_variable_radius_scaling_refuses_each_sample_work_without_allocation() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let law = || VariableRadii::new(vec![point(0.0, 1.0), point(1.0, 2.0)], &cadmpeg_test_support::service_decode_context()).expect("radius construction admission").expect("law");
    for cap in [0, 1] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        assert!(matches!(law().try_map_radii(&ctx, Ok::<_, ()>),
            Err(cadmpeg_core::CodecError::ResourceLimit(resource)) if resource.operation == "IR variable radii scaling work"));
    }
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 2;
    policy.limits.max_collection_items = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let multiplier = crate::scalar::PositiveReal::new(2.0).expect("scale");
    let actual = law()
        .try_map_radii(&ctx, |radius| radius.scaled(multiplier).ok_or("overflow"))
        .expect("admitted");
    assert_eq!(
        actual,
        law().try_map_radii(&cadmpeg_test_support::service_decode_context(), |radius| radius.scaled(multiplier).ok_or("overflow")).expect("radius map admission")
    );
    let expected = law().try_map_radii(&cadmpeg_test_support::service_decode_context(), |_| Ok::<_, ()>(crate::scalar::NonNegativeLength::ZERO)).expect("radius map admission");
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("root");
    assert_eq!(
        law()
            .try_map_radii(&ctx, |_| Ok::<_, ()>(
                crate::scalar::NonNegativeLength::ZERO
            ))
            .expect("admitted refusal"),
        expected
    );
}

#[test]
fn radius_mapping_uses_the_original_context_before_each_callback() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let law = || VariableRadii::new(vec![point(0.0, 1.0), point(0.5, 2.0), point(1.0, 3.0)], &cadmpeg_test_support::service_decode_context()).expect("radius construction admission").expect("law");
    for cap in 0..3 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let mut calls = 0;
        let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = law().try_map_radii(&ctx, |radius| { calls += 1; Ok::<_, ()>(radius) }) else { panic!("caller work must be admitted before conversion"); };
        assert_eq!(calls, cap);
        assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
        assert_eq!(limit.used, cap);
        assert_eq!(limit.operation, "IR variable radii scaling work");
        assert!(matches!(ctx.finish_session(), Err(cadmpeg_core::CodecError::ResourceLimit(sticky)) if sticky == limit));
    }
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 3;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let original = law();
    let address = original.as_slice().as_ptr();
    let mapped = original.try_map_radii(&ctx, Ok::<_, ()>).expect("exact caller work").expect("valid");
    assert_eq!(mapped.as_slice().as_ptr(), address);
    ctx.finish_session().expect("no sample copy or second scan");
}

mod construction;
