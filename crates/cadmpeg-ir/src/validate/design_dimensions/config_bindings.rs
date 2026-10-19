// SPDX-License-Identifier: Apache-2.0
//! Structural configuration overrides retain the canonical dimensional bindings.

use cadmpeg_core::{decode::DecodeContext, CodecError};
use serde_json::Value;

use crate::features::{ConfigurationFeatureState, DesignConfiguration};
use crate::report::{check::Check, check::Finding, Severity};
use crate::CadIr;

// These are the discriminants of the dimensional feature operands. A different
// variant supplies a different semantic slot, even when a field name coincides.
const DISCRIMINANTS: &[&str] = &[
    "construction",
    "definition",
    "kind",
    "missing",
    "mode",
    "reference",
    "region",
    "shape",
    "space",
    "state",
];

pub(super) fn check(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    configuration: &DesignConfiguration,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    for (id, state) in &configuration.feature_states {
        let ConfigurationFeatureState::Definition { definition } = state else {
            continue;
        };
        let Some(feature) = ir.model.features.iter().find(|feature| feature.id == *id) else {
            continue;
        };
        let before = serde_json::to_value(feature.definition())
            .map_err(|error| CodecError::malformed(error.to_string()))?;
        let after = serde_json::to_value(definition)
            .map_err(|error| CodecError::malformed(error.to_string()))?;
        let mut changed = Vec::new();
        changed_bindings(ctx, &before, &after, "", &mut changed)?;
        for (path, parameter) in changed {
            super::super::record_related_finding(
                findings,
                Check::ReferentialIntegrity,
                Severity::Error,
                configuration.id.as_str(),
                &[id.as_str(), &parameter],
                format_args!("configuration definition changes canonical parameter binding at {path}; dimensional differences belong in parameter overrides"),
            );
        }
    }
    Ok(())
}

fn changed_bindings(
    ctx: &DecodeContext<'_>,
    before: &Value,
    after: &Value,
    path: &str,
    changed: &mut Vec<(String, String)>,
) -> Result<(), CodecError> {
    let _depth = ctx.enter_nested("configuration dimensional binding comparison")?;
    match (before, after) {
        (Value::Object(before), Value::Object(after)) => {
            if before.len() == 2 && before.get("kind").and_then(Value::as_str) == Some("parameter")
            {
                if let Some(parameter) = before.get("value").and_then(Value::as_str) {
                    if before != after {
                        changed.push((path.to_owned(), parameter.to_owned()));
                    }
                    return Ok(());
                }
            }
            if DISCRIMINANTS.iter().any(|key| {
                before.get(*key).and_then(Value::as_str).is_some()
                    && before.get(*key) != after.get(*key)
            }) {
                return Ok(());
            }
            for (key, before) in before {
                if key == "tokens" {
                    continue;
                }
                if let Some(after) = after.get(key) {
                    let key = key.replace('~', "~0").replace('/', "~1");
                    changed_bindings(ctx, before, after, &format!("{path}/{key}"), changed)?;
                }
            }
        }
        (Value::Array(before), Value::Array(after)) => {
            for (index, (before, after)) in before.iter().zip(after).enumerate() {
                changed_bindings(ctx, before, after, &format!("{path}/{index}"), changed)?;
            }
        }
        _ => {}
    }
    Ok(())
}
