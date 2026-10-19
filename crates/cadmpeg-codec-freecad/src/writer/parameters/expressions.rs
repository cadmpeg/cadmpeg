// SPDX-License-Identifier: Apache-2.0
//! Native-only expression lanes coexist with regenerated neutral controls.

use super::super::xml::{self, Object};
use cadmpeg_core::CodecError;
use cadmpeg_ir::{features::FeatureOperation, CadIr};
use std::collections::{BTreeMap, HashMap};

pub(super) fn native_only(
    ir: &CadIr,
    names: &HashMap<String, String>,
) -> Result<BTreeMap<String, Vec<String>>, CodecError> {
    let Some(namespace) = ir.native.namespace("fcstd") else {
        return Ok(BTreeMap::new());
    };
    let properties = namespace.arena_as::<crate::native::PropertyRecord>("properties")?;
    let mut expressions = BTreeMap::<String, Vec<String>>::new();
    if !ir.scope.design {
        return Ok(expressions);
    }
    for property in properties
        .iter()
        .filter(|property| property.name == "ExpressionEngine")
    {
        let Some(owner) = names.get(&property.owner) else {
            continue;
        };
        let feature = ir
            .model
            .features
            .iter()
            .find(|feature| feature.native_ref.as_ref() == Some(&property.owner));
        let neutral_controls = feature
            .map(super::controls::bindings)
            .transpose()?
            .unwrap_or_default();
        // Insert an empty lane too: removal of the last projected expression
        // must not leave a stale source engine active on a Native feature.
        expressions.entry(owner.clone()).or_default();
        for value in property
            .values()
            .iter()
            .filter(|value| value.tag == "Expression")
        {
            let (Some(path), Some(expression)) = (
                value.attributes.get("path"),
                value.attributes.get("expression"),
            ) else {
                continue;
            };
            let native_operand = feature.is_some_and(|feature| matches!(feature.definition().operation(), FeatureOperation::Native { parameters, .. } if parameters.keys().any(|name| name.as_str() == path)));
            let parameter = ir.model.parameters.iter().any(|parameter| {
                parameter.native_ref.as_ref().is_some_and(|id| {
                    properties.iter().any(|source| {
                        source.id == *id && source.owner == property.owner && source.name == *path
                    })
                })
            });
            let constraints = path.starts_with("Constraints[")
                && ir
                    .model
                    .sketches
                    .iter()
                    .any(|sketch| sketch.native_ref.as_ref() == Some(&property.owner));
            if native_operand
                || parameter
                || constraints
                || neutral_controls.contains_key(path)
                || (feature.is_some() && crate::property_policy::has_neutral_form(path))
            {
                continue;
            }
            expressions
                .entry(owner.clone())
                .or_default()
                .push(xml::element(
                    "Expression",
                    &[("path", path.clone()), ("expression", expression.clone())],
                ));
        }
    }
    Ok(expressions)
}

pub(super) fn write(
    objects: &mut [Object],
    expressions: BTreeMap<String, Vec<String>>,
) -> Result<(), CodecError> {
    for (name, values) in expressions {
        let Some(object) = objects.iter_mut().find(|object| object.name == name) else {
            continue;
        };
        let mut paths = BTreeMap::new();
        for value in &values {
            let document = roxmltree::Document::parse(value).map_err(CodecError::malformed)?;
            let path = document
                .root_element()
                .attribute("path")
                .ok_or_else(|| CodecError::malformed("generated expression lacks a path"))?;
            paths.insert(path.to_owned(), value.as_str());
        }
        if paths.is_empty() {
            object.properties.remove("ExpressionEngine");
        } else {
            object.set(
                "ExpressionEngine",
                xml::property(
                    "ExpressionEngine",
                    "App::PropertyExpressionEngine",
                    &format!(
                        "<ExpressionEngine count=\"{}\">{}</ExpressionEngine>",
                        paths.len(),
                        paths.values().copied().collect::<String>()
                    ),
                ),
            );
        }
    }
    Ok(())
}
