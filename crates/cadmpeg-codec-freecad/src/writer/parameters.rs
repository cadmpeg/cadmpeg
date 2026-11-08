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

/// A literal uses the scalar property without an expression binding.
fn is_literal_binding(parameter: &cadmpeg_ir::features::DesignParameter) -> bool {
    cell_literal_value(parameter).is_some()
}

pub(in crate::writer) fn cell_literal_value(
    parameter: &cadmpeg_ir::features::DesignParameter,
) -> Option<ParameterValue> {
    if let Some(value) = &parameter.value {
        return parameter.expression.literal_value_as(value);
    }
    [
        ParameterValue::Real(cadmpeg_ir::scalar::FiniteReal::ZERO),
        ParameterValue::Length(cadmpeg_ir::scalar::Length::ZERO),
        ParameterValue::Angle(cadmpeg_ir::scalar::Angle::ZERO),
        ParameterValue::String(String::new()),
    ]
    .iter()
    .find_map(|value| parameter.expression.literal_value_as(value))
}

fn report_literal_normalization(
    parameter: &cadmpeg_ir::features::DesignParameter,
    losses: &mut Vec<LossNote>,
) {
    let Some(value) = cell_literal_value(parameter) else {
        return;
    };
    if parameter.expression != Expression::literal(&value) {
        losses.push(FreecadLossCode::ParameterExpressionNormalized.note(format!(
            "parameter {} literal expression is stored as a scalar value",
            parameter.id
        )));
    }
}

