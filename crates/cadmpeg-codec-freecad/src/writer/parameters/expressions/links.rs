// SPDX-License-Identifier: Apache-2.0
//! Keep link metadata only through proven current typed target associations.

use super::*;
use crate::native::PropertyRecord;
use cadmpeg_ir::attributes::{AttributeValue, SourceAttribute};
use std::collections::BTreeSet;

pub(super) fn write(
    original: Option<&PropertyRecord>,
    attribute: &SourceAttribute,
    links: &[String],
) -> Option<(String, bool)> {
    let Some(original) = original else {
        return Some((
            format!(
                "<XLinks count=\"{}\">{}</XLinks>",
                links.len(),
                links.concat()
            ),
            false,
        ));
    };
    let Some(container) = original.values().iter().find(|value| value.tag == "XLinks") else {
        return Some((
            format!(
                "<XLinks count=\"{}\">{}</XLinks>",
                links.len(),
                links.concat()
            ),
            false,
        ));
    };
    let current = attribute
        .values
        .iter()
        .filter(|value| {
            matches!(
                value,
                AttributeValue::Reference(_)
                    | AttributeValue::SubReference { .. }
                    | AttributeValue::ExternalReference { .. }
            )
        })
        .collect::<Vec<_>>();
    let mut mapped = BTreeMap::new();
    let mut used = BTreeSet::new();
    for (index, target) in original.links().iter().enumerate() {
        let Some(target) = target else {
            continue;
        };
        let matches = |value: &&AttributeValue| match value {
            AttributeValue::Reference(object) => {
                target.document().is_none()
                    && target.object() == Some(object.as_str())
                    && target.subelements().is_empty()
            }
            AttributeValue::SubReference {
                object,
                subelements,
            } => {
                target.document().is_none()
                    && target.object() == Some(object.as_str())
                    && target.subelements() == subelements
            }
            AttributeValue::ExternalReference {
                document,
                object,
                subelements,
            } => {
                target
                    .document()
                    .map(crate::native::ExternalDocument::as_str)
                    == Some(document.as_str())
                    && target.object() == Some(object.as_str())
                    && target.subelements() == subelements
            }
            _ => false,
        };
        let selected = if current.get(index).is_some_and(matches) && !used.contains(&index) {
            Some(index)
        } else {
            let candidates = current
                .iter()
                .enumerate()
                .filter(|(index, value)| !used.contains(index) && matches(value))
                .map(|(index, _)| index)
                .collect::<Vec<_>>();
            match candidates.as_slice() {
                [index] => Some(*index),
                _ => None,
            }
        };
        if let Some(selected) = selected {
            used.insert(selected);
            mapped.insert(index, selected);
        }
    }
    let mut attributes = container.attributes.clone();
    attributes.insert("count".into(), links.len().to_string());
    let mut lost = false;
    let mut maps = Vec::new();
    for map in original
        .values()
        .iter()
        .filter(|value| value.parent == Some(container.order) && value.tag == "DocMap")
    {
        let index = map.attributes.get("index")?.parse::<usize>().ok()?;
        let Some(current) = mapped.get(&index) else {
            lost = true;
            continue;
        };
        let mut attributes = map.attributes.clone();
        attributes.insert("index".into(), current.to_string());
        maps.push(element("DocMap", &attributes));
    }
    if maps.is_empty() {
        attributes.remove("docs");
    } else {
        attributes.insert("docs".into(), maps.len().to_string());
    }
    if let Some(hidden) = container.attributes.get("hidden") {
        let mut retained = Vec::new();
        for index in hidden.split_whitespace() {
            match mapped.get(&index.parse::<usize>().ok()?) {
                Some(current) => retained.push(current.to_string()),
                None => lost = true,
            }
        }
        if retained.is_empty() {
            attributes.remove("hidden");
        } else {
            attributes.insert("hidden".into(), retained.join(" "));
        }
    }
    let opening = element("XLinks", &attributes);
    Some((
        format!(
            "{}>{}{}</XLinks>",
            opening.strip_suffix("/>")?,
            maps.concat(),
            links.concat()
        ),
        lost,
    ))
}

fn element(tag: &str, attributes: &BTreeMap<String, String>) -> String {
    xml::element(
        tag,
        &attributes
            .iter()
            .map(|(key, value)| (key.as_str(), value.clone()))
            .collect::<Vec<_>>(),
    )
}
