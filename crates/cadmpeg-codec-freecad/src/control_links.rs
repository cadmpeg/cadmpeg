// SPDX-License-Identifier: Apache-2.0
//! Current expression links between native controls and canonical parameters.
//!
//! A control is a feature-definition dimension whose current typed
//! `ExpressionEngine` expression names one canonical parameter, directly or
//! halved. Its target is always read from that expression alone; no stored
//! row carries identity, value, cached definition, source digest, or
//! historical authority.

use crate::native::PropertyRecord;
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
                text.extend(value.chars().filter(|c| !c.is_whitespace()));
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
    matches!(
        (control, parameter, measurement),
        (
            Some(ParameterValue::Length(_)),
            Some(ParameterValue::Length(_)),
            _
        ) | (
            Some(ParameterValue::Angle(_)),
            Some(ParameterValue::Angle(_)),
            Measurement::Direct,
        ) | (
            Some(ParameterValue::Real(_)),
            Some(ParameterValue::Real(_)),
            Measurement::Direct,
        ) | (
            Some(ParameterValue::Integer(_)),
            Some(ParameterValue::Integer(_)),
            Measurement::Direct,
        ) | (
            Some(ParameterValue::Boolean(_)),
            Some(ParameterValue::Boolean(_)),
            Measurement::Direct,
        ) | (
            Some(ParameterValue::String(_)),
            Some(ParameterValue::String(_)),
            Measurement::Direct
        )
    )
}