pub(in crate::writer) fn report_cell_normalization(
    parameter: &cadmpeg_ir::features::DesignParameter,
    content: &str,
    losses: &mut Vec<LossNote>,
) {
    if cell_literal_value(parameter).is_some() {
        report_literal_normalization(parameter, losses);
        return;
    }
    let had_marker = parameter
        .expression
        .tokens
        .iter()
        .find_map(|token| match token {
            cadmpeg_ir::features::ExpressionToken::Text(text) if text.is_empty() => None,
            cadmpeg_ir::features::ExpressionToken::Text(text) => Some(text.starts_with('=')),
            cadmpeg_ir::features::ExpressionToken::Parameter(_) => Some(false),
        })
        .unwrap_or(false);
    if content.starts_with('\'') || (content.starts_with('=') && !had_marker) {
        losses.push(FreecadLossCode::ParameterExpressionNormalized.note(format!(
            "parameter {} expression is stored as spreadsheet content {content:?}",
            parameter.id
        )));
    }
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
/// `Spreadsheet::Sheet` named `Parameters`, which every `FreeCAD` version
/// loads (`App::VarSet` does not load before `FreeCAD` 1.0). A `VarSet`
/// object present as a native record is re-emitted as a `VarSet`
/// (`writer/native/neutral.rs`); only the generated owner is a sheet.
///
/// Ownerless parameters without cells become cells of the generated sheet:
/// each is allocated a column-A address in parameter order, with the
/// parameter name as its alias when the name is a valid free identifier.
/// Cell parameters have no driving flag in `FreeCAD`, so a cleared driving
/// flag is a documented loss, as is a name with no valid alias (#238 §5.5).
pub(super) fn prepare(
    ir: &CadIr,
    objects: &mut Vec<Object>,
    names: &mut HashMap<String, String>,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let properties = ir
        .native
        .namespace("fcstd")
        .map(|namespace| namespace.arena_as::<crate::native::PropertyRecord>("properties"))
        .transpose()?
        .unwrap_or_default();
    for sheet in &ir.model.spreadsheets {
        if !names.contains_key(sheet.feature.as_str()) {
            let mut name = format!("Sheet{}", objects.len());
            while objects.iter().any(|object| object.name == name) {
                name.push('_');
            }
            objects.push(Object::new(
                name.clone(),
                "Spreadsheet::Sheet",
                sheet.id.as_str(),
            ));
            names.insert(sheet.feature.as_str().to_owned(), name);
        }
        let mut aliases = std::collections::HashSet::new();
        for cell in sheet.cells() {
            let Some(parameter) = ir.model.parameters.iter().find(|p| p.id == cell.parameter)
            else {
                continue;
            };
            let address = cell.address.a1();
            let alias = super::names::is_free_identifier(&parameter.name)
                && cadmpeg_ir::CellAddress::parse(&parameter.name).is_none()
                && aliases.insert(parameter.name.clone());
            let selector = if alias { &parameter.name } else { &address };
            names.insert(format!("parameter-path:{}", parameter.id), selector.clone());
            if selector != &parameter.name {
                losses.push(FreecadLossCode::ParameterNameNotTransferred.note(format!(
                    "parameter {} has no valid cell alias and is addressed as {address}",
                    parameter.id
                )));
            }
        }
    }
    let mut authored = None;
    for parameter in ir
        .model
        .parameters
        .iter()
        .filter(|parameter| parameter.owner.is_none())
    {
        if let Some((owner, address)) = ir.model.spreadsheets.iter().find_map(|sheet| {
            let cell = sheet
                .cells()
                .iter()
                .find(|cell| cell.parameter == parameter.id)?;
            let owner = names.get(sheet.feature.as_str())?;
            Some((owner.clone(), cell.address.a1()))
        }) {
            names.insert(format!("parameter-owner:{}", parameter.id), owner);
            names.insert(format!("parameter-cell:{}", parameter.id), address);
            continue;
        }
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
    let generated = authored.clone();
    // Allocate column-A addresses for ownerless parameters without cells,
    // in parameter order. Only parameters owned by the generated sheet take
    // addresses; retained owners keep their native properties.
    if let Some(sheet) = generated.clone() {
        let mut ordered: Vec<_> = ir.model.parameters.iter().enumerate().collect();
        ordered.sort_by_key(|(index, parameter)| (parameter.ordinal, *index));
        let mut allocated = Vec::new();
        let mut count = 0_u32;
        let mut aliases = std::collections::HashSet::new();
        for (_, parameter) in ordered {
            if parameter.owner.is_some() {
                continue;
            }
            if names.get(&format!("parameter-owner:{}", parameter.id)) != Some(&sheet) {
                continue;
            }
            if ir.model.spreadsheets.iter().any(|sheet| {
                sheet
                    .cells()
                    .iter()
                    .any(|cell| cell.parameter == parameter.id)
            }) {
                continue;
            }
            let Some(address) =
                cadmpeg_ir::CellAddress::new(count + 1, 1).map(cadmpeg_ir::CellAddress::a1)
            else {
                continue;
            };
            count += 1;
            let alias = super::names::is_free_identifier(&parameter.name)
                .then(|| parameter.name.clone())
                .filter(|name| {
                    cadmpeg_ir::CellAddress::parse(name).is_none() && aliases.insert(name.clone())
                });
            if alias.as_deref() != Some(parameter.name.as_str()) {
                losses.push(FreecadLossCode::ParameterNameNotTransferred.note(format!(
                    "parameter {} has no valid cell alias and is addressed as {address}",
                    parameter.id
                )));
            }
            if !parameter.driving {
                losses.push(
                    FreecadLossCode::ParameterDrivingNotTransferred.note(format!(
                        "parameter {} driving flag is cleared with no native encoding",
                        parameter.id
                    )),
                );
            }
            names.insert(format!("parameter-cell:{}", parameter.id), address.clone());
            names.insert(
                format!("parameter-path:{}", parameter.id),
                alias.clone().unwrap_or_else(|| address.clone()),
            );
            allocated.push((parameter, address, alias));
        }
        names::prepare(ir, objects, names)?;
        let mut cells = String::new();
        for (parameter, address, alias) in allocated {
            let content = super::design::spreadsheet_expression(ir, parameter, names);
            report_cell_normalization(parameter, &content, losses);
            let mut attributes: Vec<(&str, String)> =
                vec![("address", address.clone()), ("content", content)];
            if let Some(alias) = alias {
                attributes.push(("alias", alias));
            }
            cells.push_str(&xml::element("Cell", &attributes));
        }
        if let Some(object) = objects.iter_mut().find(|object| object.name == sheet) {
            object.set(
                "cells",
                xml::property(
                    "cells",
                    "Spreadsheet::PropertySheet",
                    &format!("<Cells Count=\"{count}\">{cells}</Cells>"),
                ),
            );
        }
    }
    if generated.is_none() {
        names::prepare(ir, objects, names)?;
    }
    Ok(())
}

pub(super) fn write(
    ir: &CadIr,
    objects: &mut [Object],
    names: &HashMap<String, String>,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let mut expressions = expressions::native_only(ir, names)?;
    let native_properties = ir
        .native
        .namespace("fcstd")
        .map(|namespace| namespace.arena_as::<crate::native::PropertyRecord>("properties"))
        .transpose()?
        .unwrap_or_default();
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
        if names.contains_key(&format!("parameter-cell:{}", parameter.id)) {
            continue;
        }
        if omitted_constraints.contains(parameter.id.as_str()) {
            losses.push(FreecadLossCode::ExportFeatureNotTransferred.note(format!(
                "parameter {} belongs to an omitted sketch constraint",
                parameter.id
            )));
            continue;
        }
        if ir.model.spreadsheets.iter().any(|sheet| {
            sheet
                .cells()
                .iter()
                .any(|cell| cell.parameter == parameter.id)
        }) {
            continue;
        }
        let expression = render_expression(ir, parameter, names);
        if expression.is_none() {
            losses.push(FreecadLossCode::ExportFeatureNotTransferred.note(format!(
                "parameter {} expression has a dangling reference or an unencodable owner label and is omitted",
                parameter.id
            )));
        }
        if let Some((sketch, path)) = constraints.get(parameter.id.as_str()) {
            if is_literal_binding(parameter) {
                report_literal_normalization(parameter, losses);
            } else if let (Some(name), Some(expression)) = (names.get(*sketch), &expression) {
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
            .map_or(parameter.name.as_str(), String::as_str);
        let value = parameter.value.as_ref().map(|value| {
            parameter
                .expression
                .literal_value_as(value)
                .unwrap_or_else(|| value.clone())
        });
        let property = match &value {
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
                let kind = parameter
                    .native_ref
                    .as_ref()
                    .and_then(|id| {
                        native_properties
                            .iter()
                            .find(|source| &source.id == id && source.name == property_name)
                    })
                    .map(|source| source.type_name.as_str())
                    .filter(|kind| *kind == "App::PropertyIntegerConstraint")
                    .unwrap_or("App::PropertyInteger");
                xml::scalar(property_name, kind, "Integer", value)
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
        let owner = match &parameter.owner {
            Some(owner) => names.get(owner.as_str()),
            None => names.get(&format!("parameter-owner:{}", parameter.id)),
        };
        let Some(object) =
            owner.and_then(|name| objects.iter_mut().find(|object| object.name == *name))
        else {
            losses.push(
                FreecadLossCode::ExportFeatureNotTransferred
                    .note(format!("parameter {} has an omitted owner", parameter.id)),
            );
            continue;
        };
        let name = object.name.clone();
        // Dynamic metadata distinguishes authored scalar parameters from
        // application controls. A literal restates its value lane, so only a
        // formula needs an explicit binding.
        let property = if let Some(source) = parameter.native_ref.as_ref().and_then(|id| {
            native_properties
                .iter()
                .find(|source| &source.id == id && source.name == property_name)
        }) {
            let generated = roxmltree::Document::parse(&property).map_err(CodecError::malformed)?;
            let generated_root = generated.root_element();
            let generated_value = generated_root.first_element_child();
            let mut source = source.clone();
            let compatible = source.type_name == generated_root.attribute("type").unwrap_or("");
            let mut restored = None;
            if let (
                true,
                Some(generated_value),
                Some(crate::native::PropertyBody::Persisted { values, .. }),
            ) = (compatible, generated_value, &mut source.body)
            {
                if let [value] = values.as_mut_slice() {
                    if value.tag == generated_value.tag_name().name() {
                        if let Some(text) = generated_value.attribute("value") {
                            value.attributes.insert("value".into(), text.to_owned());
                            restored = xml::native_named(
                                &source,
                                names,
                                &std::collections::HashSet::new(),
                            )?;
                        }
                    }
                }
            }
            restored.unwrap_or(property)
        } else {
            property.replacen("<Property ", "<Property group=\"Parameters\" ", 1)
        };
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
        if is_literal_binding(parameter) {
            report_literal_normalization(parameter, losses);
        } else if let Some(expression) = expression {
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
    fn literal_spelling_writes_a_plain_property_with_the_expression_value() {
        let owner = cadmpeg_ir::features::FeatureId::mint("cadir:model:feature#owner").unwrap();
        let real = ParameterValue::Real(cadmpeg_ir::scalar::FiniteReal::new(2.0).unwrap());
        let length = ParameterValue::Length(cadmpeg_ir::scalar::Length::new(20.0).unwrap());
        for (expression, cached, expected, has_engine, normalized) in [
            ("2", real.clone(), "2", false, false),
            ("2.00", real.clone(), "2", false, true),
            ("3.00", real.clone(), "3", false, true),
            ("2 + 1", real, "2", true, false),
            ("20 mm", length.clone(), "20", false, false),
            ("2.00 cm", length.clone(), "20", false, true),
            ("3.00 cm", length, "30", false, true),
        ] {
            let mut parameter = cell_parameter(Expression::text(expression));
            parameter.owner = Some(owner.clone());
            parameter.name = "Width".into();
            parameter.value = Some(cached);
            let mut ir = cadmpeg_ir::CadIr::empty();
            ir.model.parameters.push(parameter);
            let mut objects = vec![Object::new("Owner".into(), "App::FeaturePython", "Owner")];
            let names = HashMap::from([(owner.as_str().to_owned(), "Owner".to_owned())]);
            let mut losses = Vec::new();
            write(&ir, &mut objects, &names, &mut losses).unwrap();
            assert_eq!(
                losses.iter().any(|loss| loss.code
                    == FreecadLossCode::ParameterExpressionNormalized.note("").code),
                normalized
            );
            assert_eq!(
                objects[0].properties.contains_key("ExpressionEngine"),
                has_engine
            );
            let xml = &objects[0].properties["Width"];
            let document = roxmltree::Document::parse(xml).unwrap();
            let value = document
                .descendants()
                .find(|node| node.has_tag_name("Float"))
                .unwrap();
            assert_eq!(value.attribute("value"), Some(expected));
        }
    }

    #[test]
    fn existing_sheet_allocates_distinct_valid_aliases() {
        use cadmpeg_ir::features::FeatureId;
        use cadmpeg_ir::spreadsheets::{CellAddress, Spreadsheet, SpreadsheetCell, SpreadsheetId};
        let mut ir = cadmpeg_ir::CadIr::empty();
        let mut first = cell_parameter(Expression { tokens: Vec::new() });
        first.id = ParameterId::mint("cadir:model:parameter#first").unwrap();
        first.name = "Width".into();
        let mut second = first.clone();
        second.id = ParameterId::mint("cadir:model:parameter#second").unwrap();
        let mut third = first.clone();
        third.id = ParameterId::mint("cadir:model:parameter#third").unwrap();
        third.name = "B4".into();
        let feature = FeatureId::mint("cadir:model:feature#sheet").unwrap();
        let cells = [&first, &second, &third]
            .into_iter()
            .zip(1..=3)
            .map(|(parameter, row)| SpreadsheetCell {
                address: CellAddress::new(row, 1).unwrap(),
                parameter: parameter.id.clone(),
            })
            .collect();
        ir.model.spreadsheets.push(
            Spreadsheet::new(
                SpreadsheetId::mint("cadir:model:spreadsheet#sheet").unwrap(),
                feature.clone(),
                cells,
                Vec::new(),
                Vec::new(),
                Vec::new(),
                None,
            )
            .unwrap(),
        );
        ir.model.parameters.extend([first, second, third]);
        let mut objects = vec![Object::new("Sheet".into(), "Spreadsheet::Sheet", "Sheet")];
        let mut names = HashMap::from([(feature.as_str().to_owned(), "Sheet".to_owned())]);
        let mut losses = Vec::new();
        prepare(&ir, &mut objects, &mut names, &mut losses).unwrap();
        super::super::design::spreadsheets(&ir, &mut objects, &mut names, &mut losses).unwrap();
        let xml = objects[0].properties.get("cells").unwrap();
        let document = roxmltree::Document::parse(xml).unwrap();
        let cells: Vec<_> = document
            .descendants()
            .filter(|node| node.has_tag_name("Cell"))
            .collect();
        assert_eq!(cells[0].attribute("alias"), Some("Width"));
        assert_eq!(cells[1].attribute("alias"), None);
        assert_eq!(cells[2].attribute("alias"), None);
        assert_eq!(names["parameter-path:cadir:model:parameter#second"], "A2");
        assert_eq!(names["parameter-path:cadir:model:parameter#third"], "A3");
        assert_eq!(losses.len(), 2);
    }

    #[test]
    fn sheet_without_emitted_feature_keeps_cell_as_parameter_owner() {
        use cadmpeg_ir::features::FeatureId;
        use cadmpeg_ir::spreadsheets::{CellAddress, Spreadsheet, SpreadsheetCell, SpreadsheetId};

        let mut ir = cadmpeg_ir::CadIr::empty();
        let parameter = cell_parameter(Expression::literal(&ParameterValue::Real(
            cadmpeg_ir::scalar::FiniteReal::new(3.0).unwrap(),
        )));
        let feature = FeatureId::mint("cadir:model:feature#sheet").unwrap();
        ir.model.spreadsheets.push(
            Spreadsheet::new(
                SpreadsheetId::mint("cadir:model:spreadsheet#sheet").unwrap(),
                feature.clone(),
                vec![SpreadsheetCell {
                    address: CellAddress::new(1, 1).unwrap(),
                    parameter: parameter.id.clone(),
                }],
                Vec::new(),
                Vec::new(),
                Vec::new(),
                None,
            )
            .unwrap(),
        );
        let id = parameter.id.clone();
        ir.model.parameters.push(parameter);
        let mut objects = Vec::new();
        let mut names = HashMap::new();
        let mut losses = Vec::new();
        prepare(&ir, &mut objects, &mut names, &mut losses).unwrap();
        super::super::design::spreadsheets(&ir, &mut objects, &mut names, &mut losses).unwrap();

        assert_eq!(objects.len(), 1);
        assert_eq!(names[&format!("parameter-owner:{id}")], objects[0].name);
        let document = roxmltree::Document::parse(&objects[0].properties["cells"]).unwrap();
        let cell = document
            .descendants()
            .find(|node| node.has_tag_name("Cell"))
            .unwrap();
        assert_eq!(cell.attribute("address"), Some("A1"));
        assert_eq!(cell.attribute("content"), Some("3"));
    }

    #[test]
    fn generated_cells_resolve_forward_references() {
        let mut ir = cadmpeg_ir::CadIr::empty();
        let mut first = cell_parameter(Expression { tokens: Vec::new() });
        first.id = ParameterId::mint("cadir:model:parameter#first").unwrap();
        first.name = "First".to_owned();
        let mut second = cell_parameter(Expression::literal(&ParameterValue::Real(
            cadmpeg_ir::scalar::FiniteReal::new(3.0).unwrap(),
        )));
        second.id = ParameterId::mint("cadir:model:parameter#second").unwrap();
        second.name = "Second".to_owned();
        second.ordinal = 1;
        first.expression = Expression {
            tokens: vec![
                ExpressionToken::Text("=".to_owned()),
                ExpressionToken::Parameter(second.id.clone()),
            ],
        };
        ir.model.parameters.extend([first, second]);
        let mut objects = Vec::new();
        let mut names = HashMap::new();
        prepare(&ir, &mut objects, &mut names, &mut Vec::new()).unwrap();
        let xml = objects[0].properties.get("cells").unwrap();
        let document = roxmltree::Document::parse(xml).unwrap();
        let cell = document
            .descendants()
            .find(|node| node.has_tag_name("Cell") && node.attribute("address") == Some("A1"))
            .unwrap();
        assert_eq!(cell.attribute("content"), Some("=Second"));
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
