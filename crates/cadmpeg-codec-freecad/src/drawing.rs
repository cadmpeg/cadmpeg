// SPDX-License-Identifier: Apache-2.0
//! `TechDraw` page and view graph recovery.

use std::collections::{BTreeMap, HashMap};

use crate::native::{DrawingRecord, ObjectRecord, PropertyRecord};

pub(crate) fn transfer(
    objects: &[ObjectRecord],
    properties: &[PropertyRecord],
) -> Vec<DrawingRecord> {
    let by_owner = properties.iter().fold(
        HashMap::<&str, Vec<&PropertyRecord>>::new(),
        |mut map, property| {
            map.entry(&property.owner).or_default().push(property);
            map
        },
    );
    objects
        .iter()
        .filter(|object| object.type_name.contains("TechDraw::"))
        .map(|object| {
            let owned = by_owner
                .get(object.id.as_str())
                .cloned()
                .unwrap_or_default();
            DrawingRecord {
                id: format!("fcstd:drawing:{}", object.name),
                object: object.id.clone(),
                kind: object.type_name.clone(),
                views: links(&owned, "Views")
                    .into_iter()
                    .filter_map(|link| link.object)
                    .collect(),
                template: links(&owned, "Template")
                    .into_iter()
                    .find_map(|link| link.object),
                sources: ["Source", "References2D", "References3D"]
                    .into_iter()
                    .flat_map(|name| links(&owned, name))
                    .collect(),
                parameters: drawing_parameters(&owned),
                side_entries: owned
                    .iter()
                    .flat_map(|property| &property.side_entries)
                    .cloned()
                    .collect(),
            }
        })
        .collect()
}

fn links(properties: &[&PropertyRecord], name: &str) -> Vec<crate::native::LinkTarget> {
    properties
        .iter()
        .find(|property| property.name == name)
        .map(|property| property.links.clone())
        .unwrap_or_default()
}

fn drawing_parameters(properties: &[&PropertyRecord]) -> BTreeMap<String, String> {
    const NAMES: &[&str] = &[
        "X",
        "Y",
        "Scale",
        "ScaleType",
        "Direction",
        "Rotation",
        "Caption",
        "FormatSpec",
        "MeasureType",
        "ProjectionType",
        "KeepLabel",
        "LockPosition",
    ];
    properties
        .iter()
        .filter(|property| NAMES.contains(&property.name.as_str()))
        .map(|property| {
            let value = property
                .values
                .first()
                .map(|value| value.raw_xml.clone())
                .unwrap_or_default();
            (property.name.clone(), value)
        })
        .collect()
}
