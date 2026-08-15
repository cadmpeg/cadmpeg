// SPDX-License-Identifier: Apache-2.0
//! Support attachment and frame recovery.

use std::collections::HashMap;

use cadmpeg_core::CodecError;

use crate::native::{AttachmentRecord, ObjectRecord, PropertyRecord};

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
            let support = unique_property(owned, "Support")?;
            let mode = unique_property(owned, "MapMode")?;
            let placement = placement_matrix(unique_property(owned, "Placement")?)?;
            let offset = placement_matrix(unique_property(owned, "AttachmentOffset")?)?;
            if support.is_none() && mode.is_none() && placement.is_none() && offset.is_none() {
                return Ok(None);
            }
            let effective_frame = effective_frame(placement, offset);
            Ok(Some(AttachmentRecord {
                id: crate::native::native_id("attachment", &object.name),
                object: object.id.clone(),
                supports: support.map_or_else(Vec::new, |property| property.links.clone()),
                map_mode: mode.map(property_text).transpose()?.flatten(),
                placement,
                offset,
                effective_frame,
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
        return Err(CodecError::Malformed(format!(
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

fn property_text(property: &PropertyRecord) -> Result<Option<String>, CodecError> {
    let values = property
        .values
        .iter()
        .filter_map(|value| {
            value
                .attributes
                .iter()
                .find(|(name, _)| matches!(name.as_str(), "value" | "Value"))
                .map(|(_, value)| value.clone())
                .or_else(|| value.text.clone())
        })
        .collect::<Vec<_>>();
    match values.as_slice() {
        [] => Ok(None),
        [value] => Ok(Some(value.clone())),
        _ => Err(CodecError::Malformed(format!(
            "attachment property {} has multiple text values",
            property.id
        ))),
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
