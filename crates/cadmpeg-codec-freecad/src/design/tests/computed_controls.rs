// SPDX-License-Identifier: Apache-2.0
//! Assertions for native signs represented by canonical dependent expressions.

use cadmpeg_ir::features::{DimensionValue, Expression, ExpressionToken, ParameterValue};
use cadmpeg_ir::{document::DimensionEditError, CadIr};
use serde::{de::DeserializeOwned, Serialize};

pub(super) fn assert_computed_control<Q: DimensionValue + Serialize + DeserializeOwned>(
    ir: &CadIr,
    feature_name: &str,
    path: &str,
    source_name: &str,
    raw_value: &ParameterValue,
    prefix: &str,
    semantic_value: Q,
) {
    let owner = ir
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some(feature_name))
        .unwrap()
        .id
        .clone();
    let raw = ir
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.owner.as_ref() == Some(&owner) && parameter.name == source_name)
        .unwrap();
    assert_eq!(raw.value.as_ref(), Some(raw_value));
    assert_eq!(raw.expression, Expression::literal(raw_value));
    assert!(raw.driving);
    assert!(raw.native_ref.is_some());
    let definition = serde_json::to_value(
        ir.model
            .features
            .iter()
            .find(|feature| feature.id == owner)
            .unwrap()
            .definition(),
    )
    .unwrap();
    let computed_id = definition.pointer(path).unwrap()["value"].as_str().unwrap();
    let computed = ir
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.id.as_str() == computed_id)
        .unwrap();
    assert!(!computed.driving);
    assert_eq!(computed.value, semantic_value.to_parameter_value());
    assert_eq!(computed.native_ref, None);
    assert_eq!(
        computed.expression.tokens,
        vec![
            ExpressionToken::Text(prefix.into()),
            ExpressionToken::Parameter(raw.id.clone()),
            ExpressionToken::Text(")".into()),
        ]
    );
    let mut edited = ir.clone();
    let before = serde_json::to_value(&edited).unwrap();
    assert_eq!(
        edited.set_dimension(&owner, path, semantic_value),
        Err(DimensionEditError::DrivenParameter {
            parameter: computed.id.clone(),
            drivers: vec![raw.id.clone()],
        })
    );
    assert_eq!(serde_json::to_value(&edited).unwrap(), before);
    let changed = match raw_value {
        ParameterValue::Length(value) => {
            ParameterValue::Length(cadmpeg_ir::scalar::Length::new(value.get() * 2.0).unwrap())
        }
        ParameterValue::Angle(value) => {
            ParameterValue::Angle(cadmpeg_ir::scalar::Angle::new(value.get() * 2.0).unwrap())
        }
        _ => panic!("signed-control regression requires a length or angle"),
    };
    edited
        .set_parameter_expression(
            &raw.id,
            Expression::literal(&changed),
            Some(changed.clone()),
        )
        .unwrap();
    assert_eq!(
        edited
            .model
            .parameters
            .iter()
            .find(|parameter| parameter.id == raw.id)
            .unwrap()
            .value,
        Some(changed)
    );
    assert!(
        edited
            .model
            .features
            .iter()
            .find(|feature| feature.id == owner)
            .unwrap()
            .needs_recompute
    );
    // Stored dependent caches are preserved as stale evidence, never rewritten
    // to claim an unevaluated edit has already recomputed the feature.
    assert_eq!(
        edited
            .model
            .parameters
            .iter()
            .find(|parameter| parameter.id == computed.id)
            .unwrap(),
        computed
    );
}
