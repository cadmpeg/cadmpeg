// SPDX-License-Identifier: Apache-2.0
//! Rebuild counted attribute collections from current canonical values.

use crate::native::{PropertyRecord, ValueRecord};
use cadmpeg_ir::attributes::{AttributeValue, SourceAttribute};
use std::collections::HashMap;

/// Outer `None` means another carrier owns this property family.
pub(super) fn write(
    attribute: &SourceAttribute,
    property: &mut PropertyRecord,
    names: &HashMap<String, String>,
) -> Result<Option<Option<String>>, cadmpeg_core::CodecError> {
    let (root_tag, child_tag, key) = match property.type_name.as_str() {
        "App::PropertyStringList" => ("StringList", "String", "value"),
        "App::PropertyIntegerList" => ("IntegerList", "I", "v"),
        "App::PropertyIntegerSet" => ("IntegerSet", "I", "v"),
        _ => return Ok(None),
    };
    let set = property.type_name == "App::PropertyIntegerSet";
    if set
        && attribute.values.windows(2).any(|pair| {
            !matches!((&pair[0], &pair[1]), (AttributeValue::Integer(a), AttributeValue::Integer(b)) if a < b)
        })
    {
        return Ok(Some(None));
    }
    let Some(values) = property.values_mut() else {
        return Ok(Some(None));
    };
    let Some(root) = values
        .iter()
        .find(|value| value.parent.is_none() && value.tag == root_tag)
    else {
        return Ok(Some(None));
    };
    if values.iter().any(|value| {
        value.order != root.order && (value.parent != Some(root.order) || value.tag != child_tag)
    }) {
        return Ok(Some(None));
    }
    let mut root = root.clone();
    let children = values
        .iter()
        .filter(|value| value.parent == Some(root.order))
        .collect::<Vec<_>>();
    root.order = 0;
    root.parent = None;
    root.attributes
        .insert("count".into(), attribute.values.len().to_string());
    let mut rebuilt = vec![root];
    for (index, replacement) in attribute.values.iter().enumerate() {
        let text = match replacement {
            AttributeValue::String(value) if child_tag == "String" => value.clone(),
            AttributeValue::Integer(value) if child_tag == "I" => value.to_string(),
            _ => return Ok(Some(None)),
        };
        let mut child = children
            .get(index)
            .map(|value| (*value).clone())
            .unwrap_or(ValueRecord {
                tag: child_tag.into(),
                order: 0,
                parent: None,
                item_index: None,
                link_index: None,
                attributes: Default::default(),
                decoded_json: None,
                text: None,
            });
        child.order = index + 1;
        child.parent = Some(0);
        child.attributes.insert(key.into(), text);
        rebuilt.push(child);
    }
    *values = rebuilt.into();
    Ok(Some(super::encode(property, names)?))
}
