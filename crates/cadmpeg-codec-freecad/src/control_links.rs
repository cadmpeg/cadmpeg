// SPDX-License-Identifier: Apache-2.0
//! Current expression links between native controls and canonical parameters.
//!
//! A row declares a control's role. Its target is always read from the current
//! typed ExpressionEngine expression; the row contains no identity, value,
//! cached definition, source digest, or historical authority.

use crate::native::{PropertyBody, PropertyRecord};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{Expression, ExpressionToken, ParameterId, ParameterValue};
use cadmpeg_ir::{report::loss::LossNote, CadIr};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) const PROPERTY: &str = "CadmpegControlLinks";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Measurement {
    Direct,
    Diameter,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ControlLink {
    pub(crate) control: String,
    pub(crate) measurement: Measurement,
}

fn target(expression: &Expression, measurement: Measurement) -> Option<&ParameterId> {
    let mut target = None;
    let mut text = String::new();
    for token in &expression.tokens {
        match token {
            ExpressionToken::Parameter(id) if target.is_none() => target = Some(id),
            ExpressionToken::Text(value) => {
                text.extend(value.chars().filter(|c| !c.is_whitespace()))
            }
            ExpressionToken::Parameter(_) => return None,
        }
    }
    let expected = match measurement {
        Measurement::Direct => "",
        Measurement::Diameter => "/2",
    };
    (text == expected).then_some(target).flatten()
}

fn cache_agrees(
    control: Option<&ParameterValue>,
    parameter: Option<&ParameterValue>,
    measurement: Measurement,
) -> bool {
    let close = |left: f64, right: f64| {
        (left - right).abs() <= 32.0 * f64::EPSILON * left.abs().max(right.abs()).max(1.0)
    };
    match (control, parameter, measurement) {
        (
            Some(ParameterValue::Length(control)),
            Some(ParameterValue::Length(parameter)),
            Measurement::Diameter,
        ) => close(control.get(), parameter.get() / 2.0),
        (
            Some(ParameterValue::Length(control)),
            Some(ParameterValue::Length(parameter)),
            Measurement::Direct,
        ) => close(control.get(), parameter.get()),
        (
            Some(ParameterValue::Angle(control)),
            Some(ParameterValue::Angle(parameter)),
            Measurement::Direct,
        ) => close(control.get(), parameter.get()),
        (
            Some(ParameterValue::Real(control)),
            Some(ParameterValue::Real(parameter)),
            Measurement::Direct,
        ) => close(control.get(), parameter.get()),
        (Some(control), Some(parameter), Measurement::Direct) => control == parameter,
        _ => false,
    }
}

fn cache_quantity_matches(
    control: Option<&ParameterValue>,
    parameter: Option<&ParameterValue>,
    measurement: Measurement,
) -> bool {
    match (control, parameter, measurement) {
        (Some(ParameterValue::Length(_)), Some(ParameterValue::Length(_)), _) => true,
        (Some(ParameterValue::Angle(_)), Some(ParameterValue::Angle(_)), Measurement::Direct)
        | (Some(ParameterValue::Real(_)), Some(ParameterValue::Real(_)), Measurement::Direct)
        | (
            Some(ParameterValue::Integer(_)),
            Some(ParameterValue::Integer(_)),
            Measurement::Direct,
        )
        | (
            Some(ParameterValue::Boolean(_)),
            Some(ParameterValue::Boolean(_)),
            Measurement::Direct,
        )
        | (Some(ParameterValue::String(_)), Some(ParameterValue::String(_)), Measurement::Direct) => {
            true
        }
        _ => false,
    }
}

/// Reconstruct bindings only from a verified current expression. Unverified
/// rows remain ordinary typed native metadata, with an explicit decode loss.
pub(crate) fn apply(
    ir: &mut CadIr,
    properties: &[PropertyRecord],
) -> Result<Vec<LossNote>, CodecError> {
    let mut aliases = BTreeMap::new();
    let mut losses = Vec::new();
    let source_controls = properties
        .iter()
        .map(|property| {
            (
                (property.owner.as_str(), property.name.as_str()),
                property.id.as_str(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let parameters_by_native = ir
        .model
        .parameters
        .iter()
        .filter_map(|parameter| {
            parameter
                .native_ref
                .as_deref()
                .map(|native| (native, parameter))
        })
        .collect::<BTreeMap<_, _>>();
    let parameters_by_id = ir
        .model
        .parameters
        .iter()
        .map(|parameter| (&parameter.id, parameter))
        .collect::<BTreeMap<_, _>>();
    for property in properties.iter().filter(|property| {
        property.name == PROPERTY
            && matches!(&property.body, PropertyBody::Persisted { dynamic: Some(dynamic), .. } if dynamic.group == "CADIR")
    }) {
        let mut controls = BTreeSet::new();
        for value in property.values().iter().filter(|value| value.tag == "String") {
            let row: ControlLink = serde_json::from_str(value.attributes.get("value").ok_or_else(|| {
                CodecError::malformed("control-link row lacks its typed value")
            })?).map_err(CodecError::malformed)?;
            if !controls.insert(row.control.clone()) {
                return Err(CodecError::malformed("duplicate native control link"));
            }
            let control = source_controls.get(&(property.owner.as_str(), row.control.as_str()))
                .and_then(|native| parameters_by_native.get(native).copied());
            let verified = control.and_then(|control| {
                let target = target(&control.expression, row.measurement)?;
                let parameter = parameters_by_id.get(target)?;
                let stale = control.owner.as_ref().is_some_and(|owner| ir.model.features.iter().any(|feature| feature.id == *owner && feature.needs_recompute));
                (target != &control.id
                    && cache_quantity_matches(control.value.as_ref(), parameter.value.as_ref(), row.measurement)
                    && (stale || cache_agrees(control.value.as_ref(), parameter.value.as_ref(), row.measurement)))
                    .then(|| (control.id.clone(), target.clone(), row.measurement))
            });
            if let Some((control, parameter, measurement)) = verified {
                aliases.insert(control, (parameter, measurement));
            } else {
                losses.push(crate::loss::FreecadLossCode::ExpressionReferenceUnresolved.note(format!(
                    "native control {}:{} has no matching current single-parameter expression and evaluated control for its declared measurement", property.owner, row.control
                )));
            }
        }
    }
    // A generated bridge points directly to a canonical parameter, never to
    // another bridge. Cyclic or chained declarations remain explicit records.
    let bridge_ids = aliases.keys().cloned().collect::<BTreeSet<_>>();
    aliases.retain(|control, (target, _)| {
        if bridge_ids.contains(target) {
            losses.push(crate::loss::FreecadLossCode::ExpressionReferenceUnresolved.note(format!("native control {control} references another generated control; current parameter records remain explicit")));
            false
        } else {
            true
        }
    });
    if aliases.is_empty() {
        return Ok(losses);
    }
    for feature in &mut ir.model.features {
        let mut definition =
            serde_json::to_value(feature.evaluation.definition()).map_err(CodecError::malformed)?;
        replace_dimensions(&mut definition, &aliases);
        feature
            .evaluation
            .set_definition(serde_json::from_value(definition).map_err(CodecError::malformed)?);
    }
    for parameter in &mut ir.model.parameters {
        let mut tokens = Vec::new();
        for token in &parameter.expression.tokens {
            match token {
                ExpressionToken::Parameter(id) if aliases.contains_key(id) => {
                    let (target, measurement) = &aliases[id];
                    if *measurement == Measurement::Diameter {
                        tokens.push(ExpressionToken::Text("(".into()));
                    }
                    tokens.push(ExpressionToken::Parameter(target.clone()));
                    if *measurement == Measurement::Diameter {
                        tokens.push(ExpressionToken::Text(" / 2)".into()));
                    }
                }
                token => tokens.push(token.clone()),
            }
        }
        parameter.expression = Expression { tokens };
    }
    ir.model
        .parameters
        .retain(|parameter| !aliases.contains_key(&parameter.id));
    Ok(losses)
}

fn replace_dimensions(
    value: &mut serde_json::Value,
    aliases: &BTreeMap<ParameterId, (ParameterId, Measurement)>,
) {
    if value["kind"] == "radius" && value["value"]["kind"] == "parameter" {
        if let Some(id) = value["value"]["value"].as_str() {
            if aliases.iter().any(|(source, (_, measurement))| {
                source.as_str() == id && *measurement == Measurement::Diameter
            }) {
                value["kind"] = "diameter".into();
            }
        }
    }
    if value["kind"] == "parameter" {
        if let Some(id) = value["value"].as_str() {
            if let Some((target, _)) = aliases
                .iter()
                .find_map(|(source, target)| (source.as_str() == id).then_some(target))
            {
                value["value"] = serde_json::Value::String(target.as_str().to_owned());
            }
        }
        return;
    }
    match value {
        serde_json::Value::Object(object) => {
            for (key, child) in object {
                if key != "tokens" {
                    replace_dimensions(child, aliases);
                }
            }
        }
        serde_json::Value::Array(array) => {
            for child in array {
                replace_dimensions(child, aliases);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bridge_requires_current_cache_agreement_and_quantity() {
        let length =
            |value| ParameterValue::Length(cadmpeg_ir::scalar::Length::new(value).unwrap());
        assert!(cache_agrees(
            Some(&length(3.0)),
            Some(&length(6.0)),
            Measurement::Diameter
        ));
        assert!(!cache_agrees(
            Some(&length(3.0)),
            Some(&length(6.0)),
            Measurement::Direct
        ));
        assert!(!cache_agrees(
            Some(&length(99.0)),
            Some(&length(6.0)),
            Measurement::Diameter
        ));
        assert!(!cache_agrees(
            Some(&length(3.0)),
            None,
            Measurement::Diameter
        ));
    }

    #[test]
    fn control_metadata_cannot_redirect_an_unrelated_current_formula() {
        let id = ParameterId::mint("test:parameter:dimension#0").unwrap();
        let direct = Expression {
            tokens: vec![ExpressionToken::Parameter(id.clone())],
        };
        assert_eq!(target(&direct, Measurement::Direct), Some(&id));
        assert!(target(&direct, Measurement::Diameter).is_none());
        let half = Expression {
            tokens: vec![
                ExpressionToken::Parameter(id.clone()),
                ExpressionToken::Text(" / 2".into()),
            ],
        };
        assert_eq!(target(&half, Measurement::Diameter), Some(&id));
        assert!(target(&half, Measurement::Direct).is_none());
        let edited = Expression {
            tokens: vec![
                ExpressionToken::Parameter(id),
                ExpressionToken::Text(" + 2 mm".into()),
            ],
        };
        assert!(target(&edited, Measurement::Direct).is_none());
        assert!(target(&edited, Measurement::Diameter).is_none());
    }
}
