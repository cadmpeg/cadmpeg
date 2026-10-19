// SPDX-License-Identifier: Apache-2.0
//! Live expression links from emitted application controls to canonical parameters.

use super::super::xml::{self, Object};
use crate::control_links::{ControlLink, Measurement, PROPERTY};
use cadmpeg_core::CodecError;
use cadmpeg_ir::{report::loss::LossNote, CadIr};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};

pub(super) fn write(
    ir: &CadIr,
    objects: &mut [Object],
    names: &HashMap<String, String>,
    expressions: &mut BTreeMap<String, Vec<String>>,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let parameters = ir
        .model
        .parameters
        .iter()
        .map(|parameter| (parameter.id.as_str(), parameter))
        .collect::<HashMap<_, _>>();
    for feature in &ir.model.features {
        let Some(name) = names.get(feature.id.as_str()) else {
            continue;
        };
        let Some(object) = objects.iter_mut().find(|object| object.name == *name) else {
            continue;
        };
        let controls = bindings(feature)?;
        let mut rows = Vec::new();
        for (control, bindings) in controls {
            let Some((id, measurement)) = bindings.first() else {
                continue;
            };
            if bindings
                .iter()
                .any(|binding| binding != &(id.clone(), *measurement))
            {
                losses.push(crate::loss::FreecadLossCode::ExportFeatureNotTransferred.note(format!(
                    "feature {} has distinct authored operands sharing native control {control}", feature.id
                )));
                continue;
            }
            if !object.properties.contains_key(&control) {
                losses.push(
                    crate::loss::FreecadLossCode::ExportFeatureNotTransferred.note(format!(
                        "feature {} bound operand has no emitted native control {control}",
                        feature.id
                    )),
                );
                continue;
            }
            let Some(parameter) = parameters.get(id.as_str()).copied() else {
                continue;
            };
            let owner = parameter
                .owner
                .as_ref()
                .and_then(|owner| names.get(owner.as_str()))
                .or_else(|| names.get(&format!("parameter-owner:{}", parameter.id)));
            if *measurement == Measurement::Direct
                && names
                    .get(&format!("parameter-path:{}", parameter.id))
                    .map(String::as_str)
                    .unwrap_or(&parameter.name)
                    == control.as_str()
                && owner == Some(name)
            {
                continue;
            }
            let Some(reference) = super::parameter_reference(ir, &parameter.id, Some(name), names)
            else {
                losses.push(
                    crate::loss::FreecadLossCode::ExportFeatureNotTransferred.note(format!(
                        "feature {} control {control} has an unencodable parameter reference",
                        feature.id
                    )),
                );
                continue;
            };
            let expression = match measurement {
                Measurement::Direct => reference,
                Measurement::Diameter => format!("{reference} / 2"),
            };
            expressions
                .entry(name.clone())
                .or_default()
                .push(xml::element(
                    "Expression",
                    &[("path", control.clone()), ("expression", expression)],
                ));
            rows.push(ControlLink {
                control: control.clone(),
                measurement: *measurement,
            });
        }
        if rows.is_empty() {
            object.properties.remove(PROPERTY);
        } else {
            let values = rows
                .iter()
                .map(|row| {
                    serde_json::to_string(row)
                        .map(|value| xml::element("String", &[("value", value)]))
                })
                .collect::<Result<Vec<_>, _>>()
                .map_err(CodecError::malformed)?;
            let property = xml::property(
                PROPERTY,
                "App::PropertyStringList",
                &format!(
                    "<StringList count=\"{}\">{}</StringList>",
                    values.len(),
                    values.join("")
                ),
            )
            .replacen("<Property ", "<Property group=\"CADIR\" ", 1);
            object.set(PROPERTY, property);
        }
    }
    Ok(())
}

fn collect(
    value: &Value,
    operation: &str,
    path: &mut Vec<String>,
    diameter: bool,
    controls: &mut BTreeMap<String, Vec<(String, Measurement)>>,
) {
    if value["kind"] == "parameter" {
        if let (Some(control), Some(id)) = (
            if operation == "native" && path.first().is_some_and(|part| part == "parameters") {
                path.last().cloned()
            } else {
                crate::design::dimensions::target_control(operation, path).map(str::to_owned)
            },
            value["value"].as_str(),
        ) {
            controls.entry(control).or_default().push((
                id.to_owned(),
                if diameter {
                    Measurement::Diameter
                } else {
                    Measurement::Direct
                },
            ));
        }
        return;
    }
    match value {
        Value::Object(object) => {
            let diameter = diameter || value["kind"] == "diameter";
            let form = value["kind"]
                .as_str()
                .filter(|kind| matches!(*kind, "span" | "spacing"))
                .map(|kind| format!("@{kind}"));
            if let Some(form) = &form {
                path.push(form.clone());
            }
            for (key, value) in object {
                if key != "tokens" {
                    path.push(key.clone());
                    collect(value, operation, path, diameter, controls);
                    path.pop();
                }
            }
            if form.is_some() {
                path.pop();
            }
        }
        Value::Array(array) => {
            for (index, value) in array.iter().enumerate() {
                path.push(index.to_string());
                collect(value, operation, path, diameter, controls);
                path.pop();
            }
        }
        _ => {}
    }
}

pub(super) fn bindings(
    feature: &cadmpeg_ir::features::Feature,
) -> Result<BTreeMap<String, Vec<(String, Measurement)>>, CodecError> {
    let mut definition =
        serde_json::to_value(feature.definition()).map_err(CodecError::malformed)?;
    let mut controls = BTreeMap::new();
    if definition["definition"] == "post_process" {
        if definition["fuzzy_tolerance"]["kind"] == "explicit" {
            collect(
                &definition["fuzzy_tolerance"],
                "post_process",
                &mut vec!["fuzzy_tolerance".into()],
                false,
                &mut controls,
            );
        }
        definition = definition["operation"].take();
    }
    let operation = definition["definition"].as_str().unwrap_or_default();
    collect(
        &definition,
        operation,
        &mut Vec::new(),
        false,
        &mut controls,
    );
    Ok(controls)
}
