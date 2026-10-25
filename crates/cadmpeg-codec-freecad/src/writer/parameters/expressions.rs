// SPDX-License-Identifier: Apache-2.0
//! Native-only expression lanes coexist with regenerated neutral controls.

mod links;

use super::super::xml::{self, Object};
use cadmpeg_core::CodecError;
use cadmpeg_ir::CadIr;
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
    for property in properties.iter().filter(|property| {
        property.name == "ExpressionEngine" && property.type_name == "App::PropertyExpressionEngine"
    }) {
        let Some(owner) = names.get(&property.owner) else {
            continue;
        };
        let neutral_lane = crate::attributes::projection_held(ir)
            && crate::attributes::target(ir, &property.owner)?.is_some()
            && crate::attributes::values(property).is_some();
        let bound = |path: &str| {
            ir.model.parameters.iter().any(|parameter| {
                parameter.native_ref.as_ref().is_some_and(|id| {
                    properties.iter().any(|source| {
                        &source.id == id && source.owner == property.owner && source.name == path
                    })
                })
            })
        };
        let has_bound = property
            .values()
            .iter()
            .filter_map(|value| value.attributes.get("path"))
            .any(|path| bound(path));
        if !neutral_lane && !has_bound {
            continue;
        }
        let lane = expressions.entry(owner.clone()).or_default();
        if neutral_lane {
            continue;
        }
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
            if bound(path) {
                continue;
            }
            let mut attributes = vec![("path", path.clone()), ("expression", expression.clone())];
            if let Some(comment) = value.attributes.get("comment") {
                attributes.push(("comment", comment.clone()));
            }
            lane.push(xml::element("Expression", &attributes));
        }
        if let Some(xml) = xml::native_named(property, names)? {
            let parsed = roxmltree::Document::parse(&xml).map_err(CodecError::malformed)?;
            for node in parsed
                .descendants()
                .filter(|node| node.has_tag_name("XLinks"))
            {
                lane.push(xml[node.range()].to_owned());
            }
        }
    }
    Ok(expressions)
}

pub(super) fn write(
    ir: &CadIr,
    names: &HashMap<String, String>,
    objects: &mut [Object],
    expressions: BTreeMap<String, Vec<String>>,
) -> Result<(), CodecError> {
    let properties = ir
        .native
        .namespace("fcstd")
        .map(|namespace| namespace.arena_as::<crate::native::PropertyRecord>("properties"))
        .transpose()?
        .unwrap_or_default();
    for (name, values) in expressions {
        let Some(object) = objects.iter_mut().find(|object| object.name == name) else {
            continue;
        };
        if values.is_empty() {
            object.properties.remove("ExpressionEngine");
            continue;
        }
        let count = values
            .iter()
            .filter(|value| value.starts_with("<Expression "))
            .count();
        let body = format!(
            "<ExpressionEngine count=\"{count}\">{}</ExpressionEngine>",
            values.concat()
        );
        let generated = xml::property("ExpressionEngine", "App::PropertyExpressionEngine", &body);
        let original = properties.iter().find(|property| {
            property.name == "ExpressionEngine"
                && property.type_name == "App::PropertyExpressionEngine"
                && names.get(&property.owner) == Some(&name)
        });
        let encoded = if let Some(original) = original {
            if let Some(attribute) = ir
                .model
                .attributes
                .iter()
                .find(|attribute| attribute.native_ref.as_deref() == Some(original.id.as_str()))
            {
                super::super::attributes::structured::encode_generated(
                    attribute, original, names, &generated,
                )?
                .unwrap_or(generated)
            } else if let Some(original_xml) = xml::native_named(original, names)? {
                let opening_end = original_xml.find('>').ok_or_else(|| {
                    CodecError::malformed("current native engine has no property opening")
                })?;
                format!("{}{body}</Property>", &original_xml[..=opening_end])
            } else {
                generated
            }
        } else {
            generated
        };
        object.set("ExpressionEngine", encoded);
    }
    Ok(())
}