/// Properties driven by a current `ExpressionEngine` row.
/// A row path names its property, or a child of the sketch constraint
/// property when the row addresses one constraint. These are the same keys
/// `design.rs` resolves through `expression_binding`.
fn driven_properties(properties: &[PropertyRecord]) -> BTreeSet<String> {
    let by_owner_name = properties
        .iter()
        .map(|property| {
            (
                (property.owner.as_str(), property.name.as_str()),
                property.id.as_str(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut driven = BTreeSet::new();
    for engine in properties
        .iter()
        .filter(|property| property.type_name == "App::PropertyExpressionEngine")
    {
        for value in engine
            .values()
            .iter()
            .filter(|value| value.tag == "Expression")
        {
            let Some(path) = value.attributes.get("path") else {
                continue;
            };
            if let Some(id) = by_owner_name.get(&(engine.owner.as_str(), path.as_str())) {
                driven.insert((*id).to_owned());
                continue;
            }
            if path.starts_with("Constraints[") || path.starts_with(".Constraints.") {
                if let Some(id) = by_owner_name.get(&(engine.owner.as_str(), "Constraints")) {
                    driven.insert((*id).to_owned());
                }
            }
        }
    }
    driven
}

/// Reference sites of parameter ids across the neutral model. A bridge
/// rewrites every site: feature definitions convert radius kinds for halved
/// controls, parameter expressions parenthesise the halved target, and
/// constraint definitions substitute directly. A site the rewrite cannot
/// carry keeps its alias.
#[derive(Default)]
struct Sites {
    /// Consumed in a diameter-kind feature definition position.
    feature_diameter: BTreeSet<String>,
    /// Referenced in a feature definition outside diameter context.
    feature_plain: BTreeSet<String>,
    /// Named by another parameter's expression.
    parameter: BTreeSet<String>,
    /// Bound by a sketch constraint definition.
    constraint: BTreeSet<String>,
}

/// Walk one definition, recording referenced parameter ids. Diameter context
/// comes from the definition exactly as the writer derives it: a reference
/// under a `diameter`-kind node. Expression token subtrees never name a
/// consumed dimension.
fn definition_sites(value: &serde_json::Value, diameter: bool, sites: &mut Sites) {
    match value {
        serde_json::Value::Object(fields)
            if fields.len() == 2
                && fields.get("kind").and_then(serde_json::Value::as_str) == Some("parameter") =>
        {
            if let Some(id) = fields.get("value").and_then(serde_json::Value::as_str) {
                if diameter {
                    sites.feature_diameter.insert(id.to_owned());
                } else {
                    sites.feature_plain.insert(id.to_owned());
                }
            }
            return;
        }
        serde_json::Value::Object(fields) => {
            let diameter = diameter
                || fields.get("kind").and_then(serde_json::Value::as_str) == Some("diameter");
            for (key, child) in fields {
                if key != "tokens" {
                    definition_sites(child, diameter, sites);
                }
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                definition_sites(item, diameter, sites);
            }
        }
        _ => {}
    }
}

/// Reconstruct bindings only from a verified current expression. Unverified
/// controls remain ordinary typed native metadata, with an explicit decode loss.
///
/// #209 D31: candidates come from the current `ExpressionEngine` rows
/// alone, never from stored control-link rows, so the bridge applies to every
/// FreeCAD file. The target, quantity, cache, and no-chaining checks stay.
pub(crate) fn apply(
    ir: &mut CadIr,
    properties: &[PropertyRecord],
) -> Result<Vec<LossNote>, CodecError> {
    let mut aliases = BTreeMap::new();
    let mut losses = Vec::new();
    let driven = driven_properties(properties);
    let parameters_by_id = ir
        .model
        .parameters
        .iter()
        .map(|parameter| (&parameter.id, parameter))
        .collect::<BTreeMap<_, _>>();
    let mut sites = Sites::default();
    for feature in &ir.model.features {
        let definition =
            serde_json::to_value(feature.evaluation.definition()).map_err(CodecError::malformed)?;
        definition_sites(&definition, false, &mut sites);
    }
    for parameter in &ir.model.parameters {
        for token in &parameter.expression.tokens {
            if let ExpressionToken::Parameter(id) = token {
                sites.parameter.insert(id.as_str().to_owned());
            }
        }
    }
    for constraint in &ir.model.sketch_constraints {
        let definition =
            serde_json::to_value(&constraint.definition).map_err(CodecError::malformed)?;
        let mut local = Sites::default();
        definition_sites(&definition, false, &mut local);
        sites.constraint.extend(local.feature_plain);
        sites.constraint.extend(local.feature_diameter);
    }
    for constraint in &ir.model.spatial_sketch_constraints {
        let definition =
            serde_json::to_value(&constraint.definition).map_err(CodecError::malformed)?;
        let mut local = Sites::default();
        definition_sites(&definition, false, &mut local);
        sites.constraint.extend(local.feature_plain);
        sites.constraint.extend(local.feature_diameter);
    }
    for index in 0..ir.model.parameters.len() {
        let control = &ir.model.parameters[index];
        if !control
            .native_ref
            .as_deref()
            .is_some_and(|native| driven.contains(native))
        {
            continue;
        }
        // A halved control bridges only where its consumers carry the
        // halving: a diameter-kind feature position, or another expression.
        // Constraint definitions hold no halved reference, and a plain
        // feature position would silently drop the divisor, so those keep
        // their verified alias without a loss.
        let diameter_bridgeable = |id: &str| {
            sites.feature_diameter.contains(id)
                && !sites.feature_plain.contains(id)
                && !sites.constraint.contains(id)
        };
        let candidate = target(&control.expression, Measurement::Direct)
            .map(|target| (target.clone(), Measurement::Direct))
            .or_else(|| {
                target(&control.expression, Measurement::Diameter)
                    .map(|target| (target.clone(), Measurement::Diameter))
                    .filter(|_| diameter_bridgeable(control.id.as_str()))
            });
        let Some((target, measurement)) = candidate else {
            continue;
        };
        let verified = parameters_by_id.get(&target).and_then(|parameter| {
            let stale = control.owner.as_ref().is_some_and(|owner| {
                ir.model
                    .features
                    .iter()
                    .any(|feature| feature.id == *owner && feature.needs_recompute)
            });
            (target != control.id
                && cache_quantity_matches(
                    control.value.as_ref(),
                    parameter.value.as_ref(),
                    measurement,
                )
                && (stale
                    || cache_agrees(
                        control.value.as_ref(),
                        parameter.value.as_ref(),
                        measurement,
                    )))
            .then(|| (control.id.clone(), target.clone(), measurement))
        });
        if let Some((control, parameter, measurement)) = verified {
            aliases.insert(control, (parameter, measurement));
        } else {
            losses.push(crate::loss::FreecadLossCode::ExpressionReferenceUnresolved.note(format!(
                "parameter {} has no matching current single-parameter expression and evaluated control", control.id
            )));
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
    for constraint in &mut ir.model.sketch_constraints {
        let mut definition =
            serde_json::to_value(&constraint.definition).map_err(CodecError::malformed)?;
        replace_dimensions(&mut definition, &aliases);
        constraint.definition =
            serde_json::from_value(definition).map_err(CodecError::malformed)?;
    }
    for constraint in &mut ir.model.spatial_sketch_constraints {
        let mut definition =
            serde_json::to_value(&constraint.definition).map_err(CodecError::malformed)?;
        replace_dimensions(&mut definition, &aliases);
        constraint.definition =
            serde_json::from_value(definition).map_err(CodecError::malformed)?;
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
