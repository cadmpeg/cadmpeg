// SPDX-License-Identifier: Apache-2.0
//! Typed link carriers keep the persistence grammar and native target identity.

use crate::native::{LinkTarget, LinkTargetWire, ValueRecord};
use crate::persistence::LinkGrammar;
use cadmpeg_core::CodecError;
use std::collections::BTreeMap;

pub(crate) fn expression_container(kind: &str) -> bool {
    matches!(
        kind,
        "Spreadsheet::PropertySheet" | "App::PropertyExpressionEngine"
    )
}

pub(crate) fn local_carrier(kind: &str, tag: &str) -> bool {
    crate::persistence::link_grammar(kind).is_some()
        || expression_container(kind) && matches!(tag, "XLink" | "Sub")
}

/// Original direct reference-slot ordinal, independent of descendant preorder slots.
pub(crate) fn source_index(
    kind: &str,
    source: &crate::xml_framing::XmlSource<'_>,
    index: usize,
) -> Result<Option<usize>, CodecError> {
    if expression_container(kind) {
        return super::spreadsheet::link_index(source, index);
    }
    let Some(grammar) = crate::persistence::link_grammar(kind) else {
        return Ok(None);
    };
    let tag = source.name(index)?;
    let Some(parent) = source.elements()[index].parent() else {
        return Ok(None);
    };
    let list_carrier = match grammar {
        LinkGrammar::LinkList | LinkGrammar::LinkSubList => tag == "Link",
        LinkGrammar::XLinkSubList => tag == "XLink",
        _ => false,
    };
    if !list_carrier || source.name(parent)? != grammar.root_tag() {
        return Ok(None);
    }
    Ok(source.children(parent).position(|child| child == index))
}

fn grammar(kind: &str, tag: &str) -> Option<LinkGrammar> {
    crate::persistence::link_grammar(kind).or_else(|| {
        (expression_container(kind) && matches!(tag, "XLink" | "Sub")).then_some(LinkGrammar::XLink)
    })
}

fn required<'a>(
    attributes: &'a BTreeMap<String, String>,
    key: &str,
) -> Result<&'a str, CodecError> {
    attributes
        .get(key)
        .map(String::as_str)
        .ok_or_else(|| CodecError::malformed(format_args!("link has no {key} attribute")))
}

fn subelement(value: &ValueRecord, key: &str) -> Result<String, CodecError> {
    let primary = required(&value.attributes, key)?;
    Ok(value
        .attributes
        .get("shadowed")
        .map_or(primary, String::as_str)
        .to_owned())
}

pub(crate) fn validate(
    type_name: &str,
    tag: &str,
    attributes: &BTreeMap<String, String>,
    children: usize,
) -> Result<(), CodecError> {
    let Some(grammar) = grammar(type_name, tag) else {
        return Ok(());
    };
    let (allowed, required_fields, counted) = match (grammar, tag) {
        (LinkGrammar::Link, "Link") | (LinkGrammar::LinkList, "Link") => {
            (&["value"][..], &["value"][..], false)
        }
        (LinkGrammar::LinkSubList, "Link") => (&["obj", "sub"][..], &["obj", "sub"][..], false),
        (LinkGrammar::LinkSub | LinkGrammar::XLink | LinkGrammar::XLinkSubList, "Sub") => {
            (&["value"][..], &["value"][..], false)
        }
        (LinkGrammar::LinkSub, "LinkSub") => (&["value"][..], &["value"][..], true),
        (LinkGrammar::LinkList, "LinkList")
        | (LinkGrammar::LinkSubList, "LinkSubList")
        | (LinkGrammar::XLinkSubList, "XLinkSubList") => (&[][..], &[][..], true),
        (LinkGrammar::XLink | LinkGrammar::XLinkSubList, "XLink") => {
            required(attributes, "name")?;
            reject_aliases(attributes, &["name", "file", "sub"])?;
            match (attributes.get("sub"), attributes.get("count")) {
                (Some(_), None) if children == 0 => {}
                (None, Some(count))
                    if count.parse::<usize>().ok() == Some(children) && children != 0 => {}
                (None, None) if children == 0 => {}
                _ => {
                    return Err(CodecError::malformed(
                        "XLink has conflicting or invalid subelement carriers",
                    ))
                }
            }
            return Ok(());
        }
        _ => {
            return Err(CodecError::malformed(format_args!(
                "{type_name} has unexpected {tag} element"
            )))
        }
    };
    reject_aliases(attributes, allowed)?;
    for key in required_fields {
        required(attributes, key)?;
    }
    if counted {
        if required(attributes, "count")?.parse::<usize>().ok() != Some(children) {
            return Err(CodecError::malformed(
                "link count differs from framed child count",
            ));
        }
    } else if children != 0 {
        return Err(CodecError::malformed(
            "link carrier contains nested element values",
        ));
    }
    Ok(())
}

