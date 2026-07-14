// SPDX-License-Identifier: Apache-2.0
//! Semantic annotation graph recovery.

use std::collections::HashMap;

use crate::native::{ObjectRecord, PropertyRecord, SemanticAnnotationRecord};

pub(crate) fn transfer(
    objects: &[ObjectRecord],
    properties: &[PropertyRecord],
) -> Vec<SemanticAnnotationRecord> {
    let by_owner = properties.iter().fold(
        HashMap::<&str, Vec<&PropertyRecord>>::new(),
        |mut map, property| {
            map.entry(&property.owner).or_default().push(property);
            map
        },
    );
    objects
        .iter()
        .filter(|object| is_annotation_type(&object.type_name))
        .map(|object| {
            let mut owned = by_owner
                .get(object.id.as_str())
                .cloned()
                .unwrap_or_default();
            owned.sort_by_key(|property| (property.byte_start, property.byte_end));
            let references = owned
                .iter()
                .filter(|property| !property.links.is_empty())
                .map(|property| (property.name.clone(), property.links.clone()))
                .collect();
            let parameters = owned
                .iter()
                .filter(|property| property.links.is_empty())
                .map(|property| (property.name.clone(), property.raw_xml.clone()))
                .collect();
            SemanticAnnotationRecord {
                id: crate::native::native_id("annotation", &object.name),
                object: object.id.clone(),
                kind: object.type_name.clone(),
                text: owned
                    .iter()
                    .filter(|property| is_text_property(&property.name))
                    .flat_map(|property| property.values.iter())
                    .filter_map(text_value)
                    .collect(),
                references,
                parameters,
                side_entries: owned
                    .iter()
                    .flat_map(|property| &property.side_entries)
                    .cloned()
                    .collect(),
            }
        })
        .collect()
}

pub(crate) fn is_annotation_type(type_name: &str) -> bool {
    let leaf = type_name.rsplit("::").next().unwrap_or(type_name);
    let leaf = leaf.to_ascii_lowercase();
    ["annotation", "dimension", "balloon", "leader", "symbol"]
        .iter()
        .any(|token| leaf.contains(token))
}

fn is_text_property(name: &str) -> bool {
    matches!(
        name,
        "Text" | "TextLines" | "LabelText" | "FormatSpec" | "Caption" | "Title" | "Label"
    )
}

fn text_value(value: &crate::native::ValueRecord) -> Option<String> {
    value
        .attributes
        .iter()
        .find(|(name, _)| matches!(name.as_str(), "value" | "Value" | "string" | "String"))
        .map(|(_, value)| value.clone())
        .or_else(|| value.text.clone())
        .filter(|value| !value.is_empty())
}
