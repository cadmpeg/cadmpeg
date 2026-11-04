// SPDX-License-Identifier: Apache-2.0
//! Feature expression bindings and authored `App::VarSet` global parameters.

mod controls;
mod expressions;
mod names;

use super::xml::{self, Object};
use crate::loss::FreecadLossCode;
use cadmpeg_core::CodecError;
use cadmpeg_ir::sketches::SketchConstraintDefinitionInput as Constraint;
use cadmpeg_ir::{
    document::CadIr,
    features::{Expression, ParameterValue},
    report::loss::LossNote,
};
use std::collections::HashMap;

/// A binding that restates the value lane carries no formula the value
/// lane does not already hold. Omitting it keeps the re-decode from gaining
/// an engine binding the source never stated.
fn is_literal_binding(parameter: &cadmpeg_ir::features::DesignParameter) -> bool {
    parameter
        .value
        .as_ref()
        .is_some_and(|value| parameter.expression == Expression::literal(value))
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

/// Render an expression with every unresolvable reference written as text.
///
/// A reference the target cannot name is written by its neutral parameter
/// name, or by its identity when the parameter itself is absent, so a
/// spreadsheet cell never refuses (#238). A leading `=` would re-decode as
/// a formula, so a fallback that starts one is quoted as text.
pub(in crate::writer) fn render_expression_lossy(
    ir: &CadIr,
    parameter: &cadmpeg_ir::features::DesignParameter,
    names: &HashMap<String, String>,
) -> String {
    if let Some(rendered) = render_expression(ir, parameter, names) {
        return rendered;
    }
    let current = parameter
        .owner
        .as_ref()
        .and_then(|owner| names.get(owner.as_str()))
        .or_else(|| names.get(&format!("parameter-owner:{}", parameter.id)));
    let mut output = String::new();
    for token in &parameter.expression.tokens {
        match token {
            cadmpeg_ir::features::ExpressionToken::Text(text) => output.push_str(text),
            cadmpeg_ir::features::ExpressionToken::Parameter(id) => {
                let rendered = parameter_reference(ir, id, current.map(String::as_str), names)
                    .or_else(|| {
                        ir.model
                            .parameters
                            .iter()
                            .find(|candidate| candidate.id == *id)
                            .map(|candidate| candidate.name.clone())
                    })
                    .unwrap_or_else(|| id.as_str().to_owned());
                output.push_str(&rendered);
            }
        }
    }
    if output.starts_with('=') {
        output.insert(0, '\'');
    }
    output
}

/// Allocate each ownerless parameter once. Existing typed `VarSets` remain the
/// same owners; authored globals share one deterministic new
/// `Spreadsheet::Sheet` named `Parameters`, which every FreeCAD version
/// loads (`App::VarSet` does not load before FreeCAD 1.0). A `VarSet`
/// object present as a native record is re-emitted as a `VarSet`
/// (`writer/native/neutral.rs`); only the generated owner is a sheet.
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
                    let mut object = Object::new(name.clone(), "Spreadsheet::Sheet", "Parameters");
                    object.set(
                        "cells",
                        xml::property(
                            "cells",
                            "Spreadsheet::PropertySheet",
                            &xml::element("Cells", &[("Count", "0".to_owned())]),
                        ),
                    );
                    objects.push(object);
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
    // Each object's parameter-carrying properties enter the object in
    // ascending parameter ordinal order, ties by arena order; the property
    // declaration order is the ordinal's native home (#238 §5.5).
    let mut ordered: Vec<_> = ir.model.parameters.iter().enumerate().collect();
    ordered.sort_by_key(|(index, parameter)| {
        let owner = parameter
            .owner
            .as_ref()
            .and_then(|owner| names.get(owner.as_str()))
            .or_else(|| names.get(&format!("parameter-owner:{}", parameter.id)));
        (owner, parameter.ordinal, *index)
    });
    for (_, parameter) in ordered {
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
            if !is_literal_binding(parameter) {
                if let Some(name) = names.get(*sketch) {
                    expressions
                        .entry(name.clone())
                        .or_default()
                        .push(xml::element(
                            "Expression",
                            &[("path", path.clone()), ("expression", expression.clone())],
                        ));
                }
            }
            continue;
        }
        let property_name = names
            .get(&format!("parameter-path:{}", parameter.id))
            .map_or(parameter.name.as_str(), String::as_str);
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
        // application controls. A literal restates its value lane, so only a
        // formula needs an explicit binding.
        let property = property.replacen("<Property ", "<Property group=\"Parameters\" ", 1);
        // Each object's parameter-carrying properties enter the object in
        // ascending parameter ordinal order. A bound control keeps its slot
        // only by moving to its ordinal position with the current value;
        // the neutral parameter always wins over a replayed native value
        // (#238 §5.5, #209 §2.5).
        object.properties.shift_remove(property_name);
        // The dynamic property name is the parameter's native home when it
        // is a valid free identifier; an allocated selector normalizes the
        // name away and is a documented loss (#238 §5.5).
        if let Some(selector) = names.get(&format!("parameter-path:{}", parameter.id)) {
            if selector != &parameter.name {
                losses.push(FreecadLossCode::ParameterNameNotTransferred.note(format!(
                    "parameter {} is emitted as {selector}",
                    parameter.id
                )));
            }
        }
        // Property parameters have no driving flag in FreeCAD; a cleared
        // driving flag has no native home (#238 §5.5).
        if !parameter.driving {
            losses.push(
                FreecadLossCode::ParameterDrivingNotTransferred.note(format!(
                    "parameter {} driving flag is cleared with no native encoding",
                    parameter.id
                )),
            );
        }
        object.set(property_name, property);
        if !is_literal_binding(parameter) {
            expressions.entry(name).or_default().push(xml::element(
                "Expression",
                &[
                    ("path", property_name.to_owned()),
                    ("expression", expression),
                ],
            ));
        }
    }
    controls::write(ir, objects, names, &mut expressions, losses)?;
    expressions::write(objects, expressions)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadmpeg_ir::features::{Expression, ExpressionToken, ParameterId};
    use std::collections::BTreeMap;

    fn cell_parameter(expression: Expression) -> cadmpeg_ir::features::DesignParameter {
        cadmpeg_ir::features::DesignParameter {
            product_definition: None,
            id: ParameterId::mint("fcstd:design:parameter#A1").expect("parameter id"),
            owner: None,
            ordinal: 0,
            name: "A1".to_owned(),
            expression,
            display: None,
            value: None,
            driving: true,
            properties: BTreeMap::new(),
            pmi: None,
            native_ref: None,
        }
    }

    #[test]
    fn unresolvable_cell_reference_is_written_as_text() {
        let ir = cadmpeg_ir::CadIr::empty();
        let missing = ParameterId::mint("cadir:model:parameter#missing").expect("parameter id");
        let parameter = cell_parameter(Expression {
            tokens: vec![
                ExpressionToken::Text("=2*".to_owned()),
                ExpressionToken::Parameter(missing.clone()),
            ],
        });
        // The reference cannot be named, so it is written as text rather
        // than refused; the leading quote keeps it from re-decoding as a
        // formula (#238).
        assert_eq!(
            render_expression_lossy(&ir, &parameter, &HashMap::new()),
            format!("'=2*{}", missing.as_str())
        );
    }

    #[test]
    fn unnameable_cell_reference_falls_back_to_the_neutral_name() {
        let mut ir = cadmpeg_ir::CadIr::empty();
        let target = ParameterId::mint("fcstd:design:parameter#Width").expect("parameter id");
        ir.model.parameters.push(cell_parameter(Expression::literal(
            &cadmpeg_ir::features::ParameterValue::Real(
                cadmpeg_ir::scalar::FiniteReal::new(3.0).expect("finite"),
            ),
        )));
        ir.model.parameters[0].id = target.clone();
        ir.model.parameters[0].name = "Width".to_owned();
        let parameter = cell_parameter(Expression {
            tokens: vec![
                ExpressionToken::Text("=2*".to_owned()),
                ExpressionToken::Parameter(target),
            ],
        });
        // The parameter exists but has no emitted owner or path, so the
        // neutral name is written as text rather than refused (#238).
        assert_eq!(
            render_expression_lossy(&ir, &parameter, &HashMap::new()),
            "'=2*Width"
        );
    }
}
