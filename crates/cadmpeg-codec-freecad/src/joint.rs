// SPDX-License-Identifier: Apache-2.0
//! Assembly joints recovered without executing Python proxy payloads.

use std::collections::{BTreeMap, HashMap};

use crate::native::{JointRecord, ObjectRecord, PropertyRecord};
use cadmpeg_ir::products::{
    AssemblyJoint, Component, ComponentReference, JointId, JointKind, JointLimits, JointOperand,
    Occurrence,
};

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
        let (references, placements, offsets) = if grounded {
            (
                links(&owned, "ObjectToGround"),
                placement(&owned, "Placement").into_iter().collect(),
                Vec::new(),
            )
        } else {
            let slots = [
                (
                    links(&owned, "Reference1"),
                    placement(&owned, "Placement1"),
                    placement(&owned, "Offset1"),
                ),
                (
                    links(&owned, "Reference2"),
                    placement(&owned, "Placement2"),
                    placement(&owned, "Offset2"),
                ),
            ]
            .into_iter()
            .filter(|(references, _, _)| !references.is_empty())
            .collect::<Vec<_>>();
            let references = slots
                .iter()
                .flat_map(|(references, _, _)| references.clone())
                .collect();
            let placements = slots
                .iter()
                .map(|(_, placement, _)| placement.unwrap_or_else(crate::product::identity))
                .collect();
            let offsets = slots
                .iter()
                .map(|(_, _, offset)| offset.unwrap_or_else(crate::product::identity))
                .collect();
            (references, placements, offsets)
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
            offsets,
            parameters,
        });
    }
    output
}

pub(crate) fn transfer_neutral(
    records: &[JointRecord],
    components: &[Component],
    occurrences: &[Occurrence],
) -> Vec<AssemblyJoint> {
    let component_by_native = components
        .iter()
        .filter_map(|component| {
            component
                .native_ref
                .as_deref()
                .map(|native| (native, &component.id))
        })
        .collect::<HashMap<_, _>>();
    let component_by_occurrence_native = occurrences
        .iter()
        .filter_map(|occurrence| {
            let native = occurrence.native_ref.as_deref()?;
            let ComponentReference::Local { component } = &occurrence.prototype else {
                return None;
            };
            Some((native, component))
        })
        .collect::<HashMap<_, _>>();
    records
        .iter()
        .map(|record| {
            let bool_value = |name: &str| {
                record
                    .parameters
                    .get(name)
                    .and_then(|value| parse_bool(value))
            };
            let scalar = |name: &str| {
                record
                    .parameters
                    .get(name)
                    .and_then(|value| value.parse().ok())
            };
            let enabled_limits =
                |minimum: &str, maximum: &str, enable_min: &str, enable_max: &str, scale: f64| {
                    let minimum = bool_value(enable_min)
                        .unwrap_or(false)
                        .then(|| scalar(minimum))
                        .flatten()
                        .map(|value: f64| value * scale);
                    let maximum = bool_value(enable_max)
                        .unwrap_or(false)
                        .then(|| scalar(maximum))
                        .flatten()
                        .map(|value: f64| value * scale);
                    (minimum.is_some() || maximum.is_some())
                        .then_some(JointLimits { minimum, maximum })
                };
            AssemblyJoint {
                id: JointId(crate::native::model_id(
                    "joint",
                    &record.object,
                    "constraint",
                )),
                kind: joint_kind(&record.kind),
                operands: record
                    .references
                    .iter()
                    .map(|reference| JointOperand {
                        component: reference
                            .object
                            .as_deref()
                            .and_then(|object| {
                                component_by_native
                                    .get(object)
                                    .copied()
                                    .or_else(|| component_by_occurrence_native.get(object).copied())
                            })
                            .cloned(),
                        external_document: reference.document.as_deref().map(|document| {
                            crate::product::external_document_reference(
                                document,
                                reference.document_attribute.as_deref(),
                            )
                        }),
                        object: reference.object.clone(),
                        subelements: reference.subelements.clone(),
                    })
                    .collect(),
                frames: record.placements.clone(),
                offset_frames: record.offsets.clone(),
                suppressed: bool_value("Suppressed").unwrap_or(false),
                detached: [
                    bool_value("Detach1").unwrap_or(false),
                    bool_value("Detach2").unwrap_or(false),
                ],
                angle: scalar("Angle").map(f64::to_radians),
                distance: scalar("Distance"),
                distance2: scalar("Distance2"),
                angular_limits: enabled_limits(
                    "AngleMin",
                    "AngleMax",
                    "EnableAngleMin",
                    "EnableAngleMax",
                    std::f64::consts::PI / 180.0,
                ),
                linear_limits: enabled_limits(
                    "LengthMin",
                    "LengthMax",
                    "EnableLengthMin",
                    "EnableLengthMax",
                    1.0,
                ),
                properties: record.parameters.clone(),
                native_ref: Some(record.id.clone()),
            }
        })
        .collect()
}

fn joint_kind(kind: &str) -> JointKind {
    match kind.to_ascii_lowercase().as_str() {
        "fixed" => JointKind::Fixed,
        "revolute" => JointKind::Revolute,
        "slider" | "prismatic" => JointKind::Slider,
        "cylindrical" => JointKind::Cylindrical,
        "ball" | "spherical" => JointKind::Ball,
        "distance" => JointKind::Distance,
        "parallel" => JointKind::Parallel,
        "perpendicular" => JointKind::Perpendicular,
        "angle" => JointKind::Angle,
        "rackpinion" | "rack_pinion" => JointKind::RackPinion,
        "screw" => JointKind::Screw,
        "gears" => JointKind::Gears,
        "belt" => JointKind::Belt,
        "grounded" => JointKind::Grounded,
        _ => JointKind::Native(kind.to_owned()),
    }
}

fn parse_bool(value: &str) -> Option<bool> {
    match value.to_ascii_lowercase().as_str() {
        "true" | "1" => Some(true),
        "false" | "0" => Some(false),
        _ => None,
    }
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
        .map(|property| {
            property
                .links
                .iter()
                .filter(|link| {
                    link.document.is_some()
                        || link
                            .object
                            .as_deref()
                            .is_some_and(|object| !object.is_empty())
                })
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

fn placement(properties: &[&PropertyRecord], name: &str) -> Option<[[f64; 4]; 4]> {
    crate::product::placement_matrix(properties.iter().find(|property| property.name == name)?)
}

#[cfg(test)]
mod tests {
    use super::joint_kind;

    #[test]
    fn every_primary_joint_family_has_a_neutral_variant() {
        for family in [
            "Fixed",
            "Revolute",
            "Cylindrical",
            "Slider",
            "Ball",
            "Distance",
            "Parallel",
            "Perpendicular",
            "Angle",
            "RackPinion",
            "Screw",
            "Gears",
            "Belt",
            "grounded",
        ] {
            assert!(
                !matches!(joint_kind(family), cadmpeg_ir::JointKind::Native(_)),
                "{family} must not fall through to a native joint family"
            );
        }
    }
}