/// Checks the actual local declaration namespace inside this link's own callback.
/// External-document targets are classified by the same typed wire grammar.
pub(crate) fn validate_local_target(
    type_name: &str,
    tag: &str,
    attributes: &BTreeMap<String, String>,
    declared: impl Fn(&str) -> bool,
) -> Result<(), CodecError> {
    if let Some(object) = local_target_object(type_name, tag, attributes)? {
        if !declared(&object) {
            return Err(CodecError::malformed(format_args!(
                "link target object {object} was never declared"
            )));
        }
    }
    Ok(())
}

/// Returns only local object references, using the typed external-document grammar.
pub(crate) fn local_target_object(
    type_name: &str,
    tag: &str,
    attributes: &BTreeMap<String, String>,
) -> Result<Option<String>, CodecError> {
    let Some(grammar) = grammar(type_name, tag) else {
        return Ok(None);
    };
    let (key, external) = match (grammar, tag) {
        (LinkGrammar::Link | LinkGrammar::LinkList, "Link") => ("value", false),
        (LinkGrammar::LinkSub, "LinkSub") => ("value", false),
        (LinkGrammar::LinkSubList, "Link") => ("obj", false),
        (LinkGrammar::XLink | LinkGrammar::XLinkSubList, "XLink") => ("name", true),
        _ => return Ok(None),
    };
    let document = external
        .then(|| attributes.get("file"))
        .flatten()
        .filter(|file| !file.is_empty())
        .cloned();
    let target = LinkTarget::optional_from_wire(LinkTargetWire {
        document,
        document_attribute: external.then(|| "file".into()),
        object: attributes.get(key).cloned(),
        subelements: Vec::new(),
    })
    .map_err(CodecError::malformed)?;
    Ok(target
        .filter(|target| target.document().is_none())
        .and_then(|target| target.object().map(str::to_owned)))
}

fn reject_aliases(
    attributes: &BTreeMap<String, String>,
    allowed: &[&str],
) -> Result<(), CodecError> {
    const CARRIERS: &[&str] = &[
        "value", "Value", "object", "Object", "obj", "Obj", "name", "Name", "document", "Document",
        "doc", "Doc", "file", "File", "sub", "Sub",
    ];
    if let Some(key) = attributes
        .keys()
        .find(|key| CARRIERS.contains(&key.as_str()) && !allowed.contains(&key.as_str()))
    {
        return Err(CodecError::malformed(format_args!(
            "link uses ambiguous {key} carrier"
        )));
    }
    Ok(())
}

fn target(
    value: &ValueRecord,
    object_key: &str,
    subelements: Vec<String>,
    external: bool,
) -> Result<Option<LinkTarget>, CodecError> {
    let object = required(&value.attributes, object_key)?;
    let document = external
        .then(|| value.attributes.get("file"))
        .flatten()
        .filter(|file| !file.is_empty());
    LinkTarget::optional_from_wire(LinkTargetWire {
        document: document.cloned(),
        document_attribute: external.then(|| "file".to_owned()),
        object: Some(if document.is_none() && !object.is_empty() {
            crate::native::native_id("object", object)
        } else {
            object.to_owned()
        }),
        subelements,
    })
    .map_err(CodecError::malformed)
}

