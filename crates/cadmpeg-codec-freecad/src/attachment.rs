// SPDX-License-Identifier: Apache-2.0
//! Support attachment and frame recovery.

use std::collections::HashMap;

use cadmpeg_core::CodecError;

use crate::native::{AttachmentRecord, LinkTarget, ObjectRecord, PropertyRecord};

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
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct MapModeIndex(u8);

impl MapModeIndex {
    pub(crate) fn try_new(index: usize) -> Result<Self, String> {
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

impl From<MapModeIndex> for String {
    fn from(value: MapModeIndex) -> Self {
        value.to_string()
    }
}

pub(crate) fn transfer(
    objects: &[ObjectRecord],
    properties: &[PropertyRecord],
) -> Result<Vec<AttachmentRecord>, CodecError> {
    let by_owner = properties.iter().fold(
        HashMap::<&str, Vec<&PropertyRecord>>::new(),
        |mut map, property| {
            map.entry(&property.owner).or_default().push(property);
            map
        },
    );
    objects
        .iter()
        .map(|object| {
            let Some(owned) = by_owner.get(object.id.as_str()) else {
                return Ok(None);
            };
            let support = unique_property(owned, "AttachmentSupport")?;
            let mode = unique_property(owned, "MapMode")?;
            let placement = placement_matrix(unique_property(owned, "Placement")?)?;
            let offset = placement_matrix(unique_property(owned, "AttachmentOffset")?)?;
            if support.is_none() && mode.is_none() && placement.is_none() && offset.is_none() {
                return Ok(None);
            }
            AttachmentRecord::try_new(
                crate::native::native_id("attachment", &object.name),
                object.id.clone(),
                support.map(support_links).transpose()?.unwrap_or_default(),
                mode.map(map_mode_value).transpose()?,
                placement,
                offset,
            )
            .map(Some)
            .map_err(CodecError::Malformed)
        })
        .collect::<Result<Vec<_>, CodecError>>()
        .map(|records| records.into_iter().flatten().collect())
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

fn unique_property<'a>(
    properties: &[&'a PropertyRecord],
    name: &str,
) -> Result<Option<&'a PropertyRecord>, CodecError> {
    crate::native::unique_property(properties.iter().copied(), |property| property.name == name)
        .map_err(|_| {
            CodecError::malformed(format_args!(
                "attachment property {name} occurs more than once"
            ))
        })
}

fn placement_matrix(
    property: Option<&PropertyRecord>,
) -> Result<Option<crate::native::frame::FiniteFrame>, CodecError> {
    let Some(property) = property else {
        return Ok(None);
    };
    crate::product::placement_matrix(property)
}

fn support_links(property: &PropertyRecord) -> Result<Vec<Option<LinkTarget>>, CodecError> {
    if property.type_name != "App::PropertyLinkSubList" {
        return Err(malformed(format!(
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
        return Err(malformed(format!(
            "attachment property {} requires one LinkSubList value",
            property.id
        )));
    }
    Ok(property.links().to_vec())
}

fn map_mode_value(property: &PropertyRecord) -> Result<MapModeIndex, CodecError> {
    if property.type_name != "App::PropertyEnumeration" {
        return Err(malformed(format!(
            "attachment property {} has runtime type {}, expected App::PropertyEnumeration",
            property.id, property.type_name
        )));
    }
    let [value] = property.values() else {
        return Err(malformed(format!(
            "attachment property {} requires one Integer value",
            property.id
        )));
    };
    if value.tag != "Integer" {
        return Err(malformed(format!(
            "attachment property {} requires an Integer value",
            property.id
        )));
    }
    let index = value.attributes.get("value").ok_or_else(|| {
        malformed(format!(
            "attachment property {} has no enum index",
            property.id
        ))
    })?;
    MapModeIndex::try_from(index.as_str())
        .map_err(|error| malformed(format!("attachment property {}: {error}", property.id)))
}

fn malformed(message: impl Into<String>) -> CodecError {
    CodecError::Malformed(message.into())
}

const IDENTITY: [[f64; 4]; 4] = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
];

#[cfg(test)]
mod tests;
