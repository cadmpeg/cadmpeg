// SPDX-License-Identifier: Apache-2.0
//! Support attachment and frame recovery.

use std::collections::HashMap;

use cadmpeg_core::decode::DecodeContext;
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

#[derive(Debug)]
enum MapModeIssue {
    OutOfRange(usize),
    StorageRange(usize),
}

impl std::fmt::Display for MapModeIssue {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OutOfRange(index) => write!(formatter, "map_mode index {index} is out of range"),
            Self::StorageRange(index) => write!(
                formatter,
                "map_mode index {index} exceeds its storage range"
            ),
        }
    }
}

impl MapModeIndex {
    fn try_new(index: usize) -> Result<Self, MapModeIssue> {
        if index >= MAP_MODE_NAMES.len() {
            return Err(MapModeIssue::OutOfRange(index));
        }
        u8::try_from(index)
            .map(Self)
            .map_err(|_| MapModeIssue::StorageRange(index))
    }
}

impl TryFrom<&str> for MapModeIndex {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        let index = value
            .parse::<usize>()
            .map_err(|_| format!("map_mode {value:?} is not an index"))?;
        Self::try_new(index).map_err(|issue| issue.to_string())
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
    ctx: &DecodeContext<'_>,
    objects: &[ObjectRecord],
    properties: &[PropertyRecord],
) -> Result<Vec<AttachmentRecord>, CodecError> {
    let mut owner_storage = ctx.reserve_scoped(0, "FreeCAD attachment owner storage")?;
    if objects.is_empty() || properties.is_empty() {
        return Ok(Vec::new());
    }
    let mut by_owner = HashMap::<&str, Vec<&PropertyRecord>>::new();
    let mut candidates = properties.iter();
    while candidates.len() != 0 {
        let Some(property) = ctx.next_charged(&mut candidates, "FreeCAD attachment properties")?
        else {
            break;
        };
        if !is_attachment_property(&property.name) {
            continue;
        }
        let owner = property.owner.as_str();
        if !owner.starts_with("fcstd:native:object#") {
            continue;
        }
        owner_storage.with_storage(|| {
            ctx.push_hash_group(
                &mut by_owner,
                owner,
                property,
                "FreeCAD attachment owner lookup",
                "FreeCAD attachment owner properties",
            )
        })?;
    }
    if by_owner.is_empty() {
        return Ok(Vec::new());
    }
    let mut records = Vec::new();
    let mut objects = objects.iter();
    while objects.len() != 0 {
        let Some(object) = ctx.next_charged(&mut objects, "FreeCAD attachment objects")? else {
            break;
        };
        let Some(owned) = ctx.get_hash_map(
            &by_owner,
            object.id().as_str(),
            "FreeCAD attachment object properties",
        )?
        else {
            continue;
        };
        let support = sole_named_property(ctx, "attachment", owned, "AttachmentSupport")?;
        let mode = sole_named_property(ctx, "attachment", owned, "MapMode")?;
        let placement = sole_named_property(ctx, "attachment", owned, "Placement")?
            .map(|property| crate::placement::placement_matrix(ctx, property))
            .transpose()?
            .flatten();
        let offset = sole_named_property(ctx, "attachment", owned, "AttachmentOffset")?
            .map(|property| crate::placement::placement_matrix(ctx, property))
            .transpose()?
            .flatten();
        if support.is_none() && mode.is_none() && placement.is_none() && offset.is_none() {
            continue;
        }
        let record = AttachmentRecord::try_new(
            crate::native::native_id_charged(ctx, "attachment", object.name())?,
            ctx.copy_retained_text(object.id(), "FreeCAD attachment object")?,
            support
                .map(|property| support_links(ctx, property))
                .transpose()?
                .unwrap_or_default(),
            mode.map(|property| map_mode_value(ctx, property))
                .transpose()?,
            placement,
            offset,
        )
        .map_err(CodecError::Malformed)?;
        ctx.reserve_vec(&mut records, 1, "FreeCAD attachment records")?;
        records.push(record);
    }
    Ok(records)
}

fn is_attachment_property(name: &str) -> bool {
    matches!(
        name,
        "AttachmentSupport" | "MapMode" | "Placement" | "AttachmentOffset"
    )
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

fn support_links(
    ctx: &DecodeContext<'_>,
    property: &PropertyRecord,
) -> Result<Vec<Option<LinkTarget>>, CodecError> {
    if property.type_name != "App::PropertyLinkSubList" {
        return Err(CodecError::Malformed(ctx.format_retained(
            format_args!(
                "attachment property {} has runtime type {}, expected App::PropertyLinkSubList",
                property.id, property.type_name
            ),
            "FreeCAD attachment support type error",
        )?));
    }
    if property
        .values()
        .first()
        .is_none_or(|value| value.tag != "LinkSubList")
        || ctx.any_by(
            &property.values()[1..],
            |value| Ok(value.tag != "Link"),
            "FreeCAD attachment support values",
        )?
    {
        return Err(CodecError::Malformed(ctx.format_retained(
            format_args!(
                "attachment property {} requires one LinkSubList value",
                property.id
            ),
            "FreeCAD attachment support value error",
        )?));
    }
    let mut links =
        ctx.vector_storage(property.links().len(), "FreeCAD attachment support links")?;
    let mut input = property.links().iter();
    while input.len() != 0 {
        let Some(link) = ctx.next_charged(&mut input, "FreeCAD attachment support link visits")?
        else {
            break;
        };
        ctx.push_vec(
            &mut links,
            link.as_ref()
                .map(|link| link.clone_with_context(ctx))
                .transpose()?,
            "FreeCAD attachment support links",
        )?;
    }
    Ok(links)
}

fn map_mode_value(
    ctx: &DecodeContext<'_>,
    property: &PropertyRecord,
) -> Result<MapModeIndex, CodecError> {
    if property.type_name != "App::PropertyEnumeration" {
        return Err(CodecError::Malformed(ctx.format_retained(
            format_args!(
                "attachment property {} has runtime type {}, expected App::PropertyEnumeration",
                property.id, property.type_name
            ),
            "FreeCAD attachment map-mode type error",
        )?));
    }
    let [value] = property.values() else {
        return Err(CodecError::Malformed(ctx.format_retained(
            format_args!(
                "attachment property {} requires one Integer value",
                property.id
            ),
            "FreeCAD attachment map-mode value error",
        )?));
    };
    if value.tag != "Integer" {
        return Err(CodecError::Malformed(ctx.format_retained(
            format_args!(
                "attachment property {} requires an Integer value",
                property.id
            ),
            "FreeCAD attachment map-mode tag error",
        )?));
    }
    let Some(index) = ctx.get_btree_map(
        &value.attributes,
        "value",
        "FreeCAD attachment map-mode lookup",
    )?
    else {
        return Err(CodecError::Malformed(ctx.format_retained(
            format_args!("attachment property {} has no enum index", property.id),
            "FreeCAD attachment missing map-mode index",
        )?));
    };
    match ctx.parse_text::<usize>(index, "FreeCAD attachment map-mode parse")? {
        Ok(index) => MapModeIndex::try_new(index).map_err(|issue| {
            crate::resource::malformed_charged(
                ctx,
                format_args!("attachment property {}: {issue}", property.id),
                "FreeCAD attachment map-mode error",
            )
        }),
        Err(_) => Err(crate::resource::malformed_charged(
            ctx,
            format_args!(
                "attachment property {}: map_mode {index:?} is not an index",
                property.id
            ),
            "FreeCAD attachment map-mode error",
        )),
    }
}

const IDENTITY: [[f64; 4]; 4] = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
];

#[cfg(test)]
mod tests;