pub(super) fn append_attributes(
    ir: &CadIr,
    names: &HashMap<String, String>,
    expressions: &mut BTreeMap<String, Vec<String>>,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<(), CodecError> {
    let properties = ir
        .native
        .namespace("fcstd")
        .map(|namespace| namespace.arena_as::<crate::native::PropertyRecord>("properties"))
        .transpose()?
        .unwrap_or_default();
    for attribute in &ir.model.attributes {
        let original = properties
            .iter()
            .find(|property| attribute.native_ref.as_deref() == Some(property.id.as_str()));
        let expression_lane = original
            .is_some_and(|property| property.type_name == "App::PropertyExpressionEngine")
            || attribute.values.iter().any(|value| {
                matches!(value, cadmpeg_ir::attributes::AttributeValue::Expression(_))
            });
        if !expression_lane
            || attribute.name != "ExpressionEngine"
            || attribute.target == cadmpeg_ir::attributes::AttributeTarget::Document
        {
            continue;
        }
        let owner = original
            .and_then(|property| names.get(&property.owner))
            .or_else(|| super::super::attributes::target(ir, &attribute.target, names));
        let Some(owner) = owner else {
            losses.push(
                crate::loss::FreecadLossCode::ExportAttributeNotTransferred.note(format!(
                    "expression attribute {} has no emitted owner",
                    attribute.id
                )),
            );
            continue;
        };
        if let Some(values) = content(ir, attribute, names, Some(owner), original, losses) {
            expressions.entry(owner.clone()).or_default().extend(values);
        } else {
            losses.push(
                crate::loss::FreecadLossCode::ExportAttributeNotTransferred.note(format!(
                    "expression attribute {} has an unencodable current reference",
                    attribute.id
                )),
            );
        }
    }
    Ok(())
}

pub(in crate::writer) fn property(
    ir: &CadIr,
    attribute: &cadmpeg_ir::attributes::SourceAttribute,
    names: &HashMap<String, String>,
    owner: Option<&str>,
    original: Option<&crate::native::PropertyRecord>,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<Option<String>, CodecError> {
    let Some(values) = content(ir, attribute, names, owner, original, losses) else {
        return Ok(None);
    };
    let count = values
        .iter()
        .filter(|value| value.starts_with("<Expression "))
        .count();
    let generated = xml::property(
        &attribute.name,
        "App::PropertyExpressionEngine",
        &format!(
            "<ExpressionEngine count=\"{count}\">{}</ExpressionEngine>",
            values.concat()
        ),
    );
    match original {
        Some(original) => super::super::attributes::structured::encode_generated(
            attribute, original, names, &generated,
        ),
        None => Ok(Some(generated)),
    }
}

fn content(
    ir: &CadIr,
    attribute: &cadmpeg_ir::attributes::SourceAttribute,
    names: &HashMap<String, String>,
    owner: Option<&str>,
    original: Option<&crate::native::PropertyRecord>,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Option<Vec<String>> {
    let mut values = Vec::new();
    let mut links = Vec::new();
    for value in &attribute.values {
        match value {
            cadmpeg_ir::attributes::AttributeValue::Expression(value) => {
                let text = value
                    .expression
                    .render_with(|id| super::parameter_reference(ir, id, owner, names))?;
                let mut attributes = vec![("path", value.path.clone()), ("expression", text)];
                if let Some(comment) = &value.comment {
                    attributes.push(("comment", comment.clone()));
                }
                values.push(xml::element("Expression", &attributes));
            }
            value => links.push(super::super::attributes::structured::xlink(value, names)?),
        }
    }
    if !links.is_empty()
        || original
            .is_some_and(|property| property.values().iter().any(|value| value.tag == "XLinks"))
    {
        let (link_xml, lost) = links::write(original, attribute, &links)?;
        if lost {
            losses.push(crate::loss::FreecadLossCode::ExportNativePropertyNotTransferred.note(format!("expression attribute {}: document-map or hidden-link metadata has no unique current target association and is omitted", attribute.id)));
        }
        values.push(link_xml);
    }
    Some(values)
}
