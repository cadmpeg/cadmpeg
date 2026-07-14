// SPDX-License-Identifier: Apache-2.0
//! Support attachment and frame recovery.

use std::collections::HashMap;

use crate::native::{AttachmentRecord, ObjectRecord, PropertyRecord};

pub(crate) fn transfer(
    objects: &[ObjectRecord],
    properties: &[PropertyRecord],
) -> Vec<AttachmentRecord> {
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
            let owned = by_owner.get(object.id.as_str())?;
            let named = |name: &str| owned.iter().copied().find(|property| property.name == name);
            let support = named("Support");
            let mode = named("MapMode");
            let placement = named("Placement").and_then(crate::product::placement_matrix);
            let offset = named("AttachmentOffset").and_then(crate::product::placement_matrix);
            if support.is_none() && mode.is_none() && placement.is_none() && offset.is_none() {
                return None;
            }
            let effective_frame = placement.or(offset).unwrap_or(IDENTITY);
            Some(AttachmentRecord {
                id: crate::native::native_id("attachment", &object.name),
                object: object.id.clone(),
                supports: support.map_or_else(Vec::new, |property| property.links.clone()),
                map_mode: mode.and_then(property_text),
                placement,
                offset,
                effective_frame,
            })
        })
        .collect()
}

fn property_text(property: &PropertyRecord) -> Option<String> {
    property.values.iter().find_map(|value| {
        value
            .attributes
            .iter()
            .find(|(name, _)| matches!(name.as_str(), "value" | "Value"))
            .map(|(_, value)| value.clone())
            .or_else(|| value.text.clone())
    })
}

const IDENTITY: [[f64; 4]; 4] = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
];
