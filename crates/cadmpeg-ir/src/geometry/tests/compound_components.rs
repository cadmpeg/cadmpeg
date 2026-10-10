// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use crate::geometry::{CompoundComponent, ProceduralSurfaceDefinition};

#[test]
fn a_compound_surface_component_is_one_object_carrying_its_own_scalar() {
    let definition = ProceduralSurfaceDefinition::Compound(
        crate::geometry::surface_payloads::CompoundSurfacePayload::try_new(
            vec![
                CompoundComponent {
                    parameter: -0.5,
                    component: "test:model:surface#0".try_into().expect("valid identity"),
                },
                CompoundComponent {
                    parameter: 1.5,
                    component: "test:model:surface#1".try_into().expect("valid identity"),
                },
            ],
            None,
        )
        .unwrap(),
    );
    let wire = serde_json::json!({
        "kind": "compound",
        "components": [
            {"parameter": -0.5, "component": "test:model:surface#0"},
            {"parameter": 1.5, "component": "test:model:surface#1"},
        ],
    });
    assert_eq!(serde_json::to_value(&definition).unwrap(), wire);
    assert_eq!(
        serde_json::from_value::<ProceduralSurfaceDefinition>(wire.clone()).unwrap(),
        definition
    );
}

#[test]
fn a_compound_surface_carries_no_parallel_parameter_array() {
    let struct_of_arrays = serde_json::json!({
        "kind": "compound",
        "parameters": [-0.5, 1.5],
        "components": ["test:model:surface#0", "test:model:surface#1"],
    });
    let error = serde_json::from_value::<ProceduralSurfaceDefinition>(struct_of_arrays)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("expected struct CompoundComponent"),
        "{error}"
    );

    let stray = serde_json::json!({
        "kind": "compound",
        "parameters": [-0.5, 1.5],
        "components": [
            {"parameter": -0.5, "component": "test:model:surface#0"},
            {"parameter": 1.5, "component": "test:model:surface#1"},
        ],
    });
    let error = serde_json::from_value::<ProceduralSurfaceDefinition>(stray)
        .unwrap_err()
        .to_string();
    assert!(error.contains("parameters"), "{error}");
}

#[test]
fn a_compound_curve_component_is_one_object_carrying_its_own_scalar() {
    use crate::geometry::ProceduralCurveDefinition;
    let definition = ProceduralCurveDefinition::Compound(
        crate::geometry::CompoundCurveConstruction::try_new(
            vec![0.0, 0.5, 1.0],
            vec![
                CompoundComponent {
                    parameter: -2.0,
                    component: "test:model:curve#0".try_into().expect("valid identity"),
                },
                CompoundComponent {
                    parameter: 4.0,
                    component: "test:model:curve#1".try_into().expect("valid identity"),
                },
            ],
            None,
        )
        .unwrap(),
    );
    let wire = serde_json::json!({
        "kind": "compound",
        "parameters": [0.0, 0.5, 1.0],
        "components": [
            {"parameter": -2.0, "component": "test:model:curve#0"},
            {"parameter": 4.0, "component": "test:model:curve#1"},
        ],
    });
    assert_eq!(serde_json::to_value(&definition).unwrap(), wire);
    assert_eq!(
        serde_json::from_value::<ProceduralCurveDefinition>(wire.clone()).unwrap(),
        definition
    );

    let mut stray = wire;
    stray["component_parameters"] = serde_json::json!([-2.0, 4.0]);
    let error = serde_json::from_value::<ProceduralCurveDefinition>(stray)
        .unwrap_err()
        .to_string();
    assert!(error.contains("component_parameters"), "{error}");
}

#[test]
fn a_compound_component_rejects_an_unknown_key_by_name() {
    let wire = serde_json::json!({
        "kind": "compound",
        "components": [
            {"parameter": -0.5, "component": "test:model:surface#0", "zz_bogus": 1},
        ],
    });
    let error = serde_json::from_value::<ProceduralSurfaceDefinition>(wire)
        .unwrap_err()
        .to_string();
    assert!(error.contains("zz_bogus"), "{error}");
}

