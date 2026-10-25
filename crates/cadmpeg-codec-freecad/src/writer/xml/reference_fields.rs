// SPDX-License-Identifier: Apache-2.0
//! Output references come from typed targets; source literal attributes stay intact.

use crate::native::{LinkTarget, ValueRecord, ValueSlots};
use crate::persistence::{link_grammar, LinkGrammar};
use cadmpeg_core::CodecError;
use std::collections::{BTreeMap, BTreeSet, HashMap};

fn object_field(kind: &str, tag: &str) -> Option<&'static str> {
    match (link_grammar(kind), tag) {
        (Some(LinkGrammar::Link | LinkGrammar::LinkList), "Link")
        | (Some(LinkGrammar::LinkSub), "LinkSub") => Some("value"),
        (Some(LinkGrammar::LinkSubList), "Link") => Some("obj"),
        (Some(LinkGrammar::XLink | LinkGrammar::XLinkSubList), "XLink") => Some("name"),
        (None, "XLink")
            if crate::persistence::document_properties::value_parts::links::expression_container(
                kind,
            ) => Some("name"),
        _ => None,
    }
}

pub(super) fn reconcile(
    kind: &str,
    values: &ValueSlots,
    links: &[Option<LinkTarget>],
    names: &HashMap<String, String>,
) -> Result<(Vec<ValueRecord>, bool), CodecError> {
    let slots: BTreeMap<_, _> =
        crate::native::document_value::link_carrier_slots(kind, values, links.len())?
            .into_iter()
            .collect();
    let mut omitted = BTreeSet::new();
    let mut result = Vec::new();
    for value in values {
        if value.parent.is_some_and(|parent| omitted.contains(&parent)) {
            omitted.insert(value.order);
            continue;
        }
        let mut current = value.clone();
        if let Some(field) = object_field(kind, &value.tag) {
            let Some(slot) = slots.get(&value.order) else {
                omitted.insert(value.order);
                continue;
            };
            let target = links[*slot].as_ref();
            let object = target.and_then(LinkTarget::object).unwrap_or_default();
            let output = if target.is_some_and(|target| target.document().is_none()) {
                names.get(object).map_or(object, String::as_str)
            } else {
                object
            };
            current.attributes.insert(field.into(), output.into());
            if value.tag == "XLink" {
                let document = target.and_then(LinkTarget::document_name);
                if document.is_some() || current.attributes.contains_key("file") {
                    current
                        .attributes
                        .insert("file".into(), document.unwrap_or_default().into());
                }
            }
        }
        result.push(current);
    }
    Ok((result, !omitted.is_empty()))
}

#[cfg(test)]
mod tests;
