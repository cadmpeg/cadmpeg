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
    transfer_impl(objects, properties, None).map(|(records, _)| records)
}

pub(crate) fn transfer_source<'source>(
    ctx: &crate::property_reader::PropertyReader<'_, 'source>,
    objects: &[ObjectRecord],
    properties: &[PropertyRecord],
    sources: &std::collections::BTreeMap<
        String,
        crate::persistence::object_parts::ObjectSource<'source>,
    >,
) -> Result<
    (
        Vec<AttachmentRecord>,
        Vec<crate::design::UnformedProjection>,
    ),
    CodecError,
> {
    transfer_impl(objects, properties, Some((ctx, sources)))
}

fn transfer_impl<'source>(
    objects: &[ObjectRecord],
    properties: &[PropertyRecord],
    producer: Option<(
        &crate::property_reader::PropertyReader<'_, 'source>,
        &std::collections::BTreeMap<
            String,
            crate::persistence::object_parts::ObjectSource<'source>,
        >,
    )>,
) -> Result<
    (
        Vec<AttachmentRecord>,
        Vec<crate::design::UnformedProjection>,
    ),
    CodecError,
> {
    let mut by_owner = HashMap::<&str, Vec<&PropertyRecord>>::new();
    for property in properties {
        let owner = property.owner.as_str();
        let owned = by_owner.entry(owner).or_default();
        owned.push(property);
    }
    let mut records = Vec::new();
    let mut unformed = Vec::new();
    for object in objects {
        let owned = by_owner
            .get(object.id().as_str())
            .map_or(&[][..], Vec::as_slice);
        let reader = producer.map(|(ctx, _)| ctx);
        if let Some(reader) = reader {
            reader.begin_projection(object.id())?;
        }
        let support = sole_named_property("attachment", owned, "AttachmentSupport")?;
        let mode = sole_named_property("attachment", owned, "MapMode")?;
        let placement_property = sole_named_property("attachment", owned, "Placement")?;
        let offset_property = sole_named_property("attachment", owned, "AttachmentOffset")?;
        if support.is_none()
            && mode.is_none()
            && placement_property.is_none()
            && offset_property.is_none()
        {
            let mut actual_role_failed = false;
            if let Some(reader) = reader {
                for name in [
                    "AttachmentSupport",
                    "MapMode",
                    "Placement",
                    "AttachmentOffset",
                ] {
                    actual_role_failed |= reader.unavailable_named_exact(name)?;
                }
            }
            if !actual_role_failed {
                continue;
            }
        }
        let missing = |property: Option<&PropertyRecord>, name: &str| -> Result<bool, CodecError> {
            match reader {
                Some(reader) => reader.missing_value(property, name),
                None => Ok(property.is_some_and(|property| property.body.is_none())),
            }
        };
        let support_missing = missing(support, "AttachmentSupport")?;
        let mode_missing = missing(mode, "MapMode")?;
        let placement_missing = missing(placement_property, "Placement")?;
        let offset_missing = missing(offset_property, "AttachmentOffset")?;
        if support.is_none()
            && mode.is_none()
            && placement_property.is_none()
            && offset_property.is_none()
            && !support_missing
            && !mode_missing
            && !placement_missing
            && !offset_missing
        {
            continue;
        }
        if support.is_none()
            && mode.is_none()
            && placement_property.is_none()
            && offset_property.is_none()
        {
            // A refused header is not a newly invented identity-only attachment.
            if let Some(reader) = reader {
                let homes = reader.take_unavailable();
                if !homes.is_empty() {
                    unformed.push(crate::design::UnformedProjection {
                        ids: vec![crate::native::native_id("attachment", object.name())],
                        homes,
                    });
                }
            }
            continue;
        }
        let supports = if support_missing {
            None
        } else {
            Some(support.map(support_links).transpose()?.unwrap_or_default())
        };
        let map_mode = if mode_missing {
            None
        } else {
            mode.map(map_mode_value).transpose()?
        };
        let placement = if placement_missing {
            None
        } else {
            placement_matrix(placement_property)?
        };
        let offset = if offset_missing {
            None
        } else {
            placement_matrix(offset_property)?
        };
        let id = crate::native::native_id("attachment", object.name());
        let construct = |actual_object: String| -> Result<AttachmentRecord, CodecError> {
            match (supports, placement_missing || offset_missing) {
                (Some(supports), false) => AttachmentRecord::try_new(
                    id.clone(),
                    actual_object,
                    supports,
                    map_mode,
                    placement,
                    offset,
                ),
                (supports, unknown_frame) => {
                    let frame = if unknown_frame {
                        None
                    } else {
                        Some(
                            crate::native::frame::FiniteFrame::try_from(effective_frame(
                                placement.map(crate::native::frame::FiniteFrame::rows),
                                offset.map(crate::native::frame::FiniteFrame::rows),
                            ))
                            .map_err(CodecError::Malformed)?,
                        )
                    };
                    AttachmentRecord::try_new_partial(
                        id.clone(),
                        actual_object,
                        supports,
                        map_mode,
                        placement,
                        offset,
                        frame,
                    )
                }
            }
            .map_err(CodecError::Malformed)
        };
        let record = match producer {
            Some((ctx, sources)) => sources
                .get(object.id())
                .ok_or_else(|| {
                    CodecError::malformed("attachment owner has no original source authority")
                })?
                .record(ctx, &id, construct)?,
            None => construct(object.id().to_owned())?,
        };
        records.push(record);
    }
    Ok((records, unformed))
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
    if property.body.is_some()
        && property.values().is_empty()
        && !property.links().is_empty()
        && property.links().iter().all(Option::is_none)
    {
        return Ok(property.links().to_vec());
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
