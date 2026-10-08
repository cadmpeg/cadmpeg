// SPDX-License-Identifier: Apache-2.0
use crate::curve::test_support::with_policy;
use crate::curve::{
    parse_relation_expression, relation_unit, CurveExpressionValue, RelationEvaluationContext,
};
use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use std::collections::BTreeMap;

#[test]
fn flat_relation_scans_refuse_caller_work() {
    for source in ["1+1+1", "123456789", "       1", "----1"] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = crate::test_support::allocation_limit_at(
            ResourceDimension::WorkUnits,
            Some("creo relation source scan"),
            |cap| {
                let mut trial = policy;
                trial.limits.max_work_units = cap;
                with_policy(trial, |ctx| {
                    parse_relation_expression::<CurveExpressionValue>(
                        ctx,
                        source,
                        &BTreeMap::new(),
                        RelationEvaluationContext::default(),
                    )
                })
            },
        );
        let error = with_policy(policy, |ctx| {
            parse_relation_expression::<CurveExpressionValue>(
                ctx,
                source,
                &BTreeMap::new(),
                RelationEvaluationContext::default(),
            )
        })
        .expect_err("source scan must refuse");
        assert!(
            matches!(error, CodecError::ResourceLimit(limit) if limit.operation == "creo relation source scan")
        );
    }
}

#[test]
fn relation_unit_recursion_refuses_caller_and_local_depth() {
    for depth in [0, 1024] {
        let source = if depth == 0 {
            "((mm))".to_owned()
        } else {
            format!("{}mm{}", "(".repeat(129), ")".repeat(129))
        };
        let mut policy = DecodePolicy::service();
        policy.limits.max_recursion_depth = depth;
        let error = with_policy(policy, |ctx| {
            relation_unit(ctx, &source).map(|unit| unit.is_some())
        })
        .expect_err("unit nesting must refuse");
        assert!(matches!(error, CodecError::ResourceLimit(_)));
    }
}

#[test]
fn relation_local_nesting_ceiling_refuses() {
    let source = format!("{}1{}", "(".repeat(129), ")".repeat(129));
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 1024;
    let error = with_policy(policy, |ctx| {
        parse_relation_expression::<CurveExpressionValue>(
            ctx,
            &source,
            &BTreeMap::new(),
            RelationEvaluationContext::default(),
        )
    })
    .expect_err("local nesting ceiling");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.operation == "creo relation nesting ceiling")
    );
}

#[test]
fn relation_values_reject_nonfinite_numbers_and_dedicated_residual_dimensions() {
    use crate::curve::{quantity_value, CurveExpressionQuantity, RelationDimension};
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for dimension in [
            RelationDimension::default(),
            RelationDimension::LENGTH,
            RelationDimension::ANGLE,
            RelationDimension::TIME,
        ] {
            assert!(quantity_value(value, dimension).is_none());
        }
        assert!(CurveExpressionQuantity::new(value, [0, 0, 1, 0, 0]).is_none());
    }
    for powers in [[0; 5], [1, 0, 0, 0, 0], [0, 0, 0, 1, 0]] {
        assert!(CurveExpressionQuantity::new(1.0, powers).is_none());
    }
    let value = quantity_value(1.0, RelationDimension::default()).expect("finite number");
    assert!(matches!(value, CurveExpressionValue::Number(_)));
    assert_eq!(value.truth(), Some(true));
    assert!(matches!(
        quantity_value(1.0, RelationDimension::LENGTH),
        Some(CurveExpressionValue::Length(_))
    ));
    assert!(matches!(
        quantity_value(1.0, RelationDimension::ANGLE),
        Some(CurveExpressionValue::Angle(_))
    ));
}

#[test]
fn relation_quantity_serialization_preserves_base_power_fields() {
    let value = crate::curve::CurveExpressionQuantity::new(3.5, [1, 2, 0, 0, 0])
        .expect("finite residual quantity");
    assert_eq!(
        serde_json::to_string(&value).expect("quantity JSON"),
        r#"{"value":3.5,"length_power":1,"mass_power":2,"time_power":0,"angle_power":0,"temperature_power":0}"#
    );
}

#[test]
fn relation_local_exponent_nesting_ceiling_refuses() {
    let source = format!("{}1", "1^".repeat(129));
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 1024;
    let error = with_policy(policy, |ctx| {
        parse_relation_expression::<CurveExpressionValue>(
            ctx,
            &source,
            &BTreeMap::new(),
            RelationEvaluationContext::default(),
        )
    })
    .expect_err("local exponent nesting ceiling");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.operation == "creo relation nesting ceiling")
    );
}

#[test]
fn relation_local_function_nesting_ceiling_refuses() {
    let source = format!("{}1{}", "abs(".repeat(129), ")".repeat(129));
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 1024;
    let error = with_policy(policy, |ctx| {
        parse_relation_expression::<CurveExpressionValue>(
            ctx,
            &source,
            &BTreeMap::new(),
            RelationEvaluationContext::default(),
        )
    })
    .expect_err("local function nesting ceiling");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.operation == "creo relation nesting ceiling")
    );
}

#[test]
fn relation_unit_source_scan_refuses_work_before_nonrecursive_parse() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = crate::test_support::allocation_limit_at(
        ResourceDimension::WorkUnits,
        Some("creo relation unit source scan"),
        |cap| {
            let mut trial = policy;
            trial.limits.max_work_units = cap;
            with_policy(trial, |ctx| {
                relation_unit(ctx, "mm*mm/mm").map(|unit| unit.is_some())
            })
        },
    );
    let error = with_policy(policy, |ctx| {
        relation_unit(ctx, "mm*mm/mm").map(|unit| unit.is_some())
    })
    .expect_err("unit source work");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.operation == "creo relation unit source scan")
    );
}

#[test]
fn relation_stack_operators_refuse_work_after_source_admission() {
    let value =
        crate::test_support::assert_work_boundaries(&["creo relation source scan"], |ctx| {
            parse_relation_expression::<CurveExpressionValue>(
                ctx,
                "1+2*3",
                &BTreeMap::new(),
                RelationEvaluationContext::default(),
            )
        });
    assert_eq!(
        value,
        crate::curve::quantity_value(7.0, crate::curve::RelationDimension::default())
    );
}
