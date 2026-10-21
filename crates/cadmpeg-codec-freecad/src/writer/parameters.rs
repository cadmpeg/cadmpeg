// SPDX-License-Identifier: Apache-2.0
//! Feature expression bindings and authored `App::VarSet` global parameters.

mod controls;
mod expressions;
mod names;

use super::xml::{self, Object};
use crate::loss::FreecadLossCode;
use cadmpeg_core::CodecError;
use cadmpeg_ir::sketches::SketchConstraintDefinitionInput as Constraint;
use cadmpeg_ir::{document::CadIr, features::ParameterValue, report::loss::LossNote};
use std::collections::HashMap;

/// The signed native Value can carry both authored driver and shell direction
/// only while the current neutral magnitude is exactly its abs computation.
/// A historical source kind cannot override an independently edited operand.
pub(in crate::writer) fn has_signed_thickness_driver(
    ir: &CadIr,
    feature: &cadmpeg_ir::features::Feature,
) -> bool {
    use cadmpeg_ir::features::{ExpressionToken, FeatureOperation};
    if feature.source_tag.as_deref() != Some("Part::Thickness") {
        return false;
    }
    let FeatureOperation::Shell {
        thickness: Some(thickness),
        outward: Some(outward),
        ..
    } = feature.definition().operation()
    else {
        return false;
    };
    let Some(computed) = thickness.parameter().and_then(|id| {
        ir.model
            .parameters
            .iter()
            .find(|parameter| parameter.id == *id)
    }) else {
        return false;
    };
    let [ExpressionToken::Text(prefix), ExpressionToken::Parameter(raw), ExpressionToken::Text(suffix)] =
        computed.expression.tokens.as_slice()
    else {
        return false;
    };
    if prefix != "abs(" || suffix != ")" {
        return false;
    }
    let Some(raw) = ir.model.parameters.iter().find(|parameter| {
        parameter.id == *raw
            && parameter.owner.as_ref() == Some(&feature.id)
            && parameter.name == "Value"
    }) else {
        return false;
    };
    matches!((&raw.value, &computed.value),
        (Some(ParameterValue::Length(raw)), Some(ParameterValue::Length(magnitude)))
        if raw.get().abs() == magnitude.get() && (raw.get() > 0.0) == *outward)
}

pub(in crate::writer) fn parameter_reference(
    ir: &CadIr,
    id: &cadmpeg_ir::features::ParameterId,
    current_object: Option<&str>,
    names: &HashMap<String, String>,
) -> Option<String> {
    let dependency = ir.model.parameters.iter().find(|value| value.id == *id)?;
    let constraint_path = ir.model.sketch_constraints.iter().find_map(|constraint| {
        constraint
            .definition
            .kind()
            .dimension_parameters()
            .ok()
            .filter(|parameters| parameters.contains(id))
            .and_then(|_| names.get(&format!("constraint-path:{}", constraint.id)))
    });
    let symbol = constraint_path
        .or_else(|| names.get(&format!("parameter-path:{}", dependency.id)))
        .unwrap_or(&dependency.name);
    let emitted_owner = dependency
        .owner
        .as_ref()
        .and_then(|owner| names.get(owner.as_str()))
        .or_else(|| names.get(&format!("parameter-owner:{}", dependency.id)))?;
    if Some(emitted_owner.as_str()) == current_object {
        return Some(symbol.clone());
    }
    if let Some(label) = dependency
        .owner
        .as_ref()
        .and_then(|owner| {
            ir.model
                .features
                .iter()
                .find(|feature| feature.id == *owner)
        })
        .and_then(|feature| feature.name.as_deref())
    {
        if label.contains(">>") {
            return None;
        }
        return Some(format!("<<{label}>>.{symbol}"));
    }
    Some(format!("{emitted_owner}.{symbol}"))
}

pub(in crate::writer) fn render_expression(
    ir: &CadIr,
    parameter: &cadmpeg_ir::features::DesignParameter,
    names: &HashMap<String, String>,
) -> Option<String> {
    let current = parameter
        .owner
        .as_ref()
        .and_then(|owner| names.get(owner.as_str()))
        .or_else(|| names.get(&format!("parameter-owner:{}", parameter.id)));
    parameter
        .expression
        .render_with(|id| parameter_reference(ir, id, current.map(String::as_str), names))
}

/// Allocate each ownerless parameter once. Existing typed `VarSets` remain the
/// same owners; authored globals share one deterministic new `VarSet`.
pub(super) fn prepare(
    ir: &CadIr,
    objects: &mut Vec<Object>,
    names: &mut HashMap<String, String>,
) -> Result<(), CodecError> {
    let properties = ir
        .native
        .namespace("fcstd")
        .map(|namespace| namespace.arena_as::<crate::native::PropertyRecord>("properties"))
        .transpose()?
        .unwrap_or_default();
    let mut authored = None;
    for parameter in ir
        .model
        .parameters
        .iter()
        .filter(|parameter| parameter.owner.is_none())
    {
        let retained = parameter
            .native_ref
            .as_ref()
            .and_then(|id| properties.iter().find(|property| &property.id == id))
            .and_then(|property| names.get(&property.owner))
            .filter(|name| objects.iter().any(|object| &object.name == *name))
            .cloned();
        let owner = retained.unwrap_or_else(|| {
            authored
                .get_or_insert_with(|| {
                    let mut name = "Parameters".to_owned();
                    while objects.iter().any(|object| object.name == name) {
                        name.push('_');
                    }
                    objects.push(Object::new(name.clone(), "App::VarSet", "Parameters"));
                    name
                })
                .clone()
        });
        names.insert(format!("parameter-owner:{}", parameter.id), owner);
    }
    names::prepare(ir, objects, names)?;
    Ok(())
}

