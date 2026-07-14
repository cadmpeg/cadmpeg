// SPDX-License-Identifier: Apache-2.0
//! Product containers and link occurrences recovered from the application graph.

use std::collections::HashMap;

use crate::native::{ObjectRecord, ProductNodeRecord, PropertyRecord};

pub(crate) fn transfer(
    objects: &[ObjectRecord],
    properties: &[PropertyRecord],
) -> Vec<ProductNodeRecord> {
    let by_owner = properties.iter().fold(
        HashMap::<&str, Vec<&PropertyRecord>>::new(),
        |mut map, property| {
            map.entry(&property.owner).or_default().push(property);
            map
        },
    );
    objects
        .iter()
        .filter_map(|object| {
            let kind = product_kind(&object.type_name)?;
            let owned = by_owner
                .get(object.id.as_str())
                .cloned()
                .unwrap_or_default();
            let group = owned.iter().find(|property| property.name == "Group");
            let linked = owned
                .iter()
                .find(|property| property.name == "LinkedObject");
            let prototype_link = linked.and_then(|property| property.links.first());
            let placement = owned
                .iter()
                .find(|property| property.name == "LinkPlacement")
                .or_else(|| owned.iter().find(|property| property.name == "Placement"));
            Some(ProductNodeRecord {
                id: format!("fcstd:product:{}", object.name),
                object: object.id.clone(),
                kind: kind.into(),
                members: group
                    .into_iter()
                    .flat_map(|property| &property.links)
                    .filter_map(|link| link.object.clone())
                    .collect(),
                prototype: prototype_link.and_then(|link| link.object.clone()),
                external_document: prototype_link.and_then(|link| link.document.clone()),
                local_transform: placement.and_then(|property| placement_matrix(property)),
                placement_property: placement.map(|property| property.id.clone()),
                element_count: scalar(&owned, "ElementCount").and_then(|value| value.parse().ok()),
                link_transform: scalar(&owned, "LinkTransform").and_then(parse_bool),
            })
        })
        .collect()
}

fn product_kind(kind: &str) -> Option<&'static str> {
    if kind.contains("LinkGroup") {
        Some("link_group")
    } else if kind.contains("App::Link") {
        Some("occurrence")
    } else if kind.contains("App::Part") {
        Some("part")
    } else if kind.contains("Group") {
        Some("group")
    } else {
        None
    }
}

fn scalar<'a>(properties: &'a [&PropertyRecord], name: &str) -> Option<&'a str> {
    properties
        .iter()
        .find(|property| property.name == name)?
        .values
        .iter()
        .find_map(|value| value.attributes.get("value").map(String::as_str))
}

fn parse_bool(value: &str) -> Option<bool> {
    match value.to_ascii_lowercase().as_str() {
        "true" | "1" => Some(true),
        "false" | "0" => Some(false),
        _ => None,
    }
}

fn placement_matrix(property: &PropertyRecord) -> Option<[[f64; 4]; 4]> {
    let value = property
        .values
        .iter()
        .find(|value| value.tag == "PropertyPlacement")?;
    let number = |name: &str, default: f64| {
        value
            .attributes
            .get(name)
            .and_then(|value| value.parse().ok())
            .unwrap_or(default)
    };
    let (x, y, z, w) = (
        number("Q0", 0.0),
        number("Q1", 0.0),
        number("Q2", 0.0),
        number("Q3", 1.0),
    );
    let norm = (x * x + y * y + z * z + w * w).sqrt();
    if norm <= f64::EPSILON {
        return None;
    }
    let (x, y, z, w) = (x / norm, y / norm, z / norm, w / norm);
    Some([
        [
            1.0 - 2.0 * (y * y + z * z),
            2.0 * (x * y - z * w),
            2.0 * (x * z + y * w),
            number("Px", 0.0),
        ],
        [
            2.0 * (x * y + z * w),
            1.0 - 2.0 * (x * x + z * z),
            2.0 * (y * z - x * w),
            number("Py", 0.0),
        ],
        [
            2.0 * (x * z - y * w),
            2.0 * (y * z + x * w),
            1.0 - 2.0 * (x * x + y * y),
            number("Pz", 0.0),
        ],
        [0.0, 0.0, 0.0, 1.0],
    ])
}
