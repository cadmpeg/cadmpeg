// SPDX-License-Identifier: Apache-2.0
//! Primitive feature-local to document-model frame admission.

use super::property;
use crate::native::{sole_named_property, ObjectRecord, PropertyRecord};
use crate::product::ParentMap;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::geometry::FeatureRigidPlacement;
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

/// The frame of primitive `object` in the document: its `Placement` after
/// the placement of each coordinate-system group that holds it in turn.
pub(crate) fn resolve(
    object: &ObjectRecord,
    parents: &ParentMap<'_>,
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
        let Some(parent) = parents.coordinate(current) else {
            return Ok(FeatureRigidPlacement::new(frame));
        };
        let owned = properties.get(parent).map_or(&[][..], Vec::as_slice);
        let Some(parent_frame) = read_frame(owned)? else {
            return Ok(None);
        };
        let Ok(composed) = parent_frame.compose(frame) else {
            return Ok(None);
        };
        frame = composed;
        current = parent;
    }
    Ok(None)
}

fn read_frame(
    properties: &[&PropertyRecord],
) -> Result<Option<cadmpeg_ir::transform::Transform>, CodecError> {
    Ok(sole_named_property("primitive", properties, "Placement")?
        .and_then(crate::placement::placement_matrix_unreported)
        .map(crate::native::frame::FiniteFrame::transform))
}
