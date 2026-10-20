// SPDX-License-Identifier: Apache-2.0
//! Support attachment and frame recovery.

use std::collections::HashMap;

use cadmpeg_core::CodecError;

use crate::native::{
    sole_named_property, AttachmentRecord, LinkTarget, ObjectRecord, PropertyRecord,
};

const MAP_MODE_NAMES: &[&str] = &[
    "Deactivated",
    "Translate",
    "ObjectXY",
    "ObjectXZ",
    "ObjectYZ",
    "FlatFace",
    "TangentPlane",
    "NormalToEdge",
    "FrenetNB",
    "FrenetTN",
    "FrenetTB",
    "Concentric",
    "SectionOfRevolution",
    "ThreePointsPlane",
    "ThreePointsNormal",
    "Folding",
    "ObjectX",
    "ObjectY",
    "ObjectZ",
    "AxisOfCurvature",
    "Directrix1",
    "Directrix2",
    "Asymptote1",
    "Asymptote2",
    "Tangent",
    "Normal",
    "Binormal",
    "TangentU",
    "TangentV",
    "TwoPointLine",
    "IntersectionLine",
    "ProximityLine",
    "ObjectOrigin",
    "Focus1",
    "Focus2",
    "OnEdge",
    "CenterOfCurvature",
    "CenterOfMass",
    "IntersectionPoint",
    "Vertex",
    "ProximityPoint1",
    "ProximityPoint2",
    "AxisOfInertia1",
    "AxisOfInertia2",
    "AxisOfInertia3",
    "InertialCS",
    "FaceNormal",
    "OZX",
    "OZY",
    "OXY",
    "OXZ",
    "OYZ",
    "OYX",
    "ParallelPlane",
    "MidPoint",
];

/// An index in the attachment map-mode table.
///
/// The wire spelling is the decimal index as a JSON string, which is what
/// `App::PropertyEnumeration` carries in the source document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(try_from = "String")]
pub(crate) struct MapModeIndex(u8);

impl MapModeIndex {
    fn try_new(index: usize) -> Result<Self, String> {
        if index >= MAP_MODE_NAMES.len() {
            return Err(format!("map_mode index {index} is out of range"));
        }
        u8::try_from(index)
            .map(Self)
            .map_err(|_| format!("map_mode index {index} exceeds its storage range"))
    }
}

impl TryFrom<&str> for MapModeIndex {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        let index = value
            .parse::<usize>()
            .map_err(|_| format!("map_mode {value:?} is not an index"))?;
        Self::try_new(index)
    }
}

impl TryFrom<String> for MapModeIndex {
    type Error = String;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::try_from(value.as_str())
    }
}

impl std::fmt::Display for MapModeIndex {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

impl serde::Serialize for MapModeIndex {
    /// Writes the [`Display`](std::fmt::Display) spelling as a JSON string.
    /// `collect_str` hands the serializer the formatting rather than a built
    /// `String`, so a writer-backed serializer formats the digits straight into
    /// its output and the value serializer owns one string instead of two.
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

pub(crate) fn transfer(
    objects: &[ObjectRecord],
    properties: &[PropertyRecord],
) -> Result<Vec<AttachmentRecord>, CodecError> {
    let mut by_owner = HashMap::<&str, Vec<&PropertyRecord>>::new();
    for property in properties {
        let owner = property.owner.as_str();
        let owned = by_owner.entry(owner).or_default();
        owned.push(property);
    }
    let mut records = Vec::new();
    for object in objects {
        let Some(owned) = by_owner.get(object.id().as_str()) else {
            continue;
        };
        let support = sole_named_property("attachment", owned, "AttachmentSupport")?;
        let mode = sole_named_property("attachment", owned, "MapMode")?;
        let placement = placement_matrix(sole_named_property("attachment", owned, "Placement")?)?;
        let offset = placement_matrix(sole_named_property(
            "attachment",
            owned,
            "AttachmentOffset",
        )?)?;
        if support.is_none() && mode.is_none() && placement.is_none() && offset.is_none() {
            continue;
        }
        let record = AttachmentRecord::try_new(
            crate::native::native_id("attachment", object.name()),
            object.id().to_owned(),
            support.map(support_links).transpose()?.unwrap_or_default(),
            mode.map(map_mode_value).transpose()?,
            placement,
            offset,
        )
        .map_err(CodecError::Malformed)?;
        records.push(record);
    }
    Ok(records)
}

pub(crate) fn effective_frame(
    placement: Option<[[f64; 4]; 4]>,
    offset: Option<[[f64; 4]; 4]>,
) -> [[f64; 4]; 4] {
    match (placement, offset) {
        (Some(placement), Some(offset)) => crate::product::multiply(placement, offset),
        (Some(placement), None) => placement,
        (None, Some(offset)) => offset,
        (None, None) => IDENTITY,
    }
}

fn placement_matrix(
    property: Option<&PropertyRecord>,
) -> Result<Option<crate::native::frame::FiniteFrame>, CodecError> {
    let Some(property) = property else {
        return Ok(None);
    };
    crate::placement::placement_matrix(property)
}

fn support_links(property: &PropertyRecord) -> Result<Vec<Option<LinkTarget>>, CodecError> {
    if property.type_name != "App::PropertyLinkSubList" {
        return Err(CodecError::malformed(format_args!(
            "attachment property {} has runtime type {}, expected App::PropertyLinkSubList",
            property.id, property.type_name
        )));
    }
    if property
        .values()
        .first()
        .is_none_or(|value| value.tag != "LinkSubList")
        || property.values()[1..]
            .iter()
            .any(|value| value.tag != "Link")
    {
        return Err(CodecError::malformed(format_args!(
            "attachment property {} requires one LinkSubList value",
            property.id
        )));
    }
    let mut links = Vec::new();
    for link in property.links() {
        links.push(link.clone());
    }
    Ok(links)
}

fn map_mode_value(property: &PropertyRecord) -> Result<MapModeIndex, CodecError> {
    if property.type_name != "App::PropertyEnumeration" {
        return Err(CodecError::malformed(format_args!(
            "attachment property {} has runtime type {}, expected App::PropertyEnumeration",
            property.id, property.type_name
        )));
    }
    let [value] = property.values() else {
        return Err(CodecError::malformed(format_args!(
            "attachment property {} requires one Integer value",
            property.id
        )));
    };
    if value.tag != "Integer" {
        return Err(CodecError::malformed(format_args!(
            "attachment property {} requires an Integer value",
            property.id
        )));
    }
    let Some(index) = value.attributes.get("value") else {
        return Err(CodecError::malformed(format_args!(
            "attachment property {} has no enum index",
            property.id
        )));
    };
    let mode = match index.parse::<usize>() {
        Ok(index) => MapModeIndex::try_new(index),
        Err(_) => Err(format!("map_mode {index:?} is not an index")),
    };
    mode.map_err(|error| {
        CodecError::malformed(format_args!("attachment property {}: {error}", property.id))
    })
}

const IDENTITY: [[f64; 4]; 4] = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
];

#[cfg(test)]
mod tests;