#[test]
fn compound_curve_requires_components_and_finite_parameters() {
    use crate::geometry::{CompoundCurveConstruction, ProceduralCurveDefinition};
    let component = || CompoundComponent {
        parameter: -2.0,
        component: "test:model:curve#0".try_into().unwrap(),
    };
    assert!(CompoundCurveConstruction::try_new(Vec::new(), vec![component()], None).is_ok());
    assert!(CompoundCurveConstruction::try_new(vec![0.0], Vec::new(), None).is_err());
    for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(
            CompoundCurveConstruction::try_new(vec![invalid], vec![component()], None).is_err()
        );
        let mut item = component();
        item.parameter = invalid;
        assert!(CompoundCurveConstruction::try_new(Vec::new(), vec![item], None).is_err());
    }
    let empty = serde_json::json!({
        "kind": "compound", "parameters": [], "components": []
    });
    assert!(serde_json::from_value::<ProceduralCurveDefinition>(empty).is_err());
    let unordered =
        CompoundCurveConstruction::try_new(vec![2.0, -1.0], vec![component()], None).unwrap();
    let wire = serde_json::to_value(&unordered).unwrap();
    assert_eq!(
        serde_json::from_value::<CompoundCurveConstruction>(wire).unwrap(),
        unordered
    );
}

#[test]
fn a_compound_curve_holds_its_admitted_parameters() {
    use crate::geometry::CompoundCurveConstruction;
    use crate::scalar::FiniteReal;

    let construction = CompoundCurveConstruction::try_new(
        vec![2.0, -1.0],
        vec![CompoundComponent {
            parameter: 0.5,
            component: "test:model:curve#0".try_into().unwrap(),
        }],
        None,
    )
    .unwrap();
    assert_eq!(
        construction.parameters(),
        [
            FiniteReal::new(2.0).unwrap(),
            FiniteReal::new(-1.0).unwrap()
        ]
    );
    assert_eq!(
        serde_json::to_value(&construction).unwrap()["parameters"],
        serde_json::json!([2.0, -1.0])
    );
}

#[test]
fn compound_curve_decode_admits_each_parameter_and_component() {
    use crate::geometry::CompoundCurveConstruction;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let component = || CompoundComponent {
        parameter: -2.0,
        component: "test:model:curve#0".try_into().unwrap(),
    };
    for operation in [
        "compound curve parameter admission",
        "compound curve component admission",
    ] {
        let run = |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
            let result = CompoundCurveConstruction::try_new_for_decode(
                &ctx,
                vec![2.0, -1.0],
                vec![component()],
                None,
            )?;
            ctx.finish_session()?;
            result.map_err(CodecError::malformed)
        };
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::WorkUnits,
            operation,
            run,
        );
        let CodecError::ResourceLimit(limit) = error else {
            panic!("compound visit refusal");
        };
        assert_eq!(limit.additional, 1); // One source element at each boundary.
        assert_eq!(
            run(u64::MAX).unwrap(),
            CompoundCurveConstruction::try_new(vec![2.0, -1.0], vec![component()], None,).unwrap()
        );
    }
    // A first invalid parameter stops before both its suffix and the component list.
    // A first invalid component stops before its suffix. Each case visits exactly one element.
    for invalid_parameter in [true, false] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut invalid_component = component();
        invalid_component.parameter = f64::NAN;
        let (parameters, components) = if invalid_parameter {
            (vec![f64::NAN; 128], vec![component(); 128])
        } else {
            (Vec::new(), vec![invalid_component; 128])
        };
        assert_eq!(
            CompoundCurveConstruction::try_new_for_decode(&ctx, parameters, components, None)
                .unwrap(),
            Err("compound curve parameters must be finite")
        );
        ctx.finish_session().unwrap();
    }
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert_eq!(
        CompoundCurveConstruction::try_new_for_decode(&ctx, vec![0.0; 128], Vec::new(), None)
            .unwrap(),
        Err("compound curve components must not be empty")
    );
    ctx.finish_session().unwrap();
}

#[test]
fn compound_curve_decode_rejects_after_a_valid_prefix_without_retaining_storage() {
    use crate::geometry::CompoundCurveConstruction;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let component = |parameter| CompoundComponent {
        parameter,
        component: "test:model:curve#0".try_into().unwrap(),
    };
    for (parameters, components, visits) in [
        (vec![0.0, f64::NAN, 1.0], vec![component(0.0)], 2),
        (
            vec![0.0, 1.0],
            vec![component(0.0), component(f64::INFINITY), component(1.0)],
            4,
        ),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = visits;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert_eq!(
            CompoundCurveConstruction::try_new_for_decode(&ctx, parameters, components, None)
                .unwrap(),
            Err("compound curve parameters must be finite")
        );
        ctx.finish_session().unwrap();
    }
}

