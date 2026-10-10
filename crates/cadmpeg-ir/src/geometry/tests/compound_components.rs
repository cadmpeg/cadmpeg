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