pub(super) fn write(
    ir: &CadIr,
    objects: &mut [Object],
    names: &HashMap<String, String>,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let mut expressions = expressions::native_only(ir, names)?;
    let mut constraints = HashMap::new();
    let mut omitted_constraints = std::collections::HashSet::new();
    for sketch in &ir.model.sketches {
        for constraint in ir
            .model
            .sketch_constraints
            .iter()
            .filter(|constraint| constraint.sketch == sketch.id)
        {
            let mut ids = constraint
                .definition
                .kind()
                .dimension_parameters()
                .map_err(CodecError::malformed)?;
            if let Constraint::Native {
                parameter: Some(parameter),
                ..
            } = constraint.definition.kind()
            {
                if !ids.contains(parameter) {
                    ids.push(parameter.clone());
                }
            }
            for parameter in ids {
                if let Some(path) = names.get(&format!("constraint-path:{}", constraint.id)) {
                    constraints.insert(
                        parameter.as_str().to_owned(),
                        (sketch.id.as_str(), path.clone()),
                    );
                } else {
                    omitted_constraints.insert(parameter.as_str().to_owned());
                }
            }
        }
    }
    for parameter in &ir.model.parameters {
        if omitted_constraints.contains(parameter.id.as_str()) {
            losses.push(FreecadLossCode::ExportFeatureNotTransferred.note(format!(
                "parameter {} belongs to an omitted sketch constraint",
                parameter.id
            )));
            continue;
        }
        let Some(expression) = render_expression(ir, parameter, names) else {
            losses.push(FreecadLossCode::ExportFeatureNotTransferred.note(format!(
                "parameter {} has a dangling expression reference or an unencodable owner label",
                parameter.id
            )));
            continue;
        };
        if ir.model.spreadsheets.iter().any(|sheet| {
            sheet
                .cells()
                .iter()
                .any(|cell| cell.parameter == parameter.id)
        }) {
            continue;
        }
        if let Some((sketch, path)) = constraints.get(parameter.id.as_str()) {
            if let Some(name) = names.get(*sketch) {
                expressions
                    .entry(name.clone())
                    .or_default()
                    .push(xml::element(
                        "Expression",
                        &[("path", path.clone()), ("expression", expression.clone())],
                    ));
            }
            continue;
        }
        let property_name = names
            .get(&format!("parameter-path:{}", parameter.id))
            .map_or(&parameter.name, String::as_str);
        let property = match &parameter.value {
            Some(ParameterValue::Length(value)) => {
                xml::scalar(property_name, "App::PropertyLength", "Float", value.get())
            }
            Some(ParameterValue::Angle(value)) => xml::scalar(
                property_name,
                "App::PropertyAngle",
                "Float",
                value.get().to_degrees(),
            ),
            Some(ParameterValue::Real(value)) => {
                xml::scalar(property_name, "App::PropertyFloat", "Float", value.get())
            }
            Some(ParameterValue::Integer(value)) => {
                xml::scalar(property_name, "App::PropertyInteger", "Integer", value)
            }
            Some(ParameterValue::Boolean(value)) => {
                xml::scalar(property_name, "App::PropertyBool", "Bool", u8::from(*value))
            }
            Some(ParameterValue::String(value)) => xml::string(property_name, value),
            None => {
                losses.push(FreecadLossCode::ExportFeatureNotTransferred.note(format!(
                    "parameter {} has no evaluated value from which to type a native scalar property", parameter.id
                )));
                continue;
            }
        };
        let owner = parameter
            .owner
            .as_ref()
            .and_then(|owner| names.get(owner.as_str()))
            .cloned();
        let name = match owner {
            Some(name) if objects.iter().any(|object| object.name == name) => name,
            Some(_) => {
                losses.push(
                    FreecadLossCode::ExportFeatureNotTransferred
                        .note(format!("parameter {} has an omitted owner", parameter.id)),
                );
                continue;
            }
            None => names
                .get(&format!("parameter-owner:{}", parameter.id))
                .cloned()
                .ok_or_else(|| CodecError::malformed("global parameter owner was not prepared"))?,
        };
        let object = objects
            .iter_mut()
            .find(|object| object.name == name)
            .ok_or_else(|| CodecError::malformed("parameter owner is absent"))?;
        // Dynamic metadata distinguishes authored scalar parameters from
        // application controls. Every expression has an explicit binding,
        // including a literal expression whose evaluated value differs.
        let property = property.replacen("<Property ", "<Property group=\"Parameters\" ", 1);
        object.set(property_name, property);
        expressions.entry(name).or_default().push(xml::element(
            "Expression",
            &[
                ("path", property_name.to_owned()),
                ("expression", expression),
            ],
        ));
    }
    controls::write(ir, objects, names, &mut expressions, losses)?;
    names::write(ir, objects, names)?;
    expressions::write(objects, expressions)?;
    Ok(())
}