#[test]
fn compound_curve_decode_moves_both_input_allocations_without_a_storage_charge() {
    use crate::geometry::CompoundCurveConstruction;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let mut parameters = Vec::with_capacity(128);
    parameters.extend([2.0, -1.0]);
    let mut components = Vec::with_capacity(128);
    components.push(CompoundComponent {
        parameter: 0.5,
        component: "test:model:curve#0".try_into().unwrap(),
    });
    let parameter_pointer = parameters.as_ptr().cast::<()>();
    let component_pointer = components.as_ptr().cast::<()>();
    let parameter_capacity = parameters.capacity();
    let component_capacity = components.capacity();
    let expected = CompoundCurveConstruction::try_new(parameters.clone(), components.clone(), None)
        .unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // Validation: two parameters and one component. Conversion: two parameter
    // passes and one component pass. Both input allocations move.
    policy.limits.max_work_units = (2 + 1) + (2 * 2 + 1);
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let construction =
        CompoundCurveConstruction::try_new_for_decode(&ctx, parameters, components, None)
            .unwrap()
            .unwrap();
    assert_eq!(construction, expected);
    assert_eq!(construction.parameters.as_ptr().cast::<()>(), parameter_pointer);
    assert_eq!(construction.components.as_ptr().cast::<()>(), component_pointer);
    assert_eq!(construction.parameters.capacity(), parameter_capacity);
    assert_eq!(construction.components.capacity(), component_capacity);
    ctx.finish_session().unwrap();
}

#[test]
fn compound_curve_constructors_share_the_empty_check_and_validation_order() {
    use crate::geometry::CompoundCurveConstruction;

    let component = |parameter| CompoundComponent {
        parameter,
        component: "test:model:curve#0".try_into().unwrap(),
    };
    for (parameters, components, expected_visits, message) in [
        (
            vec![f64::NAN],
            Vec::new(),
            Vec::new(),
            "compound curve components must not be empty",
        ),
        (
            vec![0.0, f64::NAN, 1.0],
            vec![component(f64::INFINITY)],
            vec!["compound curve parameter admission"; 2],
            "compound curve parameters must be finite",
        ),
        (
            vec![0.0],
            vec![component(0.0), component(f64::INFINITY), component(1.0)],
            vec![
                "compound curve parameter admission",
                "compound curve component admission",
                "compound curve component admission",
            ],
            "compound curve parameters must be finite",
        ),
    ] {
        assert_eq!(
            CompoundCurveConstruction::try_new(parameters.clone(), components.clone(), None),
            Err(message)
        );
        let mut visits = Vec::new();
        assert_eq!(
            CompoundCurveConstruction::validate(parameters, components, None, |operation, _| {
                visits.push(operation);
                Ok::<_, &'static str>(())
            })
            .unwrap(),
            Err(message)
        );
        assert_eq!(visits, expected_visits);
    }
}

#[test]
fn compound_curve_decode_admits_conversion_passes_only_after_validation() {
    use crate::geometry::CompoundCurveConstruction;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    // Validation visits 2 + 1 elements. Parameter conversion checks and maps
    // two parameters (2 * 2 steps); component conversion visits one component.
    for (cap, additional, operation) in [
        (2 + 1, 2 * 2, "compound curve parameter admission"),
        (2 + 1 + 2 * 2, 1, "compound curve component admission"),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = CompoundCurveConstruction::try_new_for_decode(
            &ctx,
            vec![2.0, -1.0],
            vec![CompoundComponent {
                parameter: 0.5,
                component: "test:model:curve#0".try_into().unwrap(),
            }],
            None,
        ).unwrap_err();
        let CodecError::ResourceLimit(limit) = error else {
            panic!("conversion work must refuse");
        };
        assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
        assert_eq!(limit.operation, operation);
        assert_eq!(limit.used, cap);
        assert_eq!(limit.additional, additional);
        assert!(ctx.finish_session().is_err());
    }
}
