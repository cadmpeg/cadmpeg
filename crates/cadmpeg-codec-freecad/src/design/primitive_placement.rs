// SPDX-License-Identifier: Apache-2.0
//! Primitive feature-local to document-model frame admission.

use super::{body_membership_property, property};
use crate::native::{sole_named_property, ObjectRecord, PropertyRecord};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::FeatureRigidPlacement;
use std::collections::{HashMap, HashSet};

pub(crate) fn carries_frame(properties: &[&PropertyRecord]) -> bool {
    [
        "Placement",
        "AttachmentOffset",
        "MapMode",
        "Support",
        "AttachmentSupport",
    ]
    .iter()
    .any(|name| property(properties, name).is_some())
}

pub(crate) fn resolve(
    object: &ObjectRecord,
    objects: &[ObjectRecord],
    properties: &HashMap<&str, Vec<&PropertyRecord>>,
) -> Result<Option<FeatureRigidPlacement>, CodecError> {
    let owned = properties
        .get(object.id().as_str())
        .map_or(&[][..], Vec::as_slice);
    if property(owned, "AttachmentSupport").is_some() && property(owned, "Support").is_some() {
        return Ok(None);
    }
    if let Some(mode) = sole_named_property("primitive", owned, "MapMode")? {
        if mode.type_name != "App::PropertyEnumeration"
            || mode.values().len() != 1
            || mode.values()[0].tag != "Integer"
            || !matches!(
                mode.values()[0].attributes.get("value").map(String::as_str),
                Some("0" | "1" | "5")
            )
        {
            return Ok(None);
        }
    } else if ["AttachmentSupport", "Support"]
        .iter()
        .any(|name| property(owned, name).is_some_and(|p| p.links().iter().any(Option::is_some)))
    {
        return Ok(None);
    }
    let Some(mut frame) = read_frame(owned)? else {
        return Ok(None);
    };
    let mut current = object.id().as_str();
    let mut seen = HashSet::new();
    while seen.insert(current) {
        let parents = objects
            .iter()
            .filter(|parent| {
                matches!(parent.type_name.as_str(), "PartDesign::Body" | "App::Part")
                    && properties
                        .get(parent.id().as_str())
                        .and_then(|p| body_membership_property(p))
                        .is_some_and(|group| {
                            group
                                .links()
                                .iter()
                                .flatten()
                                .any(|link| link.object() == Some(current))
                        })
            })
            .collect::<Vec<_>>();
        let parent = match parents.as_slice() {
            [] => return Ok(FeatureRigidPlacement::new(frame)),
            [parent] => *parent,
            _ => return Ok(None),
        };
        let owned = properties
            .get(parent.id().as_str())
            .map_or(&[][..], Vec::as_slice);
        let Some(parent_frame) = read_frame(owned)? else {
            return Ok(None);
        };
        let Ok(composed) = parent_frame.compose(frame) else {
            return Ok(None);
        };
        frame = composed;
        current = parent.id().as_str();
    }
    Ok(None)
}

fn read_frame(
    properties: &[&PropertyRecord],
) -> Result<Option<cadmpeg_ir::transform::Transform>, CodecError> {
    sole_named_property("primitive", properties, "Placement")?
        .map(crate::placement::placement_matrix)
        .transpose()
        .map(|value| {
            value
                .flatten()
                .map(crate::native::frame::FiniteFrame::transform)
        })
}
