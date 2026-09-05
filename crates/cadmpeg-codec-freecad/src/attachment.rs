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
            Ok(Some(AttachmentRecord {
                id: crate::native::native_id("attachment", &object.name),
                object: object.id.clone(),
                supports: support.map(support_links).transpose()?.unwrap_or_default(),
                map_mode: mode.map(map_mode_value).transpose()?.flatten(),
                placement,
                offset,
            }))
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
    let mut matches = properties
        .iter()
        .copied()
        .filter(|property| property.name == name);
    let Some(property) = matches.next() else {
        return Ok(None);
    };
    if matches.next().is_some() {
        return Err(malformed(format!(
            "attachment property {name} occurs more than once"
        )));
    }
    Ok(Some(property))
}

fn placement_matrix(
    property: Option<&PropertyRecord>,
) -> Result<Option<[[f64; 4]; 4]>, CodecError> {
    let Some(property) = property else {
        return Ok(None);
    };
    crate::product::placement_matrix(property)
}

fn support_links(property: &PropertyRecord) -> Result<Vec<LinkTarget>, CodecError> {
    if property.type_name != "App::PropertyLinkSubList" {
        return Err(malformed(format!(
            "attachment property {} has runtime type {}, expected App::PropertyLinkSubList",
            property.id, property.type_name
        )));
    }
    if property
        .values
        .first()
        .is_none_or(|value| value.tag != "LinkSubList")
        || property.values[1..].iter().any(|value| value.tag != "Link")
    {
        return Err(malformed(format!(
            "attachment property {} requires one LinkSubList value",
            property.id
        )));
    }
    Ok(property.links.clone())
}

fn map_mode_value(property: &PropertyRecord) -> Result<Option<String>, CodecError> {
    if property.type_name != "App::PropertyEnumeration" {
        return Err(malformed(format!(
            "attachment property {} has runtime type {}, expected App::PropertyEnumeration",
            property.id, property.type_name
        )));
    }
    let [value] = property.values.as_slice() else {
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
    let index = value
        .attributes
        .get("value")
        .ok_or_else(|| {
            malformed(format!(
                "attachment property {} has no enum index",
                property.id
            ))
        })?
        .parse::<usize>()
        .map_err(|_| {
            malformed(format!(
                "attachment property {} has an invalid enum index",
                property.id
            ))
        })?;
    MAP_MODE_NAMES
        .get(index)
        .map(|_| Some(index.to_string()))
        .ok_or_else(|| {
            malformed(format!(
                "attachment property {} enum index {index} is out of range",
                property.id
            ))
        })
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