fn xlink(value: &ValueRecord, children: &[&ValueRecord]) -> Result<Option<LinkTarget>, CodecError> {
    let subs = if value.attributes.contains_key("sub") {
        vec![subelement(value, "sub")?]
    } else {
        children
            .iter()
            .map(|value| subelement(value, "value"))
            .collect::<Result<_, _>>()?
    };
    if value
        .attributes
        .get("count")
        .and_then(|text| text.parse::<usize>().ok())
        .is_some_and(|count| count != children.len())
    {
        return Ok(None);
    }
    target(value, "name", subs, true)
}

pub(crate) fn from_values(
    type_name: &str,
    values: &[ValueRecord],
    source: &crate::xml_framing::XmlSource<'_>,
    index: usize,
    ranges: &[std::ops::Range<usize>],
) -> Result<Vec<Option<LinkTarget>>, CodecError> {
    if expression_container(type_name) {
        let mut targets = Vec::new();
        for container in source
            .children(index)
            .filter(|child| source.name(*child).is_ok_and(|name| name == "XLinks"))
        {
            for child in source
                .children(container)
                .filter(|child| source.name(*child).is_ok_and(|name| name == "XLink"))
            {
                let extent = source.extent(child)?;
                let held = ranges
                    .iter()
                    .position(|range| range.start == extent.start() && range.end == extent.end())
                    .and_then(|position| values.get(position));
                targets.push(held.map_or(Ok(None), |value| {
                    let children = values
                        .iter()
                        .filter(|child| child.parent == Some(value.order))
                        .collect::<Vec<_>>();
                    xlink(value, &children)
                })?);
            }
        }
        return Ok(targets);
    }
    let Some(grammar) = crate::persistence::link_grammar(type_name) else {
        return Ok(Vec::new());
    };
    let root = values
        .first()
        .ok_or_else(|| CodecError::malformed("link root is absent"))?;
    if root.tag != grammar.root_tag() {
        return Err(CodecError::malformed("link root has a different grammar"));
    }
    let children: Vec<_> = source
        .children(index)
        .map(|child| {
            let extent = source.extent(child)?;
            Ok(ranges
                .iter()
                .position(|range| range.start == extent.start() && range.end == extent.end())
                .and_then(|position| values.get(position)))
        })
        .collect::<Result<_, CodecError>>()?;
    let held: Vec<_> = children.iter().copied().flatten().collect();
    match grammar {
        LinkGrammar::Link => Ok(vec![target(root, "value", Vec::new(), false)?]),
        LinkGrammar::LinkList => children
            .iter()
            .map(|value| value.map_or(Ok(None), |value| target(value, "value", Vec::new(), false)))
            .collect(),
        LinkGrammar::LinkSub => {
            if held.len() != children.len() {
                return Ok(vec![None]);
            }
            Ok(vec![target(
                root,
                "value",
                held.iter()
                    .map(|value| subelement(value, "value"))
                    .collect::<Result<_, _>>()?,
                false,
            )?])
        }
        LinkGrammar::LinkSubList => children
            .iter()
            .map(|value| {
                value.map_or(Ok(None), |value| {
                    target(value, "obj", vec![subelement(value, "sub")?], false)
                })
            })
            .collect(),
        LinkGrammar::XLink => Ok(vec![xlink(root, &held)?]),
        LinkGrammar::XLinkSubList => children
            .iter()
            .map(|value| {
                value.map_or(Ok(None), |value| {
                    let subs: Vec<_> = values
                        .iter()
                        .filter(|sub| sub.parent == Some(value.order))
                        .collect();
                    xlink(value, &subs)
                })
            })
            .collect(),
    }
}
