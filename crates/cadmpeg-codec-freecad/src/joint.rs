// SPDX-License-Identifier: Apache-2.0
//! Assembly joints recovered without executing Python proxy payloads.

use std::collections::{BTreeMap, HashMap};

use crate::native::{JointRecord, ObjectRecord, PropertyRecord};

pub(crate) fn transfer(
    objects: &[ObjectRecord],
    properties: &[PropertyRecord],
) -> Vec<JointRecord> {
    let by_owner = properties.iter().fold(
        HashMap::<&str, Vec<&PropertyRecord>>::new(),
        |mut map, property| {
            map.entry(&property.owner).or_default().push(property);
            map
        },
    );
    let mut output = Vec::new();
    for object in objects {
        let owned = by_owner
            .get(object.id.as_str())
            .cloned()
            .unwrap_or_default();
        let grounded = owned
            .iter()
            .any(|property| property.name == "ObjectToGround");
        let joint_type = owned
            .iter()
            .find(|property| property.name == "JointType")
            .and_then(|property| enumeration_value(property));
        if !grounded && joint_type.is_none() {
            continue;
        }
        let references = if grounded {
            links(&owned, "ObjectToGround")
        } else {
            ["Reference1", "Reference2"]
                .into_iter()
                .flat_map(|name| links(&owned, name))
                .collect()
        };
        let placements = if grounded {
            placement(&owned, "Placement").into_iter().collect()
        } else {
            ["Placement1", "Placement2"]
                .into_iter()
                .filter_map(|name| placement(&owned, name))
                .collect()
        };
        let parameters = owned
            .iter()
            .filter(|property| {
                matches!(
                    property.name.as_str(),
                    "Angle"
                        | "AngleMin"
                        | "AngleMax"
                        | "Distance"
                        | "Distance2"
                        | "LengthMin"
                        | "LengthMax"
                        | "EnableAngleMin"
                        | "EnableAngleMax"
                        | "EnableLengthMin"
                        | "EnableLengthMax"
                        | "Detach1"
                        | "Detach2"
                        | "Suppressed"
                )
            })
            .filter_map(|property| {
                Some((
                    property.name.clone(),
                    property
                        .values
                        .iter()
                        .find_map(|value| value.attributes.get("value"))?
                        .clone(),
                ))
            })
            .collect::<BTreeMap<_, _>>();
        output.push(JointRecord {
            id: crate::native::native_id("joint", &object.name),
            object: object.id.clone(),
            kind: if grounded {
                "grounded".into()
            } else {
                joint_type.unwrap_or_else(|| "unknown".into())
            },
            references,
            placements,
            parameters,
        });
    }
    output
}

fn enumeration_value(property: &PropertyRecord) -> Option<String> {
    let index = property
        .values
        .iter()
        .find(|value| value.tag == "Integer")?
        .attributes
        .get("value")?
        .parse::<usize>()
        .ok()?;
    property
        .values
        .iter()
        .filter(|value| value.tag == "Enum")
        .nth(index)
        .and_then(|value| value.attributes.get("value"))
        .cloned()
        .or_else(|| Some(index.to_string()))
}

fn links(properties: &[&PropertyRecord], name: &str) -> Vec<crate::native::LinkTarget> {
    properties
        .iter()
        .find(|property| property.name == name)
        .map(|property| property.links.clone())
        .unwrap_or_default()
}

fn placement(properties: &[&PropertyRecord], name: &str) -> Option<[[f64; 4]; 4]> {
    crate::product::placement_matrix(properties.iter().find(|property| property.name == name)?)
}
